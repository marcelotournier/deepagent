use super::{FunctionCall, LlmClient, Message, MessagePart, ResponsePart};
use crate::api::rate_limiter::{RateLimiter, RateLimiterConfig};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

pub struct GeminiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
    rate_limiter: RateLimiter,
}

impl GeminiClient {
    pub fn new(api_key: String, model: String, daily_limit: u64, rpm: u32) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client");

        let rate_limiter = RateLimiter::new(
            RateLimiterConfig {
                rpm,
                ..Default::default()
            },
            daily_limit,
        );

        Self {
            api_key,
            model,
            client,
            rate_limiter,
        }
    }

    fn api_url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        )
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
        loop {
            self.rate_limiter.acquire().await?;

            let response = self
                .client
                .post(self.api_url())
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

                self.rate_limiter.report_rate_limit(retry_after).await;
                retries += 1;

                if retries > self.rate_limiter.max_retries() {
                    anyhow::bail!("Gemini API rate limited after {} retries", retries);
                }
                continue;
            }

            if !status.is_success() {
                let error_body = response.text().await.unwrap_or_default();
                anyhow::bail!("Gemini API error {}: {}", status.as_u16(), error_body);
            }

            self.rate_limiter.report_success().await;

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
}
