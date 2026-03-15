pub mod gemini;
pub mod rate_limiter;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A function call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub args: serde_json::Value,
}

/// A single part of a model response.
#[derive(Debug, Clone)]
pub enum ResponsePart {
    Text(String),
    FunctionCall(FunctionCall),
}

/// Result of a function call, to be sent back to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub name: String,
    pub response: serde_json::Value,
}

/// A message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub parts: Vec<MessagePart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessagePart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: FunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: FunctionResponse,
    },
}

/// Trait for LLM backends.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a conversation and get response parts.
    async fn generate(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[serde_json::Value],
    ) -> Result<Vec<ResponsePart>>;

    /// Hint: prefer a lighter model for the next call (simple tool dispatch).
    fn hint_prefer_lite(&self) {}

    /// Hint: prefer the primary model for the next call (reasoning needed).
    fn hint_prefer_primary(&self) {}
}
