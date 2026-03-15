use crate::api::{FunctionCall, FunctionResponse, LlmClient, Message, MessagePart, ResponsePart};
use crate::tools::ToolRegistry;
use anyhow::Result;

/// Maximum characters per tool result before truncation.
const MAX_TOOL_OUTPUT: usize = 16384;

/// Approximate context window sizes for Gemini models (in tokens).
/// We use chars/4 as a rough token estimate.
const CONTEXT_WINDOW_TOKENS: usize = 1_000_000; // Gemini Flash has 1M context
const COMPACTION_THRESHOLD: f64 = 0.80; // Trigger at 80% usage

/// Progress events emitted during agent execution.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A new turn is starting.
    TurnStart { turn: usize, max_turns: usize },
    /// The model is calling a tool.
    ToolCall { name: String, args: String },
    /// A tool has returned a result.
    ToolResult { name: String, output: String },
    /// The model produced text output.
    ModelText { text: String },
    /// Token usage for the current turn.
    TokenUsage {
        prompt_tokens: usize,
        candidates_tokens: usize,
        total_tokens: usize,
    },
}

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

    /// Build the default system prompt with tool schemas, environment info,
    /// and optional project instructions from DEEPAGENT.md.
    pub fn build_system_prompt(tools: &ToolRegistry, working_dir: &str, os_info: &str) -> String {
        let tool_schemas = serde_json::to_string_pretty(&tools.schemas()).unwrap_or_default();

        // Read project-specific instructions if available
        let project_instructions = Self::read_project_config(working_dir);

        format!(
            r#"You are an expert coding agent. You solve programming tasks by reading, writing, and executing code.

## Environment
- Working directory: {working_dir}
- OS: {os_info}

## Available Tools
{tool_schemas}

## Rules
1. **Explore before acting**: Use grep/glob to find relevant files before reading them. Read files before editing.
2. **Make targeted changes**: Edit only what's necessary. Don't refactor code you weren't asked to change.
3. **Verify your work**: After making changes, run tests or cargo check to verify correctness.
4. **Use the think tool** for complex tasks: Plan your approach before executing multiple steps.
5. **Use todowrite/todoread** to track progress on multi-step tasks.
6. **Be concise**: Provide clear, brief explanations. Focus on what you did and why.
7. **Handle errors**: If a tool call fails, read the error, adjust your approach, and retry.
8. **Batch operations**: If you need to read multiple files, do them in sequence efficiently.
9. **Security**: Never execute destructive commands without being explicitly asked. Never expose secrets.
10. **Complete the task**: Keep working until the task is fully done, then provide a summary.

## Response Format
- To use a tool: respond with a function_call
- When done: respond with a text summary of what you accomplished
{project_instructions}"#
        )
    }

    /// Read project-specific configuration from DEEPAGENT.md or similar files.
    /// Returns formatted section for the system prompt, or empty string if not found.
    fn read_project_config(working_dir: &str) -> String {
        let config_files = ["DEEPAGENT.md", ".deepagent.md", "CLAUDE.md"];

        for filename in &config_files {
            let path = std::path::Path::new(working_dir).join(filename);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if !content.is_empty() {
                    // Truncate to 8KB to avoid blowing up the context
                    let truncated = if content.len() > 8192 {
                        format!("{}...\n(truncated)", &content[..8192])
                    } else {
                        content
                    };

                    tracing::info!("Loaded project config from {}", filename);
                    return format!(
                        "\n## Project Instructions (from {})\n{}",
                        filename, truncated
                    );
                }
            }
        }

        String::new()
    }

    /// Run the agent loop with the given user prompt. Returns the final text output.
    pub async fn run(&self, prompt: &str) -> Result<String> {
        self.run_with_progress(prompt, |_| {}).await
    }

    /// Run the agent loop with a progress callback for streaming output.
    pub async fn run_with_progress(
        &self,
        prompt: &str,
        mut on_event: impl FnMut(AgentEvent),
    ) -> Result<String> {
        let mut messages = vec![Message {
            role: "user".to_string(),
            parts: vec![MessagePart::Text {
                text: prompt.to_string(),
            }],
        }];

        let tool_declarations = self.tools.gemini_function_declarations();
        let mut final_output = String::new();

        for turn in 0..self.max_turns {
            on_event(AgentEvent::TurnStart {
                turn: turn + 1,
                max_turns: self.max_turns,
            });
            tracing::info!("Agent turn {}/{}", turn + 1, self.max_turns);

            let response = self
                .client
                .generate(&self.system_prompt, &messages, &tool_declarations)
                .await?;

            // Emit token usage from this turn
            let usage = self.client.last_usage();
            if usage.total_tokens > 0 {
                on_event(AgentEvent::TokenUsage {
                    prompt_tokens: usage.prompt_tokens,
                    candidates_tokens: usage.candidates_tokens,
                    total_tokens: usage.total_tokens,
                });
            }

            let mut has_function_call = false;
            let mut model_parts: Vec<MessagePart> = Vec::new();
            let mut function_responses: Vec<MessagePart> = Vec::new();

            for part in &response {
                match part {
                    ResponsePart::Text(text) => {
                        tracing::info!("Model text: {}", &text[..text.len().min(100)]);
                        final_output = text.clone();
                        model_parts.push(MessagePart::Text { text: text.clone() });
                        on_event(AgentEvent::ModelText { text: text.clone() });
                    }
                    ResponsePart::FunctionCall(fc) => {
                        has_function_call = true;
                        let args_str = fc.args.to_string();
                        tracing::info!("Tool call: {}({})", fc.name, args_str);
                        on_event(AgentEvent::ToolCall {
                            name: fc.name.clone(),
                            args: args_str,
                        });

                        model_parts.push(MessagePart::FunctionCall {
                            function_call: fc.clone(),
                        });

                        // Execute the tool
                        let result = self.execute_tool(fc).await;
                        let result_text = match &result {
                            Ok(output) => truncate_tool_output(output),
                            Err(e) => format!("Error: {}", e),
                        };

                        tracing::info!(
                            "Tool result ({}): {}",
                            fc.name,
                            &result_text[..result_text.len().min(200)]
                        );
                        on_event(AgentEvent::ToolResult {
                            name: fc.name.clone(),
                            output: result_text[..result_text.len().min(500)].to_string(),
                        });

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
                self.client.hint_prefer_primary();
                break;
            }

            // Smart model routing: if only simple tool calls (read-only tools),
            // hint to use the lite model for the next turn to save tokens.
            let has_text = response.iter().any(|p| matches!(p, ResponsePart::Text(_)));
            let only_readonly_tools = response.iter().all(|p| match p {
                ResponsePart::FunctionCall(fc) => is_readonly_tool(&fc.name),
                ResponsePart::Text(_) => true,
            });

            if !has_text && only_readonly_tools {
                self.client.hint_prefer_lite();
            } else {
                self.client.hint_prefer_primary();
            }

            // Coalesce: all function responses go in a single message
            // This is critical for free-tier — one request instead of N
            messages.push(Message {
                role: "user".to_string(),
                parts: function_responses,
            });

            // Check if context is approaching capacity and compact if needed
            let estimated_tokens = estimate_tokens(&messages);
            let threshold = (CONTEXT_WINDOW_TOKENS as f64 * COMPACTION_THRESHOLD) as usize;
            if estimated_tokens > threshold {
                tracing::info!(
                    "Context at ~{} tokens ({}% of {}), compacting",
                    estimated_tokens,
                    estimated_tokens * 100 / CONTEXT_WINDOW_TOKENS,
                    CONTEXT_WINDOW_TOKENS
                );
                compact_context(&mut messages);
            } else if messages.len() > 20 {
                // Lighter compression for shorter conversations
                compress_history(&mut messages);
            }
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

/// Truncate tool output to save tokens. Keeps head and tail for context.
fn truncate_tool_output(output: &str) -> String {
    if output.len() <= MAX_TOOL_OUTPUT {
        return output.to_string();
    }

    let head_size = MAX_TOOL_OUTPUT * 3 / 4; // 75% from start
    let tail_size = MAX_TOOL_OUTPUT / 4; // 25% from end

    let head = &output[..head_size];
    let tail = &output[output.len() - tail_size..];
    let omitted = output.len() - head_size - tail_size;

    format!(
        "{}\n\n... ({} chars omitted) ...\n\n{}",
        head, omitted, tail
    )
}

/// Compress old conversation history to reduce token usage.
/// Keeps the first message (user prompt) and last 10 messages intact.
/// Middle messages have their tool results truncated aggressively.
fn compress_history(messages: &mut [Message]) {
    if messages.len() <= 12 {
        return;
    }

    let keep_tail = 10;
    let compress_end = messages.len() - keep_tail;

    // Skip first message (user prompt), compress middle
    for msg in messages[1..compress_end].iter_mut() {
        for part in &mut msg.parts {
            if let MessagePart::FunctionResponse { function_response } = part {
                if let Some(result) = function_response.response.get("result") {
                    if let Some(text) = result.as_str() {
                        if text.len() > 500 {
                            let truncated = format!("{}... (compressed)", &text[..200]);
                            function_response.response = serde_json::json!({"result": truncated});
                        }
                    }
                }
            }
        }
    }
}

/// Tools that only read state and don't modify files.
/// These can safely use a cheaper/faster model.
fn is_readonly_tool(name: &str) -> bool {
    matches!(name, "read" | "grep" | "glob" | "ls" | "think" | "todoread")
}

/// Estimate total token count across all messages.
/// Uses chars/4 as a rough approximation (works well for English/code).
fn estimate_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|msg| {
            msg.parts
                .iter()
                .map(|part| match part {
                    MessagePart::Text { text } => text.len() / 4,
                    MessagePart::FunctionCall { function_call } => {
                        (function_call.name.len() + function_call.args.to_string().len()) / 4
                    }
                    MessagePart::FunctionResponse { function_response } => {
                        (function_response.name.len()
                            + function_response.response.to_string().len())
                            / 4
                    }
                })
                .sum::<usize>()
        })
        .sum()
}

/// Aggressive context compaction for when we're near the context window limit.
/// Keeps the first message (user prompt) and last 6 messages.
/// Middle messages are summarized: tool results replaced with brief summaries,
/// text responses truncated to first 200 chars.
fn compact_context(messages: &mut [Message]) {
    if messages.len() <= 8 {
        return;
    }

    let keep_tail = 6;
    let compress_end = messages.len() - keep_tail;

    // Aggressively compress all middle messages
    for msg in messages[1..compress_end].iter_mut() {
        for part in &mut msg.parts {
            match part {
                MessagePart::FunctionResponse { function_response } => {
                    if let Some(result) = function_response.response.get("result") {
                        if let Some(text) = result.as_str() {
                            if text.len() > 100 {
                                let summary = format!(
                                    "[{}] {} chars of output (compacted)",
                                    function_response.name,
                                    text.len()
                                );
                                function_response.response = serde_json::json!({"result": summary});
                            }
                        }
                    }
                }
                MessagePart::Text { text } => {
                    if text.len() > 200 {
                        *text = format!("{}... (compacted)", &text[..200]);
                    }
                }
                MessagePart::FunctionCall { .. } => {
                    // Keep function calls as-is — they're small
                }
            }
        }
    }

    tracing::info!(
        "Compacted {} messages, kept first + last {}",
        compress_end - 1,
        keep_tail
    );
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

    #[tokio::test]
    async fn test_agent_max_turns() {
        // Agent that always calls a tool — should stop at max_turns
        let call_count = Arc::new(AtomicUsize::new(0));
        let client = MockClient {
            responses: vec![], // will always fall through to "done"
            call_count: call_count.clone(),
        };

        let tools = ToolRegistry::new();
        let agent = Agent::new(Box::new(client), tools, 3, "system".to_string());

        let result = agent.run("keep going").await.unwrap();
        // Should have called generate at most 3 times
        assert!(call_count.load(Ordering::SeqCst) <= 3);
        assert_eq!(result, "done");
    }

    #[tokio::test]
    async fn test_agent_multiple_tool_calls() {
        // Model returns two tool calls in one response
        let client = MockClient {
            responses: vec![
                vec![
                    ResponsePart::FunctionCall(FunctionCall {
                        name: "bash".to_string(),
                        args: serde_json::json!({"command": "echo one"}),
                    }),
                    ResponsePart::FunctionCall(FunctionCall {
                        name: "bash".to_string(),
                        args: serde_json::json!({"command": "echo two"}),
                    }),
                ],
                vec![ResponsePart::Text(
                    "Both commands executed successfully.".to_string(),
                )],
            ],
            call_count: Arc::new(AtomicUsize::new(0)),
        };

        let tools = ToolRegistry::with_defaults(std::env::current_dir().unwrap());
        let agent = Agent::new(Box::new(client), tools, 25, "system".to_string());

        let result = agent.run("run two commands").await.unwrap();
        assert!(result.contains("Both commands"));
    }

    #[test]
    fn test_truncate_tool_output_short() {
        let output = "short output";
        assert_eq!(truncate_tool_output(output), output);
    }

    #[test]
    fn test_truncate_tool_output_long() {
        let output = "x".repeat(MAX_TOOL_OUTPUT + 1000);
        let truncated = truncate_tool_output(&output);
        assert!(truncated.len() < output.len());
        assert!(truncated.contains("chars omitted"));
    }

    #[test]
    fn test_compress_history() {
        let mut messages: Vec<Message> = (0..25)
            .map(|i| Message {
                role: if i % 2 == 0 {
                    "user".to_string()
                } else {
                    "model".to_string()
                },
                parts: vec![MessagePart::FunctionResponse {
                    function_response: FunctionResponse {
                        name: "bash".to_string(),
                        response: serde_json::json!({"result": "x".repeat(1000)}),
                    },
                }],
            })
            .collect();

        compress_history(&mut messages);

        // Middle messages should have compressed results
        let mid = &messages[5];
        if let MessagePart::FunctionResponse { function_response } = &mid.parts[0] {
            let result = function_response.response["result"].as_str().unwrap();
            assert!(result.len() < 500);
            assert!(result.contains("compressed"));
        }

        // Last messages should be untouched
        let last = &messages[24];
        if let MessagePart::FunctionResponse { function_response } = &last.parts[0] {
            let result = function_response.response["result"].as_str().unwrap();
            assert_eq!(result.len(), 1000);
        }
    }

    #[test]
    fn test_estimate_tokens() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                parts: vec![MessagePart::Text {
                    text: "x".repeat(400), // ~100 tokens
                }],
            },
            Message {
                role: "model".to_string(),
                parts: vec![MessagePart::Text {
                    text: "y".repeat(200), // ~50 tokens
                }],
            },
        ];

        let tokens = estimate_tokens(&messages);
        assert_eq!(tokens, 150); // 400/4 + 200/4
    }

    #[test]
    fn test_compact_context() {
        let mut messages: Vec<Message> = (0..15)
            .map(|i| Message {
                role: if i % 2 == 0 {
                    "user".to_string()
                } else {
                    "model".to_string()
                },
                parts: vec![MessagePart::FunctionResponse {
                    function_response: FunctionResponse {
                        name: "bash".to_string(),
                        response: serde_json::json!({"result": "x".repeat(5000)}),
                    },
                }],
            })
            .collect();

        compact_context(&mut messages);

        // Middle messages (1..9) should be heavily compacted
        let mid = &messages[3];
        if let MessagePart::FunctionResponse { function_response } = &mid.parts[0] {
            let result = function_response.response["result"].as_str().unwrap();
            assert!(result.contains("compacted"));
            assert!(result.len() < 200);
        }

        // Last 6 messages should be untouched
        let tail = &messages[14];
        if let MessagePart::FunctionResponse { function_response } = &tail.parts[0] {
            let result = function_response.response["result"].as_str().unwrap();
            assert_eq!(result.len(), 5000);
        }
    }
}
