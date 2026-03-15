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
TASKS[10]="Create a shell script scripts/hello.sh that prints system info and the current date"

run_deepagent() {
    local task_num="$1"
    local prompt="${TASKS[$task_num]}"

    echo "  Task $task_num [deepagent]: ${prompt:0:60}..."

    local output_file="$RESULTS_DIR/task_${task_num}_deepagent.json"

    # Use JSON mode for structured metrics
    timeout 180 cargo run --release -q -- --json -p "$prompt" > "$output_file" 2>/dev/null || {
        echo '{"result":"(timeout or error)","metrics":{"elapsed_ms":180000,"turns":0,"tool_calls":0}}' > "$output_file"
    }

    # Extract metrics from JSON
    local elapsed tool_calls turns
    elapsed=$(python3 -c "import json,sys; d=json.load(open('$output_file')); print(d.get('metrics',{}).get('elapsed_ms',0))" 2>/dev/null || echo "0")
    tool_calls=$(python3 -c "import json,sys; d=json.load(open('$output_file')); print(d.get('metrics',{}).get('tool_calls',0))" 2>/dev/null || echo "0")
    turns=$(python3 -c "import json,sys; d=json.load(open('$output_file')); print(d.get('metrics',{}).get('turns',0))" 2>/dev/null || echo "0")

    local elapsed_s
    elapsed_s=$(python3 -c "print(f'{int($elapsed)/1000:.2f}')")

    echo "    Time: ${elapsed_s}s | Turns: $turns | Tool calls: $tool_calls"
    echo "$task_num,deepagent,$elapsed_s,$turns,$tool_calls" >> "$RESULTS_DIR/summary.csv"
}

run_claude() {
    local task_num="$1"
    local prompt="${TASKS[$task_num]}"

    if ! command -v claude &>/dev/null; then
        echo "    [SKIP] claude not found"
        echo "$task_num,claude,N/A,N/A,N/A" >> "$RESULTS_DIR/summary.csv"
        return
    fi

    echo "  Task $task_num [claude -p]: ${prompt:0:60}..."

    local output_file="$RESULTS_DIR/task_${task_num}_claude.txt"
    local start_time
    start_time=$(python3 -c "import time; print(time.time())")

    timeout 180 claude -p "$prompt" > "$output_file" 2>/dev/null || true

    local end_time elapsed_s output_size
    end_time=$(python3 -c "import time; print(time.time())")
    elapsed_s=$(python3 -c "print(f'{$end_time - $start_time:.2f}')")
    output_size=$(wc -c < "$output_file" | tr -d ' ')

    echo "    Time: ${elapsed_s}s | Output: ${output_size} bytes"
    echo "$task_num,claude,$elapsed_s,N/A,N/A" >> "$RESULTS_DIR/summary.csv"
}

# Header
echo "task,agent,time_seconds,turns,tool_calls" > "$RESULTS_DIR/summary.csv"
echo "================================================"
echo "  deepagent Benchmark Suite v0.1.0"
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
    run_deepagent "$t"
    if $COMPARE; then
        run_claude "$t"
    fi
done

echo ""
echo "================================================"
echo "  Results saved to: $RESULTS_DIR/"
echo "================================================"

if $COMPARE; then
    echo ""
    echo "Side-by-side comparison:"
    printf "%-5s | %-15s | %-15s | %-8s | %-8s\n" "Task" "deepagent (s)" "claude (s)" "turns" "tools"
    printf "%-5s-+-%-15s-+-%-15s-+-%-8s-+-%-8s\n" "-----" "---------------" "---------------" "--------" "--------"
    for t in "${tasks[@]}"; do
        da_time=$(awk -F',' "/^$t,deepagent,/{print \$3}" "$RESULTS_DIR/summary.csv" || echo "N/A")
        cl_time=$(awk -F',' "/^$t,claude,/{print \$3}" "$RESULTS_DIR/summary.csv" || echo "N/A")
        da_turns=$(awk -F',' "/^$t,deepagent,/{print \$4}" "$RESULTS_DIR/summary.csv" || echo "N/A")
        da_tools=$(awk -F',' "/^$t,deepagent,/{print \$5}" "$RESULTS_DIR/summary.csv" || echo "N/A")
        printf "  %2s  | %13s | %13s | %6s | %6s\n" "$t" "$da_time" "$cl_time" "$da_turns" "$da_tools"
    done
fi

echo ""
echo "Individual results in $RESULTS_DIR/task_*"
