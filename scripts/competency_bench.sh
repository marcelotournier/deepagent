#!/usr/bin/env bash
set -euo pipefail

# Competency benchmark suite: complex real-world tasks.
# Compares deepagent -p vs claude -p on tasks that produce artifacts.
#
# Usage:
#   ./scripts/competency_bench.sh              # Run all (deepagent only)
#   ./scripts/competency_bench.sh --task C3    # Single task
#   ./scripts/competency_bench.sh --compare    # Both agents side-by-side

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RESULTS_DIR="$PROJECT_DIR/competency_results"

COMPARE=false
SINGLE_TASK=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --compare) COMPARE=true; shift ;;
        --task) SINGLE_TASK="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

mkdir -p "$RESULTS_DIR"

# Complex task definitions
declare -A TASKS
TASKS[C1]="Build a Python CLI calculator app with add, subtract, multiply, divide operations. It should support history (show last 10 calculations) and undo (revert last calculation). Include pytest tests for all operations and edge cases (division by zero). Include a README.md with usage examples. Put everything in a calculator/ directory."
TASKS[C2]="Read all Rust source files in the src/ directory and write a file called ARCHITECTURE.md that explains: 1) The overall design and module structure, 2) How the agent loop works, 3) How tools are registered and executed, 4) How rate limiting and model fallback work, 5) Key design decisions and trade-offs. Be thorough and specific."
TASKS[C3]="Create a Bash script called scripts/monitor.sh that: 1) Monitors CPU and memory usage every 5 seconds, 2) Logs timestamp, CPU%, and memory% to a CSV file (monitor_log.csv), 3) Prints a warning to stderr if CPU > 80% or memory > 90%, 4) Accepts --interval and --output flags for customization, 5) Handles SIGINT gracefully (prints summary before exit). Make it work on both Linux and macOS. IMPORTANT: Do NOT run the script after writing it — only verify syntax with 'bash -n scripts/monitor.sh'."
TASKS[C4]="Find all error handling patterns in the src/ directory. Identify places that use unwrap(), expect(), or inconsistent error types. Refactor them to use anyhow::Result consistently. Create a summary of changes made."
TASKS[C5]="Write a new Rust module at src/tools/search.rs that implements fuzzy file search using Levenshtein distance. It should: 1) Accept a query string and search directory, 2) Find files whose names are similar to the query (threshold: distance <= 3), 3) Sort results by similarity (closest match first), 4) Register as a tool named 'search' in the tool registry, 5) Include unit tests."
TASKS[C6]="Analyze the Cargo.toml dependencies for this project. For each dependency: check if it's the latest version, look for known security issues, and assess if it's actively maintained. Write a SECURITY_AUDIT.md report with: 1) Summary table of all deps with version status, 2) Any security concerns, 3) Recommendations for updates or replacements."
TASKS[C7]="Create a GitHub Actions workflow file at .github/workflows/bench-pr.yml that: 1) Triggers on pull requests to main, 2) Runs cargo bench and captures output, 3) Compares benchmark results against the base branch, 4) Posts a comment on the PR with a formatted table showing performance changes (regressions in red, improvements in green). Use the github-script action for the comment."
TASKS[C8]="There might be a bug in the grep tool (src/tools/grep.rs): when context_lines > 0, the output may show incorrect line numbers or duplicate lines for adjacent matches. Investigate the code, write a test that demonstrates the issue, and fix it if found. If no bug exists, document why the implementation is correct."
TASKS[C9]="Build a JSON REST API mock server in Python using FastAPI. Create a mock_server/ directory with: 1) main.py with 3 endpoints: GET /users, GET /users/{id}, POST /users, 2) models.py with Pydantic models, 3) test_api.py with pytest tests for all endpoints, 4) requirements.txt, 5) Dockerfile to containerize it. The server should store users in memory and return proper HTTP status codes. IMPORTANT: Do NOT start the server — just write the files and verify syntax with 'python3 -m py_compile mock_server/main.py'."
TASKS[C10]="Read any benchmark result files in benchmark_results/ and competency_results/ directories. Write a BENCHMARK_REPORT.md that includes: 1) Executive summary of agent performance, 2) Task-by-task analysis with timing and token usage, 3) Strengths and weaknesses identified, 4) Recommendations for improvement, 5) Comparison table if both agents' results are available."

