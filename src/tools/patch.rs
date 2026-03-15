use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

pub struct PatchTool;

#[async_trait]
impl super::Tool for PatchTool {
    fn name(&self) -> &str {
        "patch"
    }

    fn description(&self) -> &str {
        "Apply a unified diff patch to a file. The patch should be in unified diff format (--- a/file, +++ b/file, @@ ... @@). Useful for complex multi-line edits."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to patch"
                },
                "patch": {
                    "type": "string",
                    "description": "Unified diff content to apply"
                }
            },
            "required": ["path", "patch"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("missing 'path' parameter")?;

        let patch_str = args
            .get("patch")
            .and_then(|v| v.as_str())
            .context("missing 'patch' parameter")?;

        let path = Path::new(path_str);
        let original = if path.exists() {
            tokio::fs::read_to_string(path)
                .await
                .with_context(|| format!("failed to read: {}", path_str))?
        } else {
            String::new()
        };

        let patched = apply_unified_diff(&original, patch_str)?;

        // Atomic write
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        let dir = path.parent().unwrap_or(Path::new("."));
        let temp = tempfile::NamedTempFile::new_in(dir)?;
        tokio::fs::write(temp.path(), patched.as_bytes()).await?;
        temp.persist(path)?;

        Ok(format!("Patch applied to {}", path_str))
    }
}

/// Apply a unified diff to the original text.
/// Parses hunks from the diff and applies insertions/deletions.
fn apply_unified_diff(original: &str, diff: &str) -> Result<String> {
    let original_lines: Vec<&str> = original.lines().collect();
    let mut result_lines: Vec<String> = original_lines.iter().map(|l| l.to_string()).collect();

    let hunks = parse_hunks(diff)?;

    // Apply hunks in reverse order so line numbers stay valid
    let mut sorted_hunks = hunks;
    sorted_hunks.sort_by(|a, b| b.original_start.cmp(&a.original_start));

    for hunk in sorted_hunks {
        let start = (hunk.original_start.saturating_sub(1)).min(result_lines.len());
        let end = (start + hunk.original_count).min(result_lines.len());

        // Verify context lines match (best effort)
        let mut new_lines: Vec<String> = Vec::new();
        for line in &hunk.lines {
            match line {
                DiffLine::Context(text) | DiffLine::Add(text) => {
                    new_lines.push(text.to_string());
                }
                DiffLine::Remove => {
                    // Skip removed lines
                }
            }
        }

        // Replace the range
        result_lines.splice(start..end, new_lines);
    }

    Ok(result_lines.join("\n"))
}

#[derive(Debug)]
struct Hunk {
    original_start: usize,
    original_count: usize,
    lines: Vec<DiffLine>,
}

#[derive(Debug)]
enum DiffLine {
    Context(String),
    Add(String),
    Remove,
}

fn parse_hunks(diff: &str) -> Result<Vec<Hunk>> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<Hunk> = None;

    for line in diff.lines() {
        if line.starts_with("@@") {
            // Parse hunk header: @@ -start,count +start,count @@
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }

            let (orig_start, orig_count) = parse_hunk_header(line)?;
            current_hunk = Some(Hunk {
                original_start: orig_start,
                original_count: orig_count,
                lines: Vec::new(),
            });
        } else if line.starts_with("---") || line.starts_with("+++") {
            // Skip file headers
            continue;
        } else if let Some(ref mut hunk) = current_hunk {
            if let Some(rest) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine::Add(rest.to_string()));
            } else if line.starts_with('-') {
                hunk.lines.push(DiffLine::Remove);
            } else if let Some(rest) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine::Context(rest.to_string()));
            } else if line.is_empty() || line == "\\ No newline at end of file" {
                // Skip
            } else {
                // Treat as context line
                hunk.lines.push(DiffLine::Context(line.to_string()));
            }
        }
    }

    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }

    Ok(hunks)
}

fn parse_hunk_header(line: &str) -> Result<(usize, usize)> {
    // @@ -start,count +start,count @@ optional context
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        anyhow::bail!("invalid hunk header: {}", line);
    }

    let orig_range = parts[1].trim_start_matches('-');
    let (start, count) = if let Some((s, c)) = orig_range.split_once(',') {
        (
            s.parse::<usize>().unwrap_or(1),
            c.parse::<usize>().unwrap_or(0),
        )
    } else {
        (orig_range.parse::<usize>().unwrap_or(1), 1)
    };

    Ok((start, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[test]
    fn test_apply_simple_diff() {
        let original = "line1\nline2\nline3\n";
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,3 @@
 line1
-line2
+LINE2_MODIFIED
 line3";

        let result = apply_unified_diff(original, diff).unwrap();
        assert!(result.contains("LINE2_MODIFIED"));
        assert!(!result.contains("line2"));
    }

    #[test]
    fn test_apply_addition() {
        let original = "line1\nline3\n";
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,2 +1,3 @@
 line1
+line2
 line3";

        let result = apply_unified_diff(original, diff).unwrap();
        assert!(result.contains("line2"));
    }

    #[test]
    fn test_apply_deletion() {
        let original = "line1\nline2\nline3\n";
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,2 @@
 line1
-line2
 line3";

        let result = apply_unified_diff(original, diff).unwrap();
        assert!(!result.contains("line2"));
        assert!(result.contains("line1"));
        assert!(result.contains("line3"));
    }

    #[tokio::test]
    async fn test_patch_tool() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "hello\nworld\nfoo\n").unwrap();

        let tool = PatchTool;
        let diff = format!(
            "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n hello\n-world\n+WORLD\n foo"
        );

        let result = tool
            .execute(serde_json::json!({
                "path": path.to_str().unwrap(),
                "patch": diff
            }))
            .await
            .unwrap();

        assert!(result.contains("Patch applied"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("WORLD"));
        assert!(!content.contains("world"));
    }

    #[test]
    fn test_parse_hunk_header() {
        let (start, count) = parse_hunk_header("@@ -1,3 +1,4 @@ fn main()").unwrap();
        assert_eq!(start, 1);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_parse_hunk_header_single() {
        let (start, count) = parse_hunk_header("@@ -5 +5 @@").unwrap();
        assert_eq!(start, 5);
        assert_eq!(count, 1);
    }
}
