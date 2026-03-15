use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::process::Command;

/// Shell execution tool with persistent working directory across calls.
/// If the user runs `cd /tmp`, subsequent commands execute in `/tmp`.
pub struct BashTool {
    working_dir: Arc<Mutex<PathBuf>>,
    timeout_secs: u64,
    max_output: usize,
}

impl BashTool {
    pub fn new(working_dir: PathBuf, timeout_secs: u64, max_output: usize) -> Self {
        Self {
            working_dir: Arc::new(Mutex::new(working_dir)),
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
        "Execute a shell command and return its output (stdout + stderr). Working directory persists between calls."
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

        let cwd = self.working_dir.lock().unwrap().clone();

        // Wrap command to capture the final working directory after execution.
        // This allows `cd` commands to persist across calls.
        // The __DEEPAGENT_PWD__ marker lets us extract the new cwd.
        let wrapped = format!(
            "{} ; __deepagent_exit=$?; echo; echo \"__DEEPAGENT_PWD__$(pwd)\"; exit $__deepagent_exit",
            command
        );

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            Command::new("sh")
                .arg("-c")
                .arg(&wrapped)
                .current_dir(&cwd)
                .output(),
        )
        .await
        .context("command timed out")?
        .context("failed to execute command")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Extract the new working directory from stdout
        let (user_stdout, new_cwd) = extract_cwd(&stdout);

        // Update persistent working directory if we got a valid path
        if let Some(new_dir) = new_cwd {
            let path = PathBuf::from(&new_dir);
            if path.is_dir() {
                *self.working_dir.lock().unwrap() = path;
            }
        }

        let mut result = String::new();
        if !user_stdout.is_empty() {
            result.push_str(&user_stdout);
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

/// Extract the user's stdout and the new working directory from the wrapped output.
fn extract_cwd(stdout: &str) -> (String, Option<String>) {
    const MARKER: &str = "__DEEPAGENT_PWD__";

    if let Some(marker_pos) = stdout.rfind(MARKER) {
        let user_output = stdout[..marker_pos].trim_end().to_string();
        let cwd_line = stdout[marker_pos + MARKER.len()..].trim().to_string();
        let cwd = if cwd_line.is_empty() {
            None
        } else {
            Some(cwd_line)
        };
        (user_output, cwd)
    } else {
        (stdout.to_string(), None)
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
        assert!(result.contains("hello"));
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

    #[tokio::test]
    async fn test_bash_persistent_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool::new(dir.path().to_path_buf(), 10, 8192);

        // Create a subdirectory and cd into it
        tool.execute(serde_json::json!({"command": "mkdir -p subdir"}))
            .await
            .unwrap();

        tool.execute(serde_json::json!({"command": "cd subdir"}))
            .await
            .unwrap();

        // Next command should run in subdir
        let result = tool
            .execute(serde_json::json!({"command": "pwd"}))
            .await
            .unwrap();

        assert!(
            result.contains("subdir"),
            "pwd should be in subdir: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_extract_cwd() {
        let (output, cwd) = extract_cwd("hello world\n\n__DEEPAGENT_PWD__/tmp/test\n");
        assert_eq!(output, "hello world");
        assert_eq!(cwd, Some("/tmp/test".to_string()));
    }

    #[tokio::test]
    async fn test_extract_cwd_no_marker() {
        let (output, cwd) = extract_cwd("just output\n");
        assert_eq!(output, "just output\n");
        assert!(cwd.is_none());
    }
}
