#!/bin/bash

# Comprehensive Clean Language Test Runner
# Tests all .cln files in tests/clean_files/ directory

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
TEST_DIR="$PROJECT_DIR/tests/clean_files"
TEMP_DIR="$PROJECT_DIR/temp_wasm"

# Create temp directory for WASM files
mkdir -p "$TEMP_DIR"

echo "=== COMPREHENSIVE CLEAN LANGUAGE TEST RUNNER ==="
echo "Started: $(date)"
echo "Testing directory: $TEST_DIR"

# Build compiler first
echo "Building compiler..."
cd "$PROJECT_DIR"
cargo build --release >/dev/null 2>&1

# Initialize counters
TOTAL=0
PASSED=0
FAILED=0
COMPILE_FAILURES=0
RUNTIME_FAILURES=0

echo ""
echo "Running comprehensive tests..."

# Find all .cln files and test them
while IFS= read -r -d '' cln_file; do
    TOTAL=$((TOTAL + 1))
    
    # Extract filename without path and extension
    filename=$(basename "$cln_file" .cln)
    
    # Progress indicator
    if ((TOTAL % 50 == 0)); then
        echo "Progress: $TOTAL tests processed..."
    fi
    
    # Compile to WASM
    wasm_file="$TEMP_DIR/${filename}.wasm"
    if cargo run --release --bin clean-language-compiler compile -i "$cln_file" -o "$wasm_file" >/dev/null 2>&1; then
        # Compilation succeeded, try to run
        if cargo run --release --bin wasmtime_runner "$wasm_file" >/dev/null 2>&1; then
            # Runtime succeeded
            PASSED=$((PASSED + 1))
        else
            # Runtime failed
            RUNTIME_FAILURES=$((RUNTIME_FAILURES + 1))
            FAILED=$((FAILED + 1))
        fi
    else
        # Compilation failed
        COMPILE_FAILURES=$((COMPILE_FAILURES + 1))
        FAILED=$((FAILED + 1))
    fi
    
done < <(find "$TEST_DIR" -name "*.cln" -print0)

echo ""
echo "=== COMPREHENSIVE TEST RESULTS ==="
echo "Total tests: $TOTAL"
echo "Passed: $PASSED"
echo "Failed: $FAILED"
echo "  - Compilation failures: $COMPILE_FAILURES"
echo "  - Runtime failures: $RUNTIME_FAILURES"
echo ""

# Calculate success rate
if [ $TOTAL -gt 0 ]; then
    SUCCESS_RATE=$(echo "scale=2; $PASSED * 100 / $TOTAL" | bc -l)
    echo "Success rate: $SUCCESS_RATE% ($PASSED/$TOTAL)"
else
    echo "No tests found!"
fi

echo "Target: 100% ($TOTAL/$TOTAL)"
echo "Remaining to fix: $FAILED tests"
echo "Progress: $PASSED/$TOTAL tests passing"
echo "Completed: $(date)"

# Cleanup temp directory
rm -rf "$TEMP_DIR"

# Exit with status based on success rate
if [ $PASSED -eq $TOTAL ]; then
    exit 0
else
    exit 1
fi