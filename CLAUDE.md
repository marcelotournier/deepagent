# CLAUDE.md — deepagent

## What This Is

A Rust CLI (“deepagent”) installable via `pip install deepagent` (maturin bin binding).
Runs a ReAct-loop coding agent powered by Gemini 3 Flash (Google AI Studio API).
Equivalent to `claude -p` / `gemini -p` / `codex -p` — a single-shot piped prompt that
reads stdin or `-p "prompt"`, executes tools, and prints the result.

Target device: **Raspberry Pi 4/5** (ARM64, 1-8 GB RAM).

-----

## Quick Start

```bash
# Install from source (requires Rust ≥ 1.75 + Python ≥ 3.10)
pip install maturin
maturin build --release --target aarch64-unknown-linux-gnu
pip install target/wheels/deepagent-*.whl

# Or dev mode
maturin develop --release

# Run
export GEMINI_API_KEY="your-key-from-ai.google.dev"
echo "create a hello world in rust" | deepagent
deepagent -p "refactor main.rs to use clap derive"
cat error.log | deepagent -p "diagnose this crash"
```

-----

## Architecture

```
deepagent/
├── Cargo.toml              # Workspace root
├── pyproject.toml           # Maturin bin binding config
├── CLAUDE.md                # This file — read first
├── README.md
├── src/
│   ├── main.rs              # CLI entry: clap arg parsing, prompt assembly
│   ├── lib.rs               # Library root, re-exports
│   ├── cli/
│   │   └── mod.rs           # Clap derive structs, output formatting
│   ├── api/
│   │   ├── mod.rs           # Trait definitions for LLM backends
│   │   ├── gemini.rs        # Gemini REST client (reqwest)
│   │   └── rate_limiter.rs  # Token-bucket + exponential backoff
│   ├── agent/
│   │   ├── mod.rs           # Agent trait, message types
│   │   └── executor.rs      # ReAct loop: think → act → observe → repeat
│   └── tools/
│       ├── mod.rs           # ToolRegistry, Tool trait, JSON schema
│       ├── bash.rs          # Shell execution (tokio::process)
│       ├── read.rs          # File read with line numbers
│       ├── write.rs         # File write (atomic via tempfile)
│       ├── edit.rs          # Str-replace edit (old_str → new_str)
│       ├── grep.rs          # Ripgrep wrapper / regex content search
│       ├── glob.rs          # Glob pattern file matching
│       └── ls.rs            # Directory listing (2-level depth)
├── tests/
│   └── integration.rs       # End-to-end agent tests
├── benches/
│   └── benchmark_tasks.rs   # Criterion benchmarks for tool execution
├── scripts/
│   ├── benchmark.sh         # Compare deepagent vs `claude -p` on task suite
│   └── setup_pi.sh          # Raspberry Pi environment setup
├── python/
│   └── deepagent/
│       └── __init__.py      # Thin Python wrapper (optional)
└── .github/
    └── workflows/
        └── ci.yml           # Build + test on x86_64 and aarch64
```

-----

## Key Crates

|Crate                         |Purpose                                                        |
|------------------------------|---------------------------------------------------------------|
|`rig-core`                    |LLM agent framework — agent builder, tool trait, completion API|
|`autoagents-core`             |Multi-agent orchestration, ReAct executor, memory              |
|`rayon`                       |Parallel file scanning, glob, grep (maximize Pi cores)         |
|`tokio`                       |Async runtime for API calls and process spawning               |
|`reqwest`                     |HTTP client for Gemini API                                     |
|`clap`                        |CLI argument parsing (derive)                                  |
|`serde` / `serde_json`        |JSON serialization for tool schemas and API payloads           |
|`glob`                        |File pattern matching                                          |
|`grep-regex` / `grep-searcher`|Content search (ripgrep internals)                             |
|`tempfile`                    |Atomic file writes                                             |
|`anyhow`                      |Error handling                                                 |
|`tracing`                     |Structured logging                                             |
|`criterion`                   |Benchmarks                                                     |
|`similar`                     |Diff output for edits                                          |

-----

## Gemini API Configuration

### Model Selection (priority order, free tier)

1. **`gemini-2.5-flash`** — 10 RPM, 250 RPD, 250k TPM (best free-tier balance)
1. **`gemini-2.5-flash-lite`** — 15 RPM, 1000 RPD (fallback for high-volume)
1. **`gemini-2.5-pro`** — 5 RPM, 100 RPD (complex reasoning only)
1. **`gemini-3-flash-preview`** — if available on paid tier

