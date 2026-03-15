pub mod bash;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod ls;
pub mod patch;
pub mod read;
pub mod think;
pub mod todo;
pub mod webfetch;
pub mod write;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

/// Trait that all tools must implement.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name as used in function calls.
    fn name(&self) -> &str;

    /// One-line description for the LLM.
    fn description(&self) -> &str;

    /// JSON Schema for the tool's parameters.
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with the given arguments, returning a string result.
    async fn execute(&self, args: Value) -> Result<String>;
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Create a registry with all built-in tools.
    pub fn with_defaults(working_dir: std::path::PathBuf) -> Self {
        Self::with_config(working_dir, 120, 8192)
    }

    /// Create a registry with configurable timeouts.
    pub fn with_config(
        working_dir: std::path::PathBuf,
        timeout_secs: u64,
        max_output: usize,
    ) -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(bash::BashTool::new(
            working_dir.clone(),
            timeout_secs,
            max_output,
        )));
        registry.register(Box::new(read::ReadTool::new(32000)));
        registry.register(Box::new(write::WriteTool));
        registry.register(Box::new(edit::EditTool));
        registry.register(Box::new(grep::GrepTool::new(100)));
        registry.register(Box::new(glob::GlobTool::new(200)));
        registry.register(Box::new(ls::LsTool::new(2)));
        registry.register(Box::new(patch::PatchTool));
        registry.register(Box::new(webfetch::WebFetchTool::new()));

        registry.register(Box::new(think::ThinkTool));

        let todo_list = todo::new_todo_list();
        registry.register(Box::new(todo::TodoWriteTool::new(todo_list.clone())));
        registry.register(Box::new(todo::TodoReadTool::new(todo_list)));

        registry
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Return tool schemas for the system prompt.
    pub fn schemas(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters_schema(),
                })
            })
            .collect()
    }

    /// Return tool declarations in Gemini function declaration format.
    pub fn gemini_function_declarations(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters_schema(),
                })
            })
            .collect()
    }

    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
