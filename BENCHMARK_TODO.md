# Benchmark Improvement Cycle

This file tracks the TODO cycle for competency benchmark improvements.
The loop picks up the next unchecked item each cycle.

## Current Cycle

- [ ] Run competency benchmark C1 (calculator app) — needs GEMINI_API_KEY
- [ ] Run competency benchmark C2 (architecture docs)
- [ ] Run competency benchmark C3 (monitor script)
- [ ] Run competency benchmark C4 (error handling refactor)
- [ ] Run competency benchmark C5 (fuzzy search module)
- [ ] Run competency benchmark C6 (security audit)
- [ ] Run competency benchmark C7 (CI workflow)
- [ ] Run competency benchmark C8 (bug investigation)
- [ ] Run competency benchmark C9 (REST API server)
- [ ] Run competency benchmark C10 (performance report)

## After Each Run

1. Score output (correctness, completeness, quality, efficiency: 0-5 each)
2. If score < 3 on any criterion, identify the root cause:
   - Bad tool selection? → Improve system prompt examples
   - Too many turns? → Add smarter tool chaining
   - Missing capability? → Add new CLI feature
   - Poor code quality? → Add code style instructions to prompt
3. Implement fix
4. Re-run that task
5. Tag release if improved
6. Move to next task

## Completed Cycles

(none yet — waiting for GEMINI_API_KEY to run first benchmarks)