### Free Tier Rate Limits (as of March 2026)

- Limits are **per-project**, not per-key
- RPD resets at **midnight Pacific Time**
- 250k TPM shared across all models
- Gemini 3.x preview models: paid tier only

### Exponential Backoff Strategy

```
Base delay:     1 second
Max delay:      60 seconds
Max retries:    8
Jitter:         ±25% random
Backoff factor: 2x
429 handling:   respect Retry-After header if present
Daily budget:   track RPD in-process, pause when at 90% quota
```

### Free-Usage Optimization Rules

1. **Batch context**: pack system prompt + file contents in one request
1. **Model routing**: use flash-lite for simple tool dispatch, flash for reasoning
1. **Token compression**: truncate file reads to relevant sections (head/tail)
1. **Request coalescing**: combine sequential tool results before next LLM call
1. **Cache system prompt**: reuse across turns (Gemini caches >32k contexts)
1. **Daily budget guard**: after 225 RPD (90%), switch to flash-lite or queue

-----

## Tool Specifications

Each tool matches what Claude Code, Gemini CLI, Codex CLI, and OpenCode use.

### Bash

- Execute shell commands via `tokio::process::Command`
- Persistent working directory per session
- Timeout: 120s default, configurable
- Capture stdout + stderr separately
- Truncate output to 8192 chars (configurable)

### Read

- Read file contents with line numbers (`{line_num}\t{content}`)
- Support range: `start_line..end_line`
- Binary detection → hex dump fallback
- Image files → base64 (for future multimodal)
- Max read: 32k chars, paginate beyond

### Write

- Atomic writes via tempfile + rename
- Create parent directories
- Return bytes written

### Edit

- `old_str` → `new_str` replacement
- Must match exactly once (fail if 0 or >1 matches)
- Optional `replace_all` flag
- Show diff after edit

### Grep

- Regex pattern search across files
- Options: case-insensitive, file-type filter, context lines
- Uses rayon for parallel directory walks
- Output modes: files_with_matches, content, count
- Max results: 100 matches default

### Glob

- File pattern matching (`**/*.rs`, `src/**/*.toml`)
- Sorted by modification time (newest first)
- Max results: 200 files
- Uses rayon for parallel stat calls

### Ls

- List directory contents, 2 levels deep
- Ignore: `.git`, `node_modules`, `target`, `__pycache__`, `.venv`
- Show file sizes, directory markers

-----

## Agent Loop (ReAct)

```
1. Receive prompt (stdin or -p flag)
2. Build system message with tool definitions (JSON schema)
3. Send to Gemini API
4. Parse response:
   a. If text-only → print and exit
   b. If tool_call → execute tool → append observation → goto 3
5. Max iterations: 25 (configurable via --max-turns)
6. On error: retry with error context appended
```

### System Prompt Template

```
You are a coding agent. You have access to these tools:
{tool_schemas}

Working directory: {cwd}
OS: {os_info}

Rules:
- Use Grep/Glob to find files before reading them
- Read files before editing them
- Run tests after making changes
- Be concise in explanations
- Batch independent operations

Respond with either:
1. A text response (if done)
2. A function_call to use a tool
```

-----

## Raspberry Pi Optimization

### Memory

- Stream API responses (don’t buffer full response body)
- Tool output truncation (configurable per-tool limits)
- Limit concurrent file operations to num_cpus
- Drop large strings eagerly (no reference cycles)
- Compile with `--release` + `opt-level = 3` + `lto = "thin"`

### CPU (rayon)

- `RAYON_NUM_THREADS=4` (Pi 4/5 has 4 cores)
- Parallel grep across files
- Parallel glob stat calls
- Parallel tool execution when tools are independent

### Cargo Profile

```toml
[profile.release]
opt-level = 3
lto = "thin"           # full LTO too slow on Pi
codegen-units = 1
strip = true
panic = "abort"
```

### Cross-Compilation (from x86 host)

```bash
rustup target add aarch64-unknown-linux-gnu
sudo apt install gcc-aarch64-linux-gnu
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
maturin build --release --target aarch64-unknown-linux-gnu
```

-----

## Benchmark Suite

Compare `deepagent -p` vs `claude -p` (Sonnet 4.6) on identical tasks.
Both run from same directory with same file context.

### Task Categories

