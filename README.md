# deepagent

A Rust CLI coding agent powered by Gemini (Google AI Studio), installable via `pip install deepagent`.

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

## Tools

| Tool  | Description |
|-------|-------------|
| bash  | Execute shell commands with timeout and output truncation |
| read  | Read files with line numbers, range support |
| write | Atomic file writes via tempfile + rename |
| edit  | Exact string replacement with diff output |
| grep  | Parallel regex search across files (rayon) |
| glob  | File pattern matching sorted by modification time |
| ls    | Directory listing, 2 levels deep |

## Benchmarks

Compare deepagent vs `claude -p` on 10 standardized tasks:

```bash
./scripts/benchmark.sh           # deepagent only
./scripts/benchmark.sh --compare # side-by-side with claude -p
./scripts/benchmark.sh --task 3  # single task
```

## Development

```bash
cargo build --release
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## License

MIT
