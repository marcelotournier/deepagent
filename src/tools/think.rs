use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// A tool that lets the model "think" by writing out its reasoning.
/// This is a no-op tool — it just returns the thought back.
/// Useful for complex multi-step reasoning where the model needs
/// to plan before acting.
pub struct ThinkTool;

#[async_trait]
impl super::Tool for ThinkTool {
    fn name(&self) -> &str {
        "think"
    }

    fn description(&self) -> &str {
        "Use this tool to think through a problem step by step before acting. Write your reasoning and plan. The thought is recorded but no action is taken."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "thought": {
                    "type": "string",
                    "description": "Your step-by-step reasoning about the task"
                }
            },
            "required": ["thought"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let thought = args
            .get("thought")
            .and_then(|v| v.as_str())
            .unwrap_or("(empty thought)");

        Ok(format!("Thought recorded: {}", thought))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[tokio::test]
    async fn test_think_returns_thought() {
        let tool = ThinkTool;
        let result = tool
            .execute(serde_json::json!({"thought": "I need to read the file first"}))
            .await
            .unwrap();
        assert!(result.contains("I need to read the file first"));
    }

    #[tokio::test]
    async fn test_think_empty() {
        let tool = ThinkTool;
        let result = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.contains("empty thought"));
    }
}