|# |Task                                                                          |Measures                |
|--|------------------------------------------------------------------------------|------------------------|
|1 |“List all .rs files in this project”                                          |Glob tool, basic routing|
|2 |“Read src/main.rs and explain what it does”                                   |Read + reasoning        |
|3 |“Find all TODO comments in the codebase”                                      |Grep across files       |
|4 |“Create a new file src/utils.rs with a helper function that formats durations”|Write tool              |
|5 |“In src/main.rs, replace the hardcoded timeout with a CLI flag”               |Edit tool + Read        |
|6 |“Run cargo check and fix any errors”                                          |Bash + Edit loop        |
|7 |“Add error handling to all unwrap() calls in src/api/”                        |Multi-file edit         |
|8 |“Write a unit test for the rate_limiter module”                               |Write + reasoning       |
|9 |“Refactor tools/mod.rs to use a HashMap registry”                             |Read + Edit + reasoning |
|10|“Create a shell script that builds and deploys to the Pi”                     |Write + Bash knowledge  |

### Metrics Collected

- **Latency**: wall-clock time from prompt to final output
- **Token usage**: input + output tokens
- **Tool calls**: number and type of tool invocations
- **Correctness**: manual 0-5 score (does the output work?)
- **Cost**: API cost per task (free-tier: $0 for deepagent)

### Running Benchmarks

```bash
# Run full suite
./scripts/benchmark.sh

# Run single task
./scripts/benchmark.sh --task 3

# Compare mode (requires ANTHROPIC_API_KEY for claude -p)
./scripts/benchmark.sh --compare
```

-----

## GitHub Workflow

### Initial Setup

```bash
gh repo create deepagent --public --description "Rust coding agent powered by Gemini, installable via pip"
git init && git add -A
git commit -m "init: project structure, CLAUDE.md, source skeleton"
git branch -M main
git remote add origin git@github.com:YOUR_USER/deepagent.git
git push -u origin main
```

### Commit Convention

```
feat: add grep tool with rayon parallelism
fix: rate limiter not respecting Retry-After header
bench: add task 7 multi-file edit benchmark
docs: update CLAUDE.md with Pi optimization notes
refactor: extract tool trait into separate module
test: integration test for ReAct loop with mock API
```

### CI Pipeline

- Build on `ubuntu-latest` (x86_64)
- Cross-compile for `aarch64-unknown-linux-gnu`
- Run `cargo test`
- Run `cargo clippy -- -D warnings`
- Run `cargo fmt --check`
- Build wheel with `maturin build`

-----

## Development Commands

```bash
# Build
cargo build --release

# Test
cargo test
cargo test -- --nocapture               # see output
cargo test integration                   # integration only

# Lint
cargo clippy -- -D warnings
cargo fmt --check

# Benchmark tool execution
cargo bench

# Build Python wheel
maturin build --release
maturin develop                          # install in venv

# Run
cargo run -- -p "list files in current directory"
echo "explain this code" | cargo run -- --stdin

# Cross-compile for Pi
maturin build --release --target aarch64-unknown-linux-gnu
```

-----

## Environment Variables

|Variable             |Required|Default           |Description                            |
|---------------------|--------|------------------|---------------------------------------|
|`GEMINI_API_KEY`     |yes     |—                 |Google AI Studio API key               |
|`DEEPAGENT_MODEL`    |no      |`gemini-2.5-flash`|Model to use                           |
|`DEEPAGENT_MAX_TURNS`|no      |`25`              |Max agent loop iterations              |
|`DEEPAGENT_TIMEOUT`  |no      |`120`             |Tool execution timeout (seconds)       |
|`DEEPAGENT_LOG`      |no      |`warn`            |Log level (trace/debug/info/warn/error)|
|`RAYON_NUM_THREADS`  |no      |num_cpus          |Parallel threads for file ops          |

-----

## Known Limitations

- No streaming output yet (prints full response after each turn)
- No image/multimodal input (text-only for now)
- Gemini 3.x models require paid tier
- Free tier: 250 RPD on flash, plan tasks accordingly
- No MCP server support (tools are built-in only)
- No sub-agent spawning (single agent loop)

-----

## Roadmap

1. [ ] Core tool implementations (bash, read, write, edit, grep, glob, ls)
1. [ ] Gemini API client with exponential backoff
1. [ ] ReAct agent loop
1. [ ] CLI with clap (-p flag, stdin, –model, –max-turns)
1. [ ] Maturin packaging (pip install deepagent)
1. [ ] Benchmark suite against claude -p
1. [ ] Pi-specific optimizations (rayon tuning, memory limits)
1. [ ] Streaming output
1. [ ] Model fallback chain (flash → flash-lite on 429)
1. [ ] Session persistence (resume interrupted tasks)
