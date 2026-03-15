use anyhow::{Context, Result};
use async_trait::async_trait;
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;

pub struct GrepTool {
    max_results: usize,
}

impl GrepTool {
    pub fn new(max_results: usize) -> Self {
        Self { max_results }
    }
}

#[async_trait]
impl super::Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents using regex patterns. Supports case-insensitive search, file type filters, and context lines."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in (default: current directory)"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case-insensitive search (default: false)"
                },
                "file_type": {
                    "type": "string",
                    "description": "Filter by file extension (e.g., 'rs', 'py', 'js')"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Lines of context before and after each match (default: 0)"
                },
                "output_mode": {
                    "type": "string",
                    "description": "Output mode: 'content' (matching lines), 'files_with_matches' (file paths only), 'count' (match counts). Default: 'content'",
                    "enum": ["content", "files_with_matches", "count"]
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let pattern_str = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .context("missing 'pattern' parameter")?;

        let search_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let case_insensitive = args
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let file_type = args.get("file_type").and_then(|v| v.as_str());

        let context_lines = args
            .get("context_lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let output_mode = args
            .get("output_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("content");

        let regex_pattern = if case_insensitive {
            format!("(?i){}", pattern_str)
        } else {
            pattern_str.to_string()
        };

        let re = Regex::new(&regex_pattern)
            .with_context(|| format!("invalid regex: {}", pattern_str))?;

        let path = Path::new(search_path);
        if !path.exists() {
            anyhow::bail!("path not found: {}", search_path);
        }

        let max_results = self.max_results;
        let results = Mutex::new(Vec::new());

        // Collect files
        let mut walker = WalkBuilder::new(path);
        walker.hidden(false).git_ignore(true);

        let files: Vec<_> = walker
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .filter(|e| {
                if let Some(ext_filter) = file_type {
                    e.path()
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e == ext_filter)
                        .unwrap_or(false)
                } else {
                    true
                }
            })
            .collect();

        // Search in parallel with rayon
        files.par_iter().for_each(|entry| {
            if results.lock().unwrap().len() >= max_results {
                return;
            }

            let path = entry.path();
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => return, // skip binary / unreadable files
            };

            let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            let mut match_indices = Vec::new();

            for (i, line) in lines.iter().enumerate() {
                if re.is_match(line) {
                    match_indices.push(i);
                }
            }

            if !match_indices.is_empty() {
                let mut r = results.lock().unwrap();
                if r.len() < max_results {
                    r.push((path.to_path_buf(), match_indices, lines));
                }
            }
        });

        let results = results.into_inner().unwrap();

        if results.is_empty() {
            return Ok("No matches found.".to_string());
        }

        let mut output = String::new();

        match output_mode {
            "files_with_matches" => {
                for (path, _, _) in &results {
                    output.push_str(&format!("{}\n", path.display()));
                }
            }
            "count" => {
                let mut total = 0;
                for (path, indices, _) in &results {
                    let count = indices.len();
                    total += count;
                    output.push_str(&format!("{}:{}\n", path.display(), count));
                }
                output.push_str(&format!("\nTotal: {} matches", total));
            }
            _ => {
                // content mode
                for (path, indices, lines) in &results {
                    for line_idx in indices {
                        let start = line_idx.saturating_sub(context_lines);
                        let end = (*line_idx + context_lines + 1).min(lines.len());

                        for (i, line) in lines.iter().enumerate().take(end).skip(start) {
                            let prefix = if i == *line_idx { ">" } else { " " };
                            output.push_str(&format!(
                                "{}{}:{}:{}\n",
                                prefix,
                                path.display(),
                                i + 1,
                                line
                            ));
                        }
                        if context_lines > 0 {
                            output.push_str("--\n");
                        }
                    }
                }
            }
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[tokio::test]
    async fn test_grep_finds_pattern() {
        let tool = GrepTool::new(100);
        let result = tool
            .execute(serde_json::json!({
                "pattern": "deepagent",
                "path": "Cargo.toml"
            }))
            .await
            .unwrap();
        assert!(result.contains("deepagent"));
    }

    #[tokio::test]
    async fn test_grep_no_match() {
        let tool = GrepTool::new(100);
        let result = tool
            .execute(serde_json::json!({
                "pattern": "zzzzzzzzz_nonexistent",
                "path": "Cargo.toml"
            }))
            .await
            .unwrap();
        assert!(result.contains("No matches"));
    }

    #[tokio::test]
    async fn test_grep_files_with_matches() {
        let tool = GrepTool::new(100);
        let result = tool
            .execute(serde_json::json!({
                "pattern": "deepagent",
                "path": ".",
                "output_mode": "files_with_matches",
                "file_type": "toml"
            }))
            .await
            .unwrap();
        assert!(result.contains("Cargo.toml"));
    }
}
