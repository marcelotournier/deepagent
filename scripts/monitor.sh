#!/bin/bash

# Default values
INTERVAL=5
OUTPUT="monitor_log.csv"
CPU_THRESHOLD=80
MEM_THRESHOLD=90

# Function to display help
show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo "Options:"
    echo "  --interval SECONDS  Sampling interval (default: 5)"
    echo "  --output FILE       CSV output file (default: monitor_log.csv)"
    echo "  --help              Show this help message"
}

# Parse arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --interval)
            if [[ -n "$2" && "$2" =~ ^[0-9]+$ ]]; then
                INTERVAL="$2"
                shift
            else
                echo "Error: --interval requires a numeric argument." >&2
                exit 1
            fi
            ;;
        --output)
            if [[ -n "$2" ]]; then
                OUTPUT="$2"
                shift
            else
                echo "Error: --output requires a file path." >&2
                exit 1
            fi
            ;;
        --help)
            show_help
            exit 0
            ;;
        *)
            echo "Unknown parameter: $1" >&2
            show_help
            exit 1
            ;;
    esac
    shift
done

# Check for required tools
for tool in bc awk; do
    if ! command -v $tool &> /dev/null; then
        echo "Error: $tool is required but not installed." >&2
        exit 1
    fi
done

# Initialize CSV
if [ ! -f "$OUTPUT" ]; then
    echo "Timestamp,CPU%,Memory%" > "$OUTPUT"
fi

# Stats for summary
COUNT=0
SUM_CPU=0
SUM_MEM=0

cleanup() {
    echo -e "\n\n--- Monitoring Summary ---"
    if [ $COUNT -gt 0 ]; then
        # Use awk for division to avoid scale issues with bc if not careful
        AVG_CPU=$(awk "BEGIN {printf \"%.2f\", $SUM_CPU / $COUNT}")
        AVG_MEM=$(awk "BEGIN {printf \"%.2f\", $SUM_MEM / $COUNT}")
        echo "Total samples: $COUNT"
        echo "Average CPU: $AVG_CPU%"
        echo "Average Memory: $AVG_MEM%"
    else
        echo "No samples collected."
    fi
    exit 0
}

# Handle SIGINT (Ctrl+C)
trap cleanup SIGINT

echo "Monitoring started. Press Ctrl+C to stop."
echo "Logging to $OUTPUT every $INTERVAL seconds."

get_cpu_linux() {
    # top -bn1 can be unreliable for first sample, but it's a common approach.
    # Alternatively: /proc/stat
    top -bn1 | grep "Cpu(s)" | sed "s/.*, *\([0-9.]*\)%* id.*/\1/" | awk '{print 100 - $1}'
}

get_mem_linux() {
    free | grep Mem | awk '{print $3/$2 * 100.0}'
}

get_cpu_macos() {
    top -l 1 | grep "CPU usage" | awk '{print $3}' | sed 's/%//'
}

get_mem_macos() {
    # Get total memory in bytes
    TOTAL_MEM=$(sysctl -n hw.memsize)
    # Get page size
    PAGE_SIZE=$(vm_stat | grep "page size" | awk '{print $8}')
    # Get free pages
    FREE_PAGES=$(vm_stat | grep "Pages free" | awk '{print $3}' | sed 's/\.//')
    # Get speculative pages
    SPEC_PAGES=$(vm_stat | grep "Pages speculative" | awk '{print $3}' | sed 's/\.//')
    
    FREE_MEM=$(echo "($FREE_PAGES + $SPEC_PAGES) * $PAGE_SIZE" | bc)
    USED_MEM=$(echo "$TOTAL_MEM - $FREE_MEM" | bc)
    echo "scale=2; $USED_MEM / $TOTAL_MEM * 100" | bc
}

while true; do
    TIMESTAMP=$(date "+%Y-%m-%d %H:%M:%S")
    
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        CPU_USAGE=$(get_cpu_linux)
        MEM_USAGE=$(get_mem_linux)
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        CPU_USAGE=$(get_cpu_macos)
        MEM_USAGE=$(get_mem_macos)
    else
        echo "Unsupported OS: $OSTYPE" >&2
        exit 1
    fi

    # Fallback to 0 if empty
    CPU_USAGE=${CPU_USAGE:-0}
    MEM_USAGE=${MEM_USAGE:-0}

    # Log to CSV
    echo "$TIMESTAMP,$CPU_USAGE,$MEM_USAGE" >> "$OUTPUT"

    # Warnings to stderr
    # Use awk for float comparison
    IS_CPU_HIGH=$(awk "BEGIN {print ($CPU_USAGE > $CPU_THRESHOLD)}")
    IS_MEM_HIGH=$(awk "BEGIN {print ($MEM_USAGE > $MEM_THRESHOLD)}")

    if [ "$IS_CPU_HIGH" -eq 1 ]; then
        echo "[$TIMESTAMP] WARNING: CPU usage is high: $CPU_USAGE%" >&2
    fi
    if [ "$IS_MEM_HIGH" -eq 1 ]; then
        echo "[$TIMESTAMP] WARNING: Memory usage is high: $MEM_USAGE%" >&2
    fi

    # Update summary stats
    SUM_CPU=$(echo "$SUM_CPU + $CPU_USAGE" | bc)
    SUM_MEM=$(echo "$SUM_MEM + $MEM_USAGE" | bc)
    COUNT=$((COUNT + 1))

    sleep "$INTERVAL"
done
