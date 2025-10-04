#!/bin/bash

# Comprehensive Clean Language Runtime Test
# Executes all compiled WASM files and captures runtime results

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT_DIR="$SCRIPT_DIR/tests/output"
LOGS_DIR="$SCRIPT_DIR/tests/logs"
RESULTS_DIR="$SCRIPT_DIR/tests/results"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Clean Language Comprehensive Runtime Test ===${NC}"
echo "WASM files: $OUTPUT_DIR"
echo "Logs directory: $LOGS_DIR"
echo ""

# Initialize counters and arrays
total_files=0
successful_runs=0
failed_runs=0
declare -a runtime_failures=()
declare -a memory_errors=()
declare -a execution_errors=()
declare -a timeout_errors=()

# Create results summary file
SUMMARY_FILE="$RESULTS_DIR/runtime_summary_$(date +%Y%m%d_%H%M%S).txt"
echo "Clean Language Comprehensive Runtime Test" > "$SUMMARY_FILE"
echo "Started: $(date)" >> "$SUMMARY_FILE"
echo "=========================================" >> "$SUMMARY_FILE"
echo "" >> "$SUMMARY_FILE"

# Function to categorize runtime error type
categorize_runtime_error() {
    local error_log="$1"
    if grep -q -E "(memory|bounds|alignment|segmentation)" "$error_log"; then
        return 1  # Memory error
    elif grep -q -E "(timeout|killed|SIGTERM)" "$error_log"; then
        return 2  # Timeout error
    elif grep -q -E "(trap|unreachable|invalid)" "$error_log"; then
        return 3  # Execution error
    else
        return 0  # Other/unknown error
    fi
}

echo "Processing WASM files..."
echo ""

# Test each .wasm file
for wasm_file in "$OUTPUT_DIR"/*.wasm; do
    if [ ! -f "$wasm_file" ]; then
        echo "No WASM files found to test."
        exit 0
    fi
    
    filename=$(basename "$wasm_file")
    filename_no_ext="${filename%.*}"
    runtime_log="$LOGS_DIR/${filename_no_ext}_runtime.log"
    runtime_output="$LOGS_DIR/${filename_no_ext}_output.txt"
    
    total_files=$((total_files + 1))
    
    echo -n "[$total_files] Running: $filename... "
    
    # Execute the WASM file with timeout
    if timeout 10s cargo run --release --bin wasmtime_runner -- "$wasm_file" >"$runtime_output" 2>"$runtime_log"; then
        echo -e "${GREEN}✓ SUCCESS${NC}"
        successful_runs=$((successful_runs + 1))
        echo "SUCCESS: $filename" >> "$SUMMARY_FILE"
        
        # Capture output if any
        if [ -s "$runtime_output" ]; then
            echo "  Output:" >> "$SUMMARY_FILE"
            head -5 "$runtime_output" | sed 's/^/    /' >> "$SUMMARY_FILE"
            echo "" >> "$SUMMARY_FILE"
        fi
    else
        echo -e "${RED}✗ FAILED${NC}"
        failed_runs=$((failed_runs + 1))
        runtime_failures+=("$filename")
        
        # Categorize the runtime error
        categorize_runtime_error "$runtime_log"
        error_type=$?
        
        case $error_type in
            1)
                memory_errors+=("$filename")
                echo "MEMORY ERROR: $filename" >> "$SUMMARY_FILE"
                ;;
            2)
                timeout_errors+=("$filename")
                echo "TIMEOUT ERROR: $filename" >> "$SUMMARY_FILE"
                ;;
            3)
                execution_errors+=("$filename")
                echo "EXECUTION ERROR: $filename" >> "$SUMMARY_FILE"
                ;;
            *)
                echo "OTHER RUNTIME ERROR: $filename" >> "$SUMMARY_FILE"
                ;;
        esac
        
        # Add first few lines of error to summary
        echo "  Error details:" >> "$SUMMARY_FILE"
        head -3 "$runtime_log" | sed 's/^/    /' >> "$SUMMARY_FILE"
        echo "" >> "$SUMMARY_FILE"
    fi
done

if [ $total_files -eq 0 ]; then
    echo "No WASM files found to test."
    echo "Please run the compilation test first."
    exit 1
fi

# Calculate statistics
success_rate=$((successful_runs * 100 / total_files))

echo ""
echo -e "${BLUE}=== RUNTIME RESULTS ===${NC}"
echo -e "Total WASM files tested: ${YELLOW}$total_files${NC}"
echo -e "Successful runs: ${GREEN}$successful_runs${NC}"
echo -e "Failed runs: ${RED}$failed_runs${NC}"
echo -e "Runtime success rate: ${YELLOW}${success_rate}%${NC}"

echo ""
echo -e "${BLUE}=== RUNTIME ERROR BREAKDOWN ===${NC}"
echo -e "Memory errors: ${RED}${#memory_errors[@]}${NC}"
echo -e "Execution errors: ${RED}${#execution_errors[@]}${NC}"
echo -e "Timeout errors: ${RED}${#timeout_errors[@]}${NC}"

# Write summary to file
echo "" >> "$SUMMARY_FILE"
echo "RUNTIME STATISTICS:" >> "$SUMMARY_FILE"
echo "==================" >> "$SUMMARY_FILE"
echo "Total WASM files: $total_files" >> "$SUMMARY_FILE"
echo "Successful runs: $successful_runs" >> "$SUMMARY_FILE"
echo "Failed runs: $failed_runs" >> "$SUMMARY_FILE"
echo "Runtime success rate: ${success_rate}%" >> "$SUMMARY_FILE"
echo "" >> "$SUMMARY_FILE"
echo "RUNTIME ERROR BREAKDOWN:" >> "$SUMMARY_FILE"
echo "Memory errors: ${#memory_errors[@]}" >> "$SUMMARY_FILE"
echo "Execution errors: ${#execution_errors[@]}" >> "$SUMMARY_FILE"
echo "Timeout errors: ${#timeout_errors[@]}" >> "$SUMMARY_FILE"
echo "" >> "$SUMMARY_FILE"
echo "Completed: $(date)" >> "$SUMMARY_FILE"

if [ $failed_runs -gt 0 ]; then
    echo ""
    echo -e "${YELLOW}Failed runtime details saved to: $SUMMARY_FILE${NC}"
    echo -e "${YELLOW}Individual runtime logs in: $LOGS_DIR${NC}"
fi

echo ""
echo -e "${BLUE}Runtime test completed!${NC}"

# Set exit code based on results
if [ $failed_runs -eq 0 ]; then
    exit 0
else
    exit 1
fi