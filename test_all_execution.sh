#!/bin/bash
# Test execution of ALL compiled WASM files

set -e

cd "$(dirname "$0")"

echo "=================================================="
echo "  Clean Language Compiler - EXECUTION TESTING"
echo "=================================================="
echo ""

# Counters
total=0
passed=0
failed=0

# Temporary files for tracking
failed_exec=$(mktemp)
passed_exec=$(mktemp)

trap "rm -f $failed_exec $passed_exec" EXIT

echo "Testing execution of ALL compiled WASM files..."
echo ""

# Find all .wasm files and execute them
while IFS= read -r wasm_file; do
    ((total++))

    filename=$(basename "$wasm_file" .wasm)

    echo -n "[$total] Executing $filename.wasm... "

    # Execute and capture result
    if ./target/release/wasmtime_runner "$wasm_file" > /dev/null 2>&1; then
        echo "✅"
        ((passed++))
        echo "$filename" >> "$passed_exec"
    else
        echo "❌"
        ((failed++))
        echo "$filename" >> "$failed_exec"
    fi
done < <(find tests/output -name "*.wasm" -type f | sort)

echo ""
echo "=================================================="
echo "  EXECUTION RESULTS"
echo "=================================================="
echo "Total WASM files:  $total"
echo "✅ Passed:          $passed ($(echo "scale=1; $passed * 100 / $total" | bc)%)"
echo "❌ Failed:          $failed ($(echo "scale=1; $failed * 100 / $total" | bc)%)"
echo "=================================================="
echo ""

if [ $failed -gt 0 ]; then
    echo "Failed executions:"
    echo "---"
    cat "$failed_exec"
    echo ""
fi

exit 0
