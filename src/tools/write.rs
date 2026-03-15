use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

pub struct WriteTool;

#[async_trait]
impl super::Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if needed. Uses atomic write via tempfile."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("missing 'path' parameter")?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .context("missing 'content' parameter")?;

        let path = Path::new(path_str);

        // Create parent directories
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("failed to create directory: {}", parent.display()))?;
            }
        }

        // Atomic write: write to tempfile then rename
        let dir = path.parent().unwrap_or(Path::new("."));
        let temp = tempfile::NamedTempFile::new_in(dir).context("failed to create temp file")?;

        tokio::fs::write(temp.path(), content.as_bytes())
            .await
            .context("failed to write temp file")?;

        temp.persist(path)
            .with_context(|| format!("failed to persist to: {}", path_str))?;

        let bytes = content.len();
        Ok(format!("Wrote {} bytes to {}", bytes, path_str))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[tokio::test]
    async fn test_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let tool = WriteTool;

        let result = tool
            .execute(serde_json::json!({
                "path": path.to_str().unwrap(),
                "content": "hello world"
            }))
            .await
            .unwrap();

        assert!(result.contains("11 bytes"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_write_creates_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/c/test.txt");
        let tool = WriteTool;

        let result = tool
            .execute(serde_json::json!({
                "path": path.to_str().unwrap(),
                "content": "nested"
            }))
            .await;

        assert!(result.is_ok());
        assert!(path.exists());
    }
}
