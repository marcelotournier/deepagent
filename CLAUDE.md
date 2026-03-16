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

1. **`gemini-3-flash-preview`** — Primary model (Gemini 3 Flash). ~2 RPM observed, 250 RPD.
1. **`gemini-3.1-flash-lite-preview`** — Fallback on 429/503. Higher RPM (~10), 1000 RPD.
1. **`gemini-2.5-flash`** — Stable alternative if 3.x unavailable. ~10 RPM, 250 RPD.

**NEVER fall back to 2.5 models when using 3.x primary.**

### Free Tier Hard Limits (observed, March 2026)

**THIS IS THE #1 CONSTRAINT. All design decisions must respect these limits.**

| Model | RPM (observed) | RPD | TPM |
|-------|---------------|-----|-----|
| gemini-3-flash-preview | ~2 | ~250 | 250k |
| gemini-3.1-flash-lite-preview | ~10 | ~1000 | 250k |
| gemini-2.5-flash | ~10 | ~250 | 250k |
| gemini-2.5-pro | ~2 | ~100 | 250k |

- Limits are **per-project**, not per-key
- RPD resets at **midnight Pacific Time**
- Google uses **both 429 and 503** for rate limiting
- Preview models have **much lower RPM** than documented (~2 vs claimed 10)
- 250k TPM shared across all models

### Daily Budget Math

```
Simple task (--max-turns 1):  1 API call  → 250 tasks/day
Standard task (2 turns):      2 API calls → 125 tasks/day
Complex task (10 turns):     10 API calls →  25 tasks/day
```

Design every feature to minimize API calls per task.

### Exponential Backoff Strategy

```
Base delay:     1 second
Max delay:      60 seconds
Max retries:    8 per model (16 total with fallback)
Jitter:         ±25% random
Backoff factor: 2x
429 AND 503:    both get exponential backoff + retry
Retry-After:    respect header if present
Sticky fallback: once fallen back, STAY on fallback model
                 (don't retry primary every turn — wastes ~30s)
Daily budget:   auto-switch to lite at 90% quota
RPM spacing:    60s / RPM * 1.3 safety margin
                (2 RPM = 39s between requests)
```

### Free-Usage Optimization Rules

1. **Minimize API calls**: Use `--max-turns 1` for simple lookups (1 call vs 2)
1. **Auto-complete**: When max_turns reached, return tool results directly (no summarization call)
1. **Sticky fallback**: After 429, stay on fallback model — don't retry primary every turn
1. **Token compression**: Tool output truncated to 16KB (head 75% + tail 25%)
1. **Request coalescing**: All tool results sent in single message (1 call, not N)
1. **No schema duplication**: Tool schemas sent via API `tools` field, not in system prompt
1. **Daily budget guard**: Auto-switch to lite at 90% RPD
1. **System prompt efficiency**: Instructs model to combine text + tool calls, skip unnecessary verification

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

## Competency Benchmarks (Complex Tasks)

Head-to-head comparison of `deepagent -p` vs `claude -p` on real-world
complex tasks. Each task produces artifacts (files, reports, code) that
can be manually scored for correctness, completeness, and quality.

Both agents run in an isolated temp directory with the same starting files.
Outputs saved as JSON with metrics + artifacts for side-by-side review.

### Complex Task Suite

|#  |Task Prompt                                                                                           |Tests                          |
|---|------------------------------------------------------------------------------------------------------|-------------------------------|
|C1 |"Build a Python CLI calculator app with +, -, *, /, history, and undo. Include tests and a README."   |Full app creation, testing     |
|C2 |"Read all Rust source files in src/ and write a ARCHITECTURE.md explaining the codebase design."      |Code analysis, documentation   |
|C3 |"Create a Bash script that monitors CPU and memory usage every 5s, logs to CSV, and alerts if CPU>80%"|Systems scripting, monitoring  |
|C4 |"Find all error handling patterns in src/ and refactor them to use a consistent error type."          |Multi-file refactoring         |
|C5 |"Write a Rust module src/tools/search.rs that implements fuzzy file search using Levenshtein distance"|Algorithm implementation       |
|C6 |"Analyze the Cargo.toml dependencies and write a security audit report: outdated crates, known CVEs." |Research, analysis, reporting  |
|C7 |"Create a complete GitHub Actions workflow that runs benchmarks on every PR and posts results as a comment"|CI/CD knowledge, YAML       |
|C8 |"Debug: the grep tool returns wrong line numbers when context_lines > 0. Find and fix the bug."       |Debugging, reading, fixing     |
|C9 |"Build a JSON REST API mock server in Python (Flask/FastAPI) with 3 endpoints, tests, and Dockerfile."|Full-stack app creation        |
|C10|"Read the benchmark results in benchmark_results/ and write a performance report comparing the agents."|Data analysis, report writing |

