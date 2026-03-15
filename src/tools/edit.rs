use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use similar::{ChangeTag, TextDiff};

pub struct EditTool;

#[async_trait]
impl super::Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Replace exact text in a file. old_str must match exactly once (unless replace_all is true). Shows diff after edit."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to edit"
                },
                "old_str": {
                    "type": "string",
                    "description": "Exact text to find and replace"
                },
                "new_str": {
                    "type": "string",
                    "description": "Replacement text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: false)"
                }
            },
            "required": ["path", "old_str", "new_str"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("missing 'path' parameter")?;

        let old_str = args
            .get("old_str")
            .and_then(|v| v.as_str())
            .context("missing 'old_str' parameter")?;

        let new_str = args
            .get("new_str")
            .and_then(|v| v.as_str())
            .context("missing 'new_str' parameter")?;

        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let content = tokio::fs::read_to_string(path_str)
            .await
            .with_context(|| format!("failed to read: {}", path_str))?;

        let count = content.matches(old_str).count();

        if count == 0 {
            anyhow::bail!("old_str not found in {}", path_str);
        }

        if count > 1 && !replace_all {
            anyhow::bail!(
                "old_str found {} times in {} (use replace_all to replace all occurrences)",
                count,
                path_str
            );
        }

        let new_content = if replace_all {
            content.replace(old_str, new_str)
        } else {
            content.replacen(old_str, new_str, 1)
        };

        // Atomic write
        let path = std::path::Path::new(path_str);
        let dir = path.parent().unwrap_or(std::path::Path::new("."));
        let temp = tempfile::NamedTempFile::new_in(dir)?;
        tokio::fs::write(temp.path(), new_content.as_bytes()).await?;
        temp.persist(path)?;

        // Generate diff
        let diff = TextDiff::from_lines(&content, &new_content);
        let mut diff_output = String::new();
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => continue,
            };
            diff_output.push_str(&format!("{}{}", sign, change));
        }

        let replaced_msg = if replace_all && count > 1 {
            format!("Replaced {} occurrences in {}", count, path_str)
        } else {
            format!("Replaced 1 occurrence in {}", path_str)
        };

        Ok(format!("{}\n\nDiff:\n{}", replaced_msg, diff_output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[tokio::test]
    async fn test_edit_single() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "hello world").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_str": "hello",
                "new_str": "goodbye"
            }))
            .await
            .unwrap();

        assert!(result.contains("Replaced 1"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "goodbye world");
    }

    #[tokio::test]
    async fn test_edit_multiple_without_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "aaa bbb aaa").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_str": "aaa",
                "new_str": "ccc"
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_edit_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "aaa bbb aaa").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_str": "aaa",
                "new_str": "ccc",
                "replace_all": true
            }))
            .await
            .unwrap();

        assert!(result.contains("Replaced 2"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "ccc bbb ccc");
    }

    #[tokio::test]
    async fn test_edit_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "hello").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_str": "xyz",
                "new_str": "abc"
            }))
            .await;

        assert!(result.is_err());
    }
}
