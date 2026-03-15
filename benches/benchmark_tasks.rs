//! Criterion benchmarks for deepagent tool execution.
//!
//! Measures raw tool performance independent of LLM calls.
//! Critical for Raspberry Pi optimization — ensures tools stay fast
//! on constrained hardware.
//!
//! Run with: cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use deepagent::tools::{self, Tool, ToolRegistry};
use tokio::runtime::Runtime;

fn bench_bash_tool(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tool = tools::bash::BashTool::new(std::env::current_dir().unwrap(), 10, 8192);

    c.bench_function("bash_echo", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({"command": "echo hello"})))
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("bash_ls", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({"command": "ls src/"})))
                    .await
                    .unwrap()
            })
        })
    });
}

fn bench_read_tool(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tool = tools::read::ReadTool::new(32000);

    c.bench_function("read_cargo_toml", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({"path": "Cargo.toml"})))
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("read_with_range", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(
                    serde_json::json!({"path": "Cargo.toml", "start_line": 1, "end_line": 10}),
                ))
                .await
                .unwrap()
            })
        })
    });
}

fn bench_write_tool(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tool = tools::write::WriteTool;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bench_write.txt");
    let path_str = path.to_str().unwrap().to_string();

    c.bench_function("write_small_file", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({
                    "path": &path_str,
                    "content": "hello world\n"
                })))
                .await
                .unwrap()
            })
        })
    });
}

fn bench_edit_tool(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tool = tools::edit::EditTool;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bench_edit.txt");
    let path_str = path.to_str().unwrap().to_string();

    c.bench_function("edit_replace", |b| {
        b.iter(|| {
            // Reset file each iteration
            std::fs::write(&path, "hello world foo bar").unwrap();
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({
                    "path": &path_str,
                    "old_str": "hello",
                    "new_str": "goodbye"
                })))
                .await
                .unwrap()
            })
        })
    });
}

fn bench_grep_tool(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tool = tools::grep::GrepTool::new(100);

    c.bench_function("grep_single_file", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({
                    "pattern": "deepagent",
                    "path": "Cargo.toml"
                })))
                .await
                .unwrap()
            })
        })
    });

    c.bench_function("grep_directory", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({
                    "pattern": "fn ",
                    "path": "src/",
                    "file_type": "rs"
                })))
                .await
                .unwrap()
            })
        })
    });
}

fn bench_glob_tool(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tool = tools::glob::GlobTool::new(200);

    c.bench_function("glob_rs_files", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({"pattern": "**/*.rs"})))
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("glob_toml_files", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({"pattern": "*.toml"})))
                    .await
                    .unwrap()
            })
        })
    });
}

fn bench_ls_tool(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tool = tools::ls::LsTool::new(2);

    c.bench_function("ls_current_dir", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({"path": "."})))
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("ls_src_dir", |b| {
        b.iter(|| {
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({"path": "src/"})))
                    .await
                    .unwrap()
            })
        })
    });
}

fn bench_tool_registry(c: &mut Criterion) {
    let working_dir = std::env::current_dir().unwrap();

    c.bench_function("registry_create", |b| {
        b.iter(|| {
            black_box(ToolRegistry::with_defaults(working_dir.clone()));
        })
    });

    let registry = ToolRegistry::with_defaults(working_dir);

    c.bench_function("registry_schemas", |b| {
        b.iter(|| {
            black_box(registry.schemas());
        })
    });

    c.bench_function("registry_lookup", |b| {
        b.iter(|| {
            black_box(registry.get("bash"));
            black_box(registry.get("read"));
            black_box(registry.get("grep"));
        })
    });
}

fn bench_patch_tool(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tool = tools::patch::PatchTool;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bench_patch.txt");
    let path_str = path.to_str().unwrap().to_string();

    let diff = "--- a/file\n+++ b/file\n@@ -1,3 +1,3 @@\n line1\n-line2\n+LINE2\n line3";

    c.bench_function("patch_apply", |b| {
        b.iter(|| {
            std::fs::write(&path, "line1\nline2\nline3\n").unwrap();
            rt.block_on(async {
                tool.execute(black_box(serde_json::json!({
                    "path": &path_str,
                    "patch": diff
                })))
                .await
                .unwrap()
            })
        })
    });
}

criterion_group!(
    benches,
    bench_bash_tool,
    bench_read_tool,
    bench_write_tool,
    bench_edit_tool,
    bench_grep_tool,
    bench_glob_tool,
    bench_ls_tool,
    bench_tool_registry,
    bench_patch_tool,
);
criterion_main!(benches);
