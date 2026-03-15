use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use tokio::process::Command;

pub struct BashTool {
    working_dir: PathBuf,
    timeout_secs: u64,
    max_output: usize,
}

impl BashTool {
    pub fn new(working_dir: PathBuf, timeout_secs: u64, max_output: usize) -> Self {
        Self {
            working_dir,
            timeout_secs,
            max_output,
        }
    }
}

#[async_trait]
impl super::Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output (stdout + stderr)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 120)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .context("missing 'command' parameter")?;

        let timeout = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.timeout_secs);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&self.working_dir)
                .output(),
        )
        .await
        .context("command timed out")?
        .context("failed to execute command")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("STDERR:\n");
            result.push_str(&stderr);
        }

        if result.is_empty() {
            result.push_str("(no output)");
        }

        // Add exit code info if non-zero
        if !output.status.success() {
            result.push_str(&format!(
                "\n[exit code: {}]",
                output.status.code().unwrap_or(-1)
            ));
        }

        // Truncate if too long
        if result.len() > self.max_output {
            result.truncate(self.max_output);
            result.push_str("\n... (truncated)");
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[tokio::test]
    async fn test_bash_echo() {
        let tool = BashTool::new(std::env::current_dir().unwrap(), 10, 8192);
        let result = tool
            .execute(serde_json::json!({"command": "echo hello"}))
            .await
            .unwrap();
        assert_eq!(result.trim(), "hello");
    }

    #[tokio::test]
    async fn test_bash_exit_code() {
        let tool = BashTool::new(std::env::current_dir().unwrap(), 10, 8192);
        let result = tool
            .execute(serde_json::json!({"command": "exit 1"}))
            .await
            .unwrap();
        assert!(result.contains("[exit code: 1]"));
    }

    #[tokio::test]
    async fn test_bash_stderr() {
        let tool = BashTool::new(std::env::current_dir().unwrap(), 10, 8192);
        let result = tool
            .execute(serde_json::json!({"command": "echo err >&2"}))
            .await
            .unwrap();
        assert!(result.contains("STDERR:"));
        assert!(result.contains("err"));
    }

    #[tokio::test]
    async fn test_bash_timeout() {
        let tool = BashTool::new(std::env::current_dir().unwrap(), 1, 8192);
        let result = tool
            .execute(serde_json::json!({"command": "sleep 10"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_bash_truncation() {
        let tool = BashTool::new(std::env::current_dir().unwrap(), 10, 50);
        let result = tool
            .execute(serde_json::json!({"command": "python3 -c \"print('x' * 200)\""}))
            .await
            .unwrap();
        assert!(result.contains("(truncated)"));
    }
}
