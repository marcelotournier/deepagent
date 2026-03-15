#!/usr/bin/env bash
set -euo pipefail

# Benchmark suite: compare deepagent -p vs claude -p on identical tasks.
# Usage:
#   ./scripts/benchmark.sh              # Run full suite (deepagent only)
#   ./scripts/benchmark.sh --task 3     # Run single task
#   ./scripts/benchmark.sh --compare    # Compare deepagent vs claude -p

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

COMPARE=false
SINGLE_TASK=""
RESULTS_DIR="$PROJECT_DIR/benchmark_results"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --compare) COMPARE=true; shift ;;
        --task) SINGLE_TASK="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

mkdir -p "$RESULTS_DIR"

# Task definitions
declare -a TASKS
TASKS[1]="List all .rs files in this project"
TASKS[2]="Read src/main.rs and explain what it does in 2-3 sentences"
TASKS[3]="Find all TODO comments in the codebase"
TASKS[4]="Create a new file src/utils.rs with a helper function that formats durations into human-readable strings like '2m 30s'"
TASKS[5]="In src/main.rs, find the timeout configuration and explain how it works"
TASKS[6]="Run cargo check and report any errors or warnings"
TASKS[7]="Find all uses of unwrap() in the src/api/ directory"
TASKS[8]="Write a unit test for the format_size function in src/tools/ls.rs"
TASKS[9]="Read src/tools/mod.rs and explain the ToolRegistry design"
TASKS[10]="Create a shell script scripts/setup_pi.sh that installs Rust and builds deepagent for Raspberry Pi"

run_task() {
    local task_num="$1"
    local agent="$2"  # "deepagent" or "claude"
    local prompt="${TASKS[$task_num]}"

    echo "  Task $task_num [$agent]: $prompt"

    local start_time
    start_time=$(python3 -c "import time; print(time.time())")

    local output_file="$RESULTS_DIR/task_${task_num}_${agent}.txt"

    if [[ "$agent" == "deepagent" ]]; then
        timeout 120 cargo run --release -- -p "$prompt" > "$output_file" 2>/dev/null || true
    elif [[ "$agent" == "claude" ]]; then
        if command -v claude &>/dev/null; then
            timeout 120 claude -p "$prompt" > "$output_file" 2>/dev/null || true
        else
            echo "    [SKIP] claude not found" | tee "$output_file"
            return
        fi
    fi

    local end_time
    end_time=$(python3 -c "import time; print(time.time())")

    local elapsed
    elapsed=$(python3 -c "print(f'{$end_time - $start_time:.2f}')")

    local output_size
    output_size=$(wc -c < "$output_file" | tr -d ' ')

    echo "    Time: ${elapsed}s | Output: ${output_size} bytes"
    echo "$task_num,$agent,$elapsed,$output_size" >> "$RESULTS_DIR/summary.csv"
}

# Header
echo "task,agent,time_seconds,output_bytes" > "$RESULTS_DIR/summary.csv"
echo "================================================"
echo "  deepagent Benchmark Suite"
echo "  Project: $PROJECT_DIR"
echo "  Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "================================================"

cd "$PROJECT_DIR"

if [[ -n "$SINGLE_TASK" ]]; then
    tasks=("$SINGLE_TASK")
else
    tasks=(1 2 3 4 5 6 7 8 9 10)
fi

for t in "${tasks[@]}"; do
    run_task "$t" "deepagent"
    if $COMPARE; then
        run_task "$t" "claude"
    fi
done

echo ""
echo "================================================"
echo "  Results saved to: $RESULTS_DIR/summary.csv"
echo "================================================"

if $COMPARE; then
    echo ""
    echo "Side-by-side comparison:"
    echo "Task | deepagent (s) | claude (s) | deepagent (bytes) | claude (bytes)"
    echo "-----|---------------|------------|-------------------|---------------"
    for t in "${tasks[@]}"; do
        da_line=$(grep "^$t,deepagent," "$RESULTS_DIR/summary.csv" || echo "$t,deepagent,N/A,N/A")
        cl_line=$(grep "^$t,claude," "$RESULTS_DIR/summary.csv" || echo "$t,claude,N/A,N/A")
        da_time=$(echo "$da_line" | cut -d',' -f3)
        cl_time=$(echo "$cl_line" | cut -d',' -f3)
        da_bytes=$(echo "$da_line" | cut -d',' -f4)
        cl_bytes=$(echo "$cl_line" | cut -d',' -f4)
        printf "  %2s | %13s | %10s | %17s | %s\n" "$t" "$da_time" "$cl_time" "$da_bytes" "$cl_bytes"
    done
fi
