# deepagent

A Rust CLI coding agent powered by Gemini Flash 3.1 (Google AI Studio free tier), installable via `pip install deepagent`.

Equivalent to `claude -p` / `gemini -p` / `codex -p` — a single-shot piped prompt that reads stdin or `-p "prompt"`, executes tools, and prints the result.

**Target device:** Raspberry Pi 4/5 (ARM64, 1-8 GB RAM)

## Quick Start

```bash
# Install from source (requires Rust >= 1.75 + Python >= 3.10)
pip install maturin
maturin build --release
pip install target/wheels/deepagent-*.whl

# Or dev mode
maturin develop --release

# Run
export GEMINI_API_KEY="your-key-from-ai.google.dev"
deepagent -p "list all .rs files in this project"
echo "explain this code" | deepagent
cat error.log | deepagent -p "diagnose this crash"
```

## Tools (12)

| Tool       | Description |
|------------|-------------|
| bash       | Execute shell commands with timeout and output truncation |
| read       | Read files with line numbers, range support |
| write      | Atomic file writes via tempfile + rename |
| edit       | Exact string replacement with diff output |
| grep       | Parallel regex search across files (rayon) |
| glob       | File pattern matching sorted by modification time |
| ls         | Directory listing, 2 levels deep |
| patch      | Apply unified diffs for complex multi-line edits |
| webfetch   | Fetch web content (HTTP GET with headers) |
| todowrite  | Manage task lists during execution |
| todoread   | Read current task list |
| think      | Step-by-step reasoning before acting |

## Features

- **Model fallback chain**: Flash 3.1 → Flash Lite on rate limits
- **RPM-aware rate limiting**: Exponential backoff with jitter, respects Retry-After
- **Context compaction**: Auto-summarize at 80% of 1M token window
- **Tool output truncation**: 16KB head+tail split for large outputs
- **Streaming progress**: `--verbose` shows tool calls in real-time on stderr
- **JSON output**: `--json` for structured output with events and timing
- **Project config**: Reads `DEEPAGENT.md` or `CLAUDE.md` for project instructions
- **Pre-commit hooks**: cargo fmt + clippy + test on every commit
- **CI pipeline**: 5 GitHub Actions jobs (lint, test x2, wheel, aarch64 cross-compile)

## CLI

```
deepagent -p "your prompt"              # basic usage
echo "code" | deepagent -p "explain"    # combine stdin + prompt
deepagent -v -p "fix this bug"          # verbose progress
deepagent --json -p "list files"        # structured JSON output
deepagent --model gemini-2.5-pro -p "hard task"  # use a different model
deepagent --max-turns 10 -p "simple task"         # limit iterations
deepagent --timeout 60 -p "quick check"           # set tool timeout
```

## Benchmarks

Compare deepagent vs `claude -p` on 10 standardized tasks:

```bash
./scripts/benchmark.sh           # deepagent only
./scripts/benchmark.sh --compare # side-by-side with claude -p
./scripts/benchmark.sh --task 3  # single task
```

## Raspberry Pi Setup

```bash
./scripts/setup_pi.sh  # installs Rust, builds, installs wheel
```

## Development

```bash
cargo build --release
cargo test                    # 71 tests
cargo clippy -- -D warnings
cargo fmt --check
```

## License

MIT
