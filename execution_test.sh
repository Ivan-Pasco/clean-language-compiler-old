#!/bin/bash

# Test actual execution success rate using wasmtime_runner

set -e

COMPILER_DIR="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"
OUTPUT_DIR="$COMPILER_DIR/tests/output"
RESULTS_DIR="$COMPILER_DIR/qa_results"

cd "$COMPILER_DIR"

# Read successful compilations and test execution
EXECUTION_SUCCESSES=0
EXECUTION_FAILURES=0
TOTAL_TESTS=0

echo "Testing execution of successfully compiled WASM files..."
echo

while IFS= read -r filename; do
    if [[ -n "$filename" ]]; then
        wasm_file="$OUTPUT_DIR/${filename}.wasm"
        if [[ -f "$wasm_file" ]]; then
            ((TOTAL_TESTS++))
            echo -n "Testing execution: $filename... "
            
            if cargo run --quiet --bin wasmtime_runner -- "$wasm_file" >/dev/null 2>&1; then
                echo "✅ SUCCESS"
                ((EXECUTION_SUCCESSES++))
            else
                echo "❌ FAILED"
                ((EXECUTION_FAILURES++))
            fi
        fi
    fi
done < "$RESULTS_DIR/compilation_successes.txt"

# Calculate execution success rate
EXECUTION_PERCENTAGE=$(awk "BEGIN {printf \"%.2f\", $EXECUTION_SUCCESSES/$TOTAL_TESTS*100}")

echo
echo "=========================================="
echo "EXECUTION TEST RESULTS"
echo "=========================================="
echo "Total compiled files tested: $TOTAL_TESTS"
echo "Execution successes: $EXECUTION_SUCCESSES ($EXECUTION_PERCENTAGE%)"
echo "Execution failures: $EXECUTION_FAILURES"
echo "=========================================="