use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

/// Maximum response body size to return (characters).
const MAX_BODY_SIZE: usize = 32000;

pub struct WebFetchTool {
    client: reqwest::Client,
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetchTool {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("deepagent/0.1")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("failed to build HTTP client");

        Self { client }
    }
}

#[async_trait]
impl super::Tool for WebFetchTool {
    fn name(&self) -> &str {
        "webfetch"
    }

    fn description(&self) -> &str {
        "Fetch the content of a URL. Returns the response body as text. Useful for reading documentation, APIs, or web pages."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers as key-value pairs (e.g. {\"Authorization\": \"Bearer token\"})"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .context("missing 'url' parameter")?;

        // Basic URL validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            anyhow::bail!("URL must start with http:// or https://");
        }

        let mut request = self.client.get(url);

        // Add custom headers if provided
        if let Some(headers) = args.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(val) = value.as_str() {
                    request = request.header(key.as_str(), val);
                }
            }
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("failed to fetch: {}", url))?;

        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();

        let body = response
            .text()
            .await
            .with_context(|| format!("failed to read response body from: {}", url))?;

        let mut result = format!(
            "HTTP {} | Content-Type: {}\n\n",
            status.as_u16(),
            content_type
        );

        if body.len() > MAX_BODY_SIZE {
            result.push_str(&body[..MAX_BODY_SIZE]);
            result.push_str(&format!("\n\n... (truncated, {} total chars)", body.len()));
        } else {
            result.push_str(&body);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[test]
    fn test_webfetch_schema() {
        let tool = WebFetchTool::new();
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["url"].is_object());
    }

    #[tokio::test]
    async fn test_webfetch_invalid_url() {
        let tool = WebFetchTool::new();
        let result = tool.execute(serde_json::json!({"url": "not-a-url"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_webfetch_missing_url() {
        let tool = WebFetchTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }
}
