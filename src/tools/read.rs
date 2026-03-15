use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

pub struct ReadTool {
    max_chars: usize,
}

impl ReadTool {
    pub fn new(max_chars: usize) -> Self {
        Self { max_chars }
    }
}

#[async_trait]
impl super::Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a file's contents with line numbers. Supports optional start_line and end_line range."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to read"
                },
                "start_line": {
                    "type": "integer",
                    "description": "First line to read (1-based, default: 1)"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Last line to read (inclusive, default: end of file)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("missing 'path' parameter")?;

        let path = Path::new(path_str);
        if !path.exists() {
            anyhow::bail!("file not found: {}", path_str);
        }

        // Check if binary
        let content_bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read: {}", path_str))?;

        if is_binary(&content_bytes) {
            return Ok(format!("[binary file: {} bytes]", content_bytes.len()));
        }

        let content = String::from_utf8_lossy(&content_bytes);
        let lines: Vec<&str> = content.lines().collect();

        let start = args
            .get("start_line")
            .and_then(|v| v.as_u64())
            .map(|v| v.max(1) as usize)
            .unwrap_or(1);

        let end = args
            .get("end_line")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(lines.len());

        let start_idx = (start - 1).min(lines.len());
        let end_idx = end.min(lines.len());

        let mut result = String::new();
        let mut total_chars = 0;

        for (i, line) in lines[start_idx..end_idx].iter().enumerate() {
            let line_num = start_idx + i + 1;
            let formatted = format!("{}\t{}\n", line_num, line);
            total_chars += formatted.len();
            if total_chars > self.max_chars {
                result.push_str(&format!(
                    "... (truncated at {} chars, file has {} lines)\n",
                    self.max_chars,
                    lines.len()
                ));
                break;
            }
            result.push_str(&formatted);
        }

        if result.is_empty() {
            result = "(empty file)".to_string();
        }

        Ok(result)
    }
}

fn is_binary(bytes: &[u8]) -> bool {
    let check_len = bytes.len().min(8192);
    bytes[..check_len].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[tokio::test]
    async fn test_read_file() {
        let tool = ReadTool::new(32000);
        let result = tool
            .execute(serde_json::json!({"path": "Cargo.toml"}))
            .await
            .unwrap();
        assert!(result.contains("deepagent"));
        // Check line numbers
        assert!(result.starts_with("1\t"));
    }

    #[tokio::test]
    async fn test_read_range() {
        let tool = ReadTool::new(32000);
        let result = tool
            .execute(serde_json::json!({"path": "Cargo.toml", "start_line": 1, "end_line": 3}))
            .await
            .unwrap();
        let lines: Vec<&str> = result.trim().lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[tokio::test]
    async fn test_read_not_found() {
        let tool = ReadTool::new(32000);
        let result = tool
            .execute(serde_json::json!({"path": "nonexistent.txt"}))
            .await;
        assert!(result.is_err());
    }
}
