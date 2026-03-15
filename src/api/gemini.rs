use super::{FunctionCall, LlmClient, Message, MessagePart, ResponsePart};
use crate::api::rate_limiter::{RateLimiter, RateLimiterConfig};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// A model in the fallback chain with its own rate limiter.
struct ModelSlot {
    name: String,
    rate_limiter: RateLimiter,
}

/// Gemini API client with automatic model fallback on rate limits.
///
/// When the primary model hits repeated 429s, it falls back to the next
/// model in the chain. This maximizes free-tier usage across models.
pub struct GeminiClient {
    api_key: String,
    client: reqwest::Client,
    models: Vec<ModelSlot>,
    active_model: AtomicUsize,
}

/// Configuration for a model in the fallback chain.
pub struct ModelConfig {
    pub name: String,
    pub daily_limit: u64,
    pub rpm: u32,
}

impl GeminiClient {
    /// Create a client with a single model (no fallback).
    pub fn new(api_key: String, model: String, daily_limit: u64, rpm: u32) -> Self {
        Self::with_fallback(
            api_key,
            vec![ModelConfig {
                name: model,
                daily_limit,
                rpm,
            }],
        )
    }

    /// Create a client with a fallback chain of models.
    /// Models are tried in order; falls back to next on repeated 429s.
    pub fn with_fallback(api_key: String, models: Vec<ModelConfig>) -> Self {
        assert!(!models.is_empty(), "at least one model required");

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client");

        let model_slots = models
            .into_iter()
            .map(|mc| ModelSlot {
                name: mc.name,
                rate_limiter: RateLimiter::new(
                    RateLimiterConfig {
                        rpm: mc.rpm,
                        ..Default::default()
                    },
                    mc.daily_limit,
                ),
            })
            .collect();

        Self {
            api_key,
            client,
            models: model_slots,
            active_model: AtomicUsize::new(0),
        }
    }

    fn api_url_for_model(model: &str) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        )
    }

    fn active_slot(&self) -> &ModelSlot {
        let idx = self.active_model.load(Ordering::Relaxed);
        &self.models[idx]
    }

    /// Try to fall back to the next model. Returns true if successful.
    fn try_fallback(&self) -> bool {
        let current = self.active_model.load(Ordering::Relaxed);
        let next = current + 1;
        if next < self.models.len() {
            self.active_model.store(next, Ordering::Relaxed);
            tracing::warn!(
                "Falling back from {} to {} due to rate limits",
                self.models[current].name,
                self.models[next].name
            );
            true
        } else {
            false
        }
    }

    pub fn active_model_name(&self) -> &str {
        &self.active_slot().name
    }

    fn build_request_body(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[Value],
    ) -> Value {
        let contents: Vec<Value> = messages
            .iter()
            .map(|msg| {
                let parts: Vec<Value> = msg
                    .parts
                    .iter()
                    .map(|part| match part {
                        MessagePart::Text { text } => {
                            serde_json::json!({"text": text})
                        }
                        MessagePart::FunctionCall { function_call } => {
                            serde_json::json!({
                                "functionCall": {
                                    "name": function_call.name,
                                    "args": function_call.args
                                }
                            })
                        }
                        MessagePart::FunctionResponse { function_response } => {
                            serde_json::json!({
                                "functionResponse": {
                                    "name": function_response.name,
                                    "response": function_response.response
                                }
                            })
                        }
                    })
                    .collect();

                serde_json::json!({
                    "role": msg.role,
                    "parts": parts
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "contents": contents,
            "systemInstruction": {
                "parts": [{"text": system_prompt}]
            },
            "generationConfig": {
                "temperature": 0.2,
                "maxOutputTokens": 8192
            }
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!([{
                "functionDeclarations": tools
            }]);
        }

        body
    }
}

#[async_trait]
impl LlmClient for GeminiClient {
    async fn generate(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[Value],
    ) -> Result<Vec<ResponsePart>> {
        let body = self.build_request_body(system_prompt, messages, tools);

        let mut retries = 0;
        let max_total_retries = self
            .models
            .iter()
            .map(|m| m.rate_limiter.max_retries())
            .sum::<u32>();

        loop {
            let slot = self.active_slot();
            let model_name = slot.name.clone();

            // Try to acquire from rate limiter; if daily budget exhausted, fallback
            if let Err(e) = slot.rate_limiter.acquire().await {
                tracing::warn!("Model {} budget issue: {}", model_name, e);
                if self.try_fallback() {
                    continue;
                }
                return Err(e);
            }

            let url = Self::api_url_for_model(&model_name);
            let response = self
                .client
                .post(url)
                .query(&[("key", &self.api_key)])
                .json(&body)
                .send()
                .await
                .context("failed to send request to Gemini API")?;

            let status = response.status();

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs);

                slot.rate_limiter.report_rate_limit(retry_after).await;
                retries += 1;

                // After 3 consecutive 429s on this model, try fallback
                if retries % 3 == 0 && self.try_fallback() {
                    continue;
                }

                if retries > max_total_retries {
                    anyhow::bail!(
                        "Gemini API rate limited after {} retries across all models",
                        retries
                    );
                }
                continue;
            }

            if !status.is_success() {
                let error_body = response.text().await.unwrap_or_default();
                anyhow::bail!("Gemini API error {}: {}", status.as_u16(), error_body);
            }

            slot.rate_limiter.report_success().await;

            let resp_body: Value = response
                .json()
                .await
                .context("failed to parse Gemini response")?;

            return parse_gemini_response(&resp_body);
        }
    }
}

