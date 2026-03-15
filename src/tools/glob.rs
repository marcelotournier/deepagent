use anyhow::{Context, Result};
use async_trait::async_trait;
use rayon::prelude::*;
use serde_json::Value;
use std::path::PathBuf;
use std::time::SystemTime;

pub struct GlobTool {
    max_results: usize,
}

impl GlobTool {
    pub fn new(max_results: usize) -> Self {
        Self { max_results }
    }
}

#[async_trait]
impl super::Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern (e.g., '**/*.rs'). Returns paths sorted by modification time (newest first)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match (e.g., '**/*.rs', 'src/**/*.toml')"
                },
                "path": {
                    "type": "string",
                    "description": "Base directory for the glob (default: current directory)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .context("missing 'pattern' parameter")?;

        let base_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let full_pattern = if pattern.starts_with('/') || pattern.starts_with('.') {
            pattern.to_string()
        } else {
            format!("{}/{}", base_path, pattern)
        };

        let entries: Vec<PathBuf> = glob::glob(&full_pattern)
            .with_context(|| format!("invalid glob pattern: {}", pattern))?
            .filter_map(|r| r.ok())
            .collect();

        if entries.is_empty() {
            return Ok("No files matched.".to_string());
        }

        // Get modification times in parallel
        let mut with_times: Vec<(PathBuf, SystemTime)> = entries
            .par_iter()
            .filter_map(|p| {
                std::fs::metadata(p)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| (p.clone(), t))
            })
            .collect();

        // Sort by modification time, newest first
        with_times.sort_by(|a, b| b.1.cmp(&a.1));

        // Limit results
        let limited = &with_times[..with_times.len().min(self.max_results)];

        let mut output = String::new();
        for (path, _) in limited {
            output.push_str(&format!("{}\n", path.display()));
        }

        if with_times.len() > self.max_results {
            output.push_str(&format!(
                "\n... ({} more files not shown)\n",
                with_times.len() - self.max_results
            ));
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[tokio::test]
    async fn test_glob_finds_rs_files() {
        let tool = GlobTool::new(200);
        let result = tool
            .execute(serde_json::json!({"pattern": "**/*.rs"}))
            .await
            .unwrap();
        assert!(result.contains(".rs"));
    }

    #[tokio::test]
    async fn test_glob_no_match() {
        let tool = GlobTool::new(200);
        let result = tool
            .execute(serde_json::json!({"pattern": "**/*.zzzzz"}))
            .await
            .unwrap();
        assert!(result.contains("No files matched"));
    }

    #[tokio::test]
    async fn test_glob_cargo_toml() {
        let tool = GlobTool::new(200);
        let result = tool
            .execute(serde_json::json!({"pattern": "*.toml"}))
            .await
            .unwrap();
        assert!(result.contains("Cargo.toml"));
    }
}
