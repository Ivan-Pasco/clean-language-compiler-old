#!/bin/bash

# Sample Test Runner - Tests a few files from each category
# Much faster than full suite for initial error discovery

COMPILER="./target/debug/clean-language-compiler"
RUNNER="./target/debug/wasmtime_runner"

# Build first if needed
if [ ! -f "$COMPILER" ]; then
    echo "Building compiler..."
    cargo build --bin clean-language-compiler --bin wasmtime_runner
fi

mkdir -p tests/output

declare -A results
total=0
passed=0

echo "========================================="
echo "Sample Test Run - Testing Representative Files"
echo "========================================="
echo ""

# Function to test a file
test_file() {
    local file=$1
    local category=$2
    total=$((total + 1))

    local wasm_file="tests/output/$(basename ${file%.cln}).wasm"

    # Compile
    if $COMPILER compile -i "$file" -o "$wasm_file" 2>&1 | grep -q "Successfully compiled"; then
        # Execute
        if timeout 2 $RUNNER "$wasm_file" 2>&1 | grep -q "✅ Execution completed successfully"; then
            echo "✅ [$category] $(basename $file)"
            passed=$((passed + 1))
            results["$category"]=$((${results["$category"]:-0} + 1))
        else
            echo "❌ [$category] $(basename $file) - EXEC FAILED"
            results["${category}_failed"]=$((${results["${category}_failed"]:-0} + 1))
        fi
    else
        echo "❌ [$category] $(basename $file) - COMPILE FAILED"
        results["${category}_failed"]=$((${results["${category}_failed"]:-0} + 1))
    fi
}

# Test core/basics (2 files)
test_file "tests/cln/core/basics/00_minimal.cln" "core/basics"
test_file "tests/cln/core/basics/95_apply_blocks_specification.cln" "core/basics"

# Test core/types (2 files)
for f in $(find tests/cln/core/types -name "*.cln" | head -2); do
    test_file "$f" "core/types"
done

# Test core/variables (2 files)
for f in $(find tests/cln/core/variables -name "*.cln" | head -2); do
    test_file "$f" "core/variables"
done

# Test functions (3 files)
for f in $(find tests/cln/functions -name "*.cln" | head -3); do
    test_file "$f" "functions"
done

# Test control flow (3 files)
for f in $(find tests/cln/control -name "*.cln" | head -3); do
    test_file "$f" "control"
done

# Test language features (3 files)
for f in $(find tests/cln/language -name "*.cln" | head -3); do
    test_file "$f" "language"
done

# Test stdlib (2 files)
for f in $(find tests/cln/stdlib -name "*.cln" | head -2); do
    test_file "$f" "stdlib"
done

echo ""
echo "========================================="
echo "Sample Test Summary"
echo "========================================="
echo "Total Tested: $total"
echo "Passed: $passed"
echo "Failed: $((total - passed))"
echo "Success Rate: $(awk "BEGIN {printf \"%.1f\", ($passed/$total)*100}")%"
echo ""
echo "Category Breakdown:"
for key in "${!results[@]}"; do
    echo "  $key: ${results[$key]}"
done