fn parse_gemini_response(body: &Value) -> Result<Vec<ResponsePart>> {
    let mut parts = Vec::new();

    let candidates = body
        .get("candidates")
        .and_then(|c| c.as_array())
        .context("no candidates in Gemini response")?;

    for candidate in candidates {
        let content = candidate
            .get("content")
            .context("no content in candidate")?;

        if let Some(response_parts) = content.get("parts").and_then(|p| p.as_array()) {
            for part in response_parts {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    parts.push(ResponsePart::Text(text.to_string()));
                }

                if let Some(fc) = part.get("functionCall") {
                    let name = fc
                        .get("name")
                        .and_then(|n| n.as_str())
                        .context("functionCall missing name")?
                        .to_string();

                    let args = fc.get("args").cloned().unwrap_or(serde_json::json!({}));

                    parts.push(ResponsePart::FunctionCall(FunctionCall { name, args }));
                }
            }
        }
    }

    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_response() {
        let body = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hello!"}]
                }
            }]
        });

        let parts = parse_gemini_response(&body).unwrap();
        assert_eq!(parts.len(), 1);
        match &parts[0] {
            ResponsePart::Text(t) => assert_eq!(t, "Hello!"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn test_parse_function_call() {
        let body = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "bash",
                            "args": {"command": "ls"}
                        }
                    }]
                }
            }]
        });

        let parts = parse_gemini_response(&body).unwrap();
        assert_eq!(parts.len(), 1);
        match &parts[0] {
            ResponsePart::FunctionCall(fc) => {
                assert_eq!(fc.name, "bash");
                assert_eq!(fc.args["command"], "ls");
            }
            _ => panic!("expected function call"),
        }
    }

    #[test]
    fn test_parse_mixed_response() {
        let body = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        {"text": "Let me check that."},
                        {"functionCall": {"name": "read", "args": {"path": "foo.rs"}}}
                    ]
                }
            }]
        });

        let parts = parse_gemini_response(&body).unwrap();
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn test_build_request_body() {
        let client = GeminiClient::new("test-key".into(), "gemini-2.5-flash".into(), 250, 10);
        let messages = vec![Message {
            role: "user".into(),
            parts: vec![MessagePart::Text {
                text: "hello".into(),
            }],
        }];

        let body = client.build_request_body("system prompt", &messages, &[]);
        assert!(body.get("contents").is_some());
        assert!(body.get("systemInstruction").is_some());
    }

    #[test]
    fn test_fallback_chain() {
        let client = GeminiClient::with_fallback(
            "test-key".into(),
            vec![
                ModelConfig {
                    name: "gemini-2.5-flash-preview-04-17".into(),
                    daily_limit: 250,
                    rpm: 10,
                },
                ModelConfig {
                    name: "gemini-2.5-flash-lite".into(),
                    daily_limit: 1000,
                    rpm: 15,
                },
            ],
        );

        assert_eq!(client.active_model_name(), "gemini-2.5-flash-preview-04-17");
        assert!(client.try_fallback());
        assert_eq!(client.active_model_name(), "gemini-2.5-flash-lite");
        assert!(!client.try_fallback()); // no more models
    }

    #[test]
    fn test_single_model_no_fallback() {
        let client = GeminiClient::new("test-key".into(), "gemini-2.5-flash".into(), 250, 10);
        assert!(!client.try_fallback());
    }
}