run_task() {
    local task_id="$1"
    local agent="$2"
    local prompt="${TASKS[$task_id]}"

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  $task_id [$agent]"
    echo "  ${prompt:0:70}..."
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Create isolated workspace
    local workspace="$RESULTS_DIR/${task_id}_${agent}_workspace"
    rm -rf "$workspace"
    mkdir -p "$workspace"

    # Copy project files for context (but not target/)
    rsync -a --exclude='target/' --exclude='competency_results/' \
        --exclude='benchmark_results/' --exclude='.git/' \
        "$PROJECT_DIR/" "$workspace/" 2>/dev/null || true

    local output_file="$RESULTS_DIR/${task_id}_${agent}.json"
    local start_time
    start_time=$(python3 -c "import time; print(time.time())")

    if [[ "$agent" == "deepagent" ]]; then
        cd "$workspace"
        timeout 300 cargo run --manifest-path "$PROJECT_DIR/Cargo.toml" \
            --release -q -- --json -p "$prompt" > "$output_file" 2>/dev/null || {
            echo '{"result":"(timeout or error)","metrics":{"elapsed_ms":300000,"turns":0,"tool_calls":0},"files_changed":[]}' > "$output_file"
        }
        cd "$PROJECT_DIR"
    elif [[ "$agent" == "claude" ]]; then
        if ! command -v claude &>/dev/null; then
            echo "  [SKIP] claude not found"
            echo '{"result":"(claude not available)"}' > "$output_file"
            return
        fi
        cd "$workspace"
        timeout 300 claude -p "$prompt" > "$RESULTS_DIR/${task_id}_claude.txt" 2>/dev/null || true
        cd "$PROJECT_DIR"
        # Wrap text output in JSON
        local text
        text=$(cat "$RESULTS_DIR/${task_id}_claude.txt" 2>/dev/null || echo "(error)")
        local end_time
        end_time=$(python3 -c "import time; print(time.time())")
        local elapsed
        elapsed=$(python3 -c "print(int(($end_time - $start_time) * 1000))")
        python3 -c "
import json, sys
text = open('$RESULTS_DIR/${task_id}_claude.txt').read()
json.dump({'result': text, 'metrics': {'elapsed_ms': $elapsed}}, open('$output_file', 'w'), indent=2)
" 2>/dev/null || true
    fi

    # Report results
    local end_time
    end_time=$(python3 -c "import time; print(time.time())")
    local elapsed_s
    elapsed_s=$(python3 -c "print(f'{$end_time - $start_time:.1f}')")

    if [[ -f "$output_file" ]]; then
        local metrics
        metrics=$(python3 -c "
import json
d = json.load(open('$output_file'))
m = d.get('metrics', {})
fc = d.get('files_changed', [])
print(f\"Time: {m.get('elapsed_ms', 0)/1000:.1f}s | Turns: {m.get('turns', '?')} | Tools: {m.get('tool_calls', '?')} | Files: {len(fc)}\")
" 2>/dev/null || echo "Time: ${elapsed_s}s")
        echo "  $metrics"
    fi

    # List created files in workspace
    local new_files
    new_files=$(cd "$workspace" && find . -newer "$output_file" -type f 2>/dev/null | head -10 || true)
    if [[ -n "$new_files" ]]; then
        echo "  New files:"
        echo "$new_files" | sed 's/^/    /'
    fi

    # Save to summary
    echo "$task_id,$agent,$elapsed_s" >> "$RESULTS_DIR/competency_summary.csv"
}

# Header
echo "task,agent,time_seconds" > "$RESULTS_DIR/competency_summary.csv"
echo "╔════════════════════════════════════════════╗"
echo "║  deepagent Competency Benchmark Suite      ║"
echo "║  $(date -u +%Y-%m-%dT%H:%M:%SZ)                       ║"
echo "╚════════════════════════════════════════════╝"

if [[ -n "$SINGLE_TASK" ]]; then
    task_ids=("$SINGLE_TASK")
else
    task_ids=(C1 C2 C3 C4 C5 C6 C7 C8 C9 C10)
fi

for t in "${task_ids[@]}"; do
    run_task "$t" "deepagent"
    if $COMPARE; then
        run_task "$t" "claude"
    fi
done

echo ""
echo "╔════════════════════════════════════════════╗"
echo "║  Results: $RESULTS_DIR/"
echo "║  Review: cat competency_results/C1_deepagent.json | jq"
echo "╚════════════════════════════════════════════╝"

# Print TODO for next cycle
echo ""
echo "=== NEXT CYCLE TODO ==="
echo "1. Score each task output (0-5 per criterion)"
echo "2. Identify weakest task"
echo "3. Add feature or improve prompt to address weakness"
echo "4. Re-run that task to verify improvement"
echo "5. Tag new release with improvements"
