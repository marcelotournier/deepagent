use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    ".mypy_cache",
];

pub struct LsTool {
    max_depth: usize,
}

impl LsTool {
    pub fn new(max_depth: usize) -> Self {
        Self { max_depth }
    }
}

#[async_trait]
impl super::Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        "List directory contents up to 2 levels deep. Shows file sizes and directory markers."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list (default: current directory)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path_str = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let path = Path::new(path_str);
        if !path.exists() {
            anyhow::bail!("directory not found: {}", path_str);
        }
        if !path.is_dir() {
            anyhow::bail!("not a directory: {}", path_str);
        }

        let mut output = String::new();
        list_dir(path, &mut output, 0, self.max_depth)?;

        if output.is_empty() {
            output = "(empty directory)".to_string();
        }

        Ok(output)
    }
}

fn list_dir(path: &Path, output: &mut String, depth: usize, max_depth: usize) -> Result<()> {
    if depth > max_depth {
        return Ok(());
    }

    let indent = "  ".repeat(depth);

    let mut entries: Vec<_> = std::fs::read_dir(path)
        .with_context(|| format!("failed to read directory: {}", path.display()))?
        .filter_map(|e| e.ok())
        .collect();

    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip ignored directories
        if IGNORED_DIRS.contains(&name_str.as_ref()) {
            continue;
        }

        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            output.push_str(&format!("{}{}/ \n", indent, name_str));
            list_dir(&entry.path(), output, depth + 1, max_depth)?;
        } else {
            let size = metadata.len();
            let size_str = format_size(size);
            output.push_str(&format!("{}{} ({})\n", indent, name_str, size_str));
        }
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[tokio::test]
    async fn test_ls_current_dir() {
        let tool = LsTool::new(2);
        let result = tool
            .execute(serde_json::json!({"path": "."}))
            .await
            .unwrap();
        assert!(result.contains("Cargo.toml"));
        assert!(result.contains("src/"));
    }

    #[tokio::test]
    async fn test_ls_not_found() {
        let tool = LsTool::new(2);
        let result = tool
            .execute(serde_json::json!({"path": "/nonexistent_dir_xyz"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ls_ignores_git() {
        let tool = LsTool::new(2);
        let result = tool
            .execute(serde_json::json!({"path": "."}))
            .await
            .unwrap();
        assert!(!result.contains(".git/"));
    }
}