### Scoring Criteria (per task, 0-5 each)

- **Correctness**: Does the output work? Can it compile/run without errors?
- **Completeness**: Did the agent address all parts of the prompt?
- **Quality**: Is the code clean, idiomatic, well-structured?
- **Efficiency**: How many turns/tokens were used? Fewer is better.

### Running Competency Benchmarks

```bash
# Run all complex tasks
./scripts/competency_bench.sh

# Run single task
./scripts/competency_bench.sh --task C3

# Compare mode
./scripts/competency_bench.sh --compare

# Review results
ls competency_results/
cat competency_results/C1_deepagent.json | jq '.metrics'
```

### TODO Cycle

After each benchmark run, evaluate results and create next improvement:
1. Run benchmark → save results
2. Score outputs (0-5 per criterion)
3. Identify weakest task
4. Add feature or improve prompt to address weakness
5. Re-run that task → verify improvement
6. Repeat

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
|`DEEPAGENT_MODEL`    |no      |`gemini-3-flash-preview`|Model to use                     |
|`DEEPAGENT_SYSTEM_PROMPT`|no  |—                 |Override system prompt                 |
|`DEEPAGENT_MAX_TURNS`|no      |`25`              |Max agent loop iterations              |
|`DEEPAGENT_TIMEOUT`  |no      |`120`             |Tool execution timeout (seconds)       |
|`DEEPAGENT_LOG`      |no      |`warn`            |Log level (trace/debug/info/warn/error)|
|`RAYON_NUM_THREADS`  |no      |num_cpus          |Parallel threads for file ops          |

-----

## Gemini 3.x API Requirements

- **Thought signatures**: Gemini 3.x returns `thoughtSignature` on function call
  parts. This MUST be preserved and sent back in the conversation, or you get a
  400 error: "Function call is missing a thought_signature".
  See: https://ai.google.dev/gemini-api/docs/thought-signatures
- **No `additionalProperties`** in tool schemas (Gemini rejects it)
- **No empty `required: []`** arrays in tool schemas
- **Function declarations** sent via API `tools` field, not in system prompt text

## Known Limitations

- No true streaming output (prints after each turn, verbose shows progress)
- No image/multimodal input (text-only)
- Free tier: ~2 RPM on preview models, 250 RPD — design for efficiency
- No MCP server support (tools are built-in only)
- No sub-agent spawning (single agent loop)
- Smart model routing disabled (3.x lite requires thought signatures too)

-----

## Roadmap

1. [x] Core tool implementations (bash, read, write, edit, grep, glob, ls) — 12 tools total
1. [x] Gemini API client with exponential backoff (429 + 503)
1. [x] ReAct agent loop with context compaction and loop detection
1. [x] CLI with clap (-p, stdin, --model, --max-turns, --json, --verbose, --init, --sessions, --resume)
1. [x] Maturin packaging (pip install deepagent)
1. [x] Benchmark suite against claude -p (10 efficiency + 10 competency tasks)
1. [x] Pi-specific optimizations (rayon, release profile, 4.5MB binary)
1. [x] Streaming progress output (--verbose)
1. [x] Model fallback chain (gemini-3-flash → gemini-3.1-flash-lite on 429/503)
1. [x] Session persistence (--resume last)
1. [x] Thought signature support for Gemini 3.x
1. [x] Auto-complete optimization (--max-turns 1 = 1.3s per task)
1. [x] File change tracking in output
1. [x] GitHub release workflow (4 platform binaries)
1. [x] Python wrapper (from deepagent import run, run_json)
1. [x] Criterion benchmarks (16 tool execution benchmarks)

### Benchmark Results (v1.1.0, Gemini 3 Flash, free tier)

**Efficiency benchmarks** (10 tasks, --max-turns 3):
| Task | Time | Turns | Tools | Tokens |
|------|------|-------|-------|--------|
| List files | 87.5s | 2 | 1 | 8.5K |
| Explain code | 87.7s | 2 | 1 | 13.1K |
| Find TODOs | 146.1s | 2 | 1 | 8.4K |
| Explain timeout | 95.4s | 3 | 2 | 19.2K |
| Cargo check | 117.0s | 2 | 1 | 7.9K |
| Find unwrap() | 87.5s | 2 | 1 | 8.3K |
| Explain registry | 154.1s | 2 | 1 | 9.6K |

**Fast mode** (--max-turns 1): 1.3s, 1 API call, 3.9K tokens

**Competency benchmark C2** (architecture docs): 84.6s, 10 turns, 158K tokens, score 18/20
