use crate::api::{FunctionCall, FunctionResponse, LlmClient, Message, MessagePart, ResponsePart};
use crate::tools::ToolRegistry;
use anyhow::Result;

/// The ReAct agent that orchestrates think → act → observe loops.
pub struct Agent {
    client: Box<dyn LlmClient>,
    tools: ToolRegistry,
    max_turns: usize,
    system_prompt: String,
}

impl Agent {
    pub fn new(
        client: Box<dyn LlmClient>,
        tools: ToolRegistry,
        max_turns: usize,
        system_prompt: String,
    ) -> Self {
        Self {
            client,
            tools,
            max_turns,
            system_prompt,
        }
    }

    /// Build the default system prompt with tool schemas and environment info.
    pub fn build_system_prompt(tools: &ToolRegistry, working_dir: &str, os_info: &str) -> String {
        let tool_schemas = serde_json::to_string_pretty(&tools.schemas()).unwrap_or_default();

        format!(
            r#"You are a coding agent. You have access to these tools:
{tool_schemas}

Working directory: {working_dir}
OS: {os_info}

Rules:
- Use grep/glob to find files before reading them
- Read files before editing them
- Run tests after making changes
- Be concise in explanations
- Batch independent operations
- When your task is complete, provide a clear summary of what you did

Respond with either:
1. A text response (if done)
2. A function_call to use a tool"#
        )
    }

    /// Run the agent loop with the given user prompt. Returns the final text output.
    pub async fn run(&self, prompt: &str) -> Result<String> {
        let mut messages = vec![Message {
            role: "user".to_string(),
            parts: vec![MessagePart::Text {
                text: prompt.to_string(),
            }],
        }];

        let tool_declarations = self.tools.gemini_function_declarations();
        let mut final_output = String::new();

        for turn in 0..self.max_turns {
            tracing::info!("Agent turn {}/{}", turn + 1, self.max_turns);

            let response = self
                .client
                .generate(&self.system_prompt, &messages, &tool_declarations)
                .await?;

            let mut has_function_call = false;
            let mut model_parts: Vec<MessagePart> = Vec::new();
            let mut function_responses: Vec<MessagePart> = Vec::new();

            for part in &response {
                match part {
                    ResponsePart::Text(text) => {
                        tracing::info!("Model text: {}", &text[..text.len().min(100)]);
                        final_output = text.clone();
                        model_parts.push(MessagePart::Text { text: text.clone() });
                    }
                    ResponsePart::FunctionCall(fc) => {
                        has_function_call = true;
                        tracing::info!("Tool call: {}({})", fc.name, fc.args);

                        model_parts.push(MessagePart::FunctionCall {
                            function_call: fc.clone(),
                        });

                        // Execute the tool
                        let result = self.execute_tool(fc).await;
                        let result_text = match &result {
                            Ok(output) => output.clone(),
                            Err(e) => format!("Error: {}", e),
                        };

                        tracing::info!(
                            "Tool result ({}): {}",
                            fc.name,
                            &result_text[..result_text.len().min(200)]
                        );

                        function_responses.push(MessagePart::FunctionResponse {
                            function_response: FunctionResponse {
                                name: fc.name.clone(),
                                response: serde_json::json!({"result": result_text}),
                            },
                        });
                    }
                }
            }

            // Add model response to conversation
            messages.push(Message {
                role: "model".to_string(),
                parts: model_parts,
            });

            if !has_function_call {
                // Model is done — return text
                break;
            }

            // Add function responses
            messages.push(Message {
                role: "user".to_string(),
                parts: function_responses,
            });
        }

        if final_output.is_empty() {
            final_output = "(agent completed without text output)".to_string();
        }

        Ok(final_output)
    }

    async fn execute_tool(&self, fc: &FunctionCall) -> Result<String> {
        match self.tools.get(&fc.name) {
            Some(tool) => tool.execute(fc.args.clone()).await,
            None => anyhow::bail!("unknown tool: {}", fc.name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{LlmClient, Message, ResponsePart};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct MockClient {
        responses: Vec<Vec<ResponsePart>>,
        call_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl LlmClient for MockClient {
        async fn generate(
            &self,
            _system_prompt: &str,
            _messages: &[Message],
            _tools: &[serde_json::Value],
        ) -> Result<Vec<ResponsePart>> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            if idx < self.responses.len() {
                Ok(self.responses[idx].clone())
            } else {
                Ok(vec![ResponsePart::Text("done".to_string())])
            }
        }
    }

    #[tokio::test]
    async fn test_agent_text_only() {
        let client = MockClient {
            responses: vec![vec![ResponsePart::Text("Hello!".to_string())]],
            call_count: Arc::new(AtomicUsize::new(0)),
        };

        let tools = ToolRegistry::new();
        let agent = Agent::new(Box::new(client), tools, 25, "system".to_string());

        let result = agent.run("say hello").await.unwrap();
        assert_eq!(result, "Hello!");
    }

    #[tokio::test]
    async fn test_agent_tool_then_text() {
        let client = MockClient {
            responses: vec![
                vec![ResponsePart::FunctionCall(FunctionCall {
                    name: "bash".to_string(),
                    args: serde_json::json!({"command": "echo hi"}),
                })],
                vec![ResponsePart::Text("Done! The output was 'hi'.".to_string())],
            ],
            call_count: Arc::new(AtomicUsize::new(0)),
        };

        let tools = ToolRegistry::with_defaults(std::env::current_dir().unwrap());
        let agent = Agent::new(Box::new(client), tools, 25, "system".to_string());

        let result = agent.run("run echo hi").await.unwrap();
        assert!(result.contains("Done!"));
    }
}
