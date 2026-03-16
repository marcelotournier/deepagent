# deepagent

A Rust CLI coding agent powered by Gemini 3 Flash (Google AI Studio free tier), installable via `pip install deepagent`.

Equivalent to `claude -p` / `gemini -p` / `codex -p` — a single-shot piped prompt that reads stdin or `-p "prompt"`, executes tools, and prints the result.

**Target device:** Raspberry Pi 4/5 (ARM64, 1-8 GB RAM)
**Default model:** `gemini-3-flash-preview` → fallback `gemini-3.1-flash-lite-preview`
**Cost:** $0 (free tier: ~250 RPD, ~2 RPM on preview models)

## Install

### From release binaries (fastest)

```bash
# Download for your platform from:
# https://github.com/marcelotournier/deepagent/releases/latest
curl -L https://github.com/marcelotournier/deepagent/releases/latest/download/deepagent-aarch64-unknown-linux-gnu.tar.gz | tar xz
sudo mv deepagent /usr/local/bin/
```

### From source

```bash
pip install maturin
git clone https://github.com/marcelotournier/deepagent.git
cd deepagent
maturin build --release
pip install target/wheels/deepagent-*.whl
```

### Raspberry Pi

```bash
./scripts/setup_pi.sh  # installs Rust, builds, installs wheel
```

## Usage

```bash
export GEMINI_API_KEY="your-key-from-ai.google.dev"

# Basic usage
deepagent -p "list all .rs files in this project"

# Pipe input
echo "explain this code" | deepagent
cat error.log | deepagent -p "diagnose this crash"

# Verbose mode (shows tool calls on stderr)
deepagent -v -p "fix the bug in src/main.rs"

# JSON output (for scripting/benchmarks)
deepagent --json -p "list files" | jq '.metrics'

# Session management
deepagent --sessions              # list saved sessions
deepagent --resume last           # continue last session

# Project setup
deepagent --init                  # create DEEPAGENT.md config

# Model selection
deepagent --model gemini-2.5-pro -p "complex refactor"
deepagent --max-turns 10 -p "quick check"
deepagent --timeout 60 -p "fast task"
deepagent --system-prompt "You are a Python expert" -p "optimize this"
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

## Free-Tier Optimization

**Hard limits (observed):** ~2 RPM, 250 RPD on gemini-3-flash-preview. Design for efficiency.

- **Auto-complete**: `--max-turns 1` returns tool results directly (1.3s, 1 API call)
- **Sticky fallback**: After 429/503, stay on fallback model (saves ~30s/turn)
- **429 + 503 backoff**: Google uses both for rate limiting — both get exp backoff
- **RPM spacing**: 39s between requests on preview models (2 RPM + 30% margin)
- **Auto-switch at 90% budget**: Seamlessly moves to Flash Lite
- **Context compaction**: Auto-summarize at 80% of 1M token window
- **No schema duplication**: Tool schemas via API field, not in prompt text
- **Request coalescing**: All tool results in single message
- **Budget**: 125 simple tasks/day or 25 complex tasks/day

## Benchmarks

### Efficiency benchmarks (10 tool tasks)

```bash
./scripts/benchmark.sh              # deepagent only (JSON metrics)
./scripts/benchmark.sh --compare    # side-by-side with claude -p
./scripts/benchmark.sh --task 3     # single task
```

### Competency benchmarks (10 complex tasks)

Real-world tasks: build apps, write reports, refactor code, debug.

```bash
./scripts/competency_bench.sh              # all 10 tasks
./scripts/competency_bench.sh --task C1    # single task
./scripts/competency_bench.sh --compare    # vs claude -p
```

Tasks: CLI calculator app, architecture docs, system monitor script,
error handling refactor, fuzzy search module, security audit, CI workflow,
bug investigation, REST API server, performance report.

### Tool execution (Criterion)

```bash
cargo bench
```

Results on Apple Silicon M1:
| Tool | Latency |
|------|---------|
| registry lookup | 47 ns |
| read file | 27 µs |
| grep single file | 39 µs |
| write file | 168 µs |
| edit replace | 227 µs |
| grep directory | 653 µs |
| bash echo | 2.6 ms |

## Python Wrapper

```python
from deepagent import run, run_json

result = run("list all .rs files")
data = run_json("explain src/main.rs")
print(data["metrics"]["total_tokens"])
```

## Development

```bash
cargo build --release           # 4.5MB binary
cargo test                      # 103 tests
cargo bench                     # 16 Criterion benchmarks
cargo clippy -- -D warnings
cargo fmt --check
cargo doc --no-deps
```

## CI

5 GitHub Actions jobs on every push:
- Check formatting + Clippy + Bench compile
- Test on Ubuntu + macOS
- Build Python wheel
- Cross-compile for aarch64 (Raspberry Pi)

Release workflow on tags: builds binaries for x86_64-linux, aarch64-linux, x86_64-macos, aarch64-macos.

## License

MIT
