#!/bin/bash

# Comprehensive test script for Clean Language compiler
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

PASSED=0
FAILED=0
FAILED_FILES=()

echo "Starting comprehensive compilation test..."

# Create output directory if it doesn't exist
mkdir -p tests/wasm

# Test each .cln file
for file in tests/clean_files/*.cln; do
    basename=$(basename "$file" .cln)
    output_file="tests/wasm/${basename}.wasm"
    
    echo -n "Testing $basename... "
    
    # Run compilation and capture output
    if cargo run --bin clean-language-compiler compile -i "$file" -o "$output_file" >/dev/null 2>&1; then
        echo "✓ PASS"
        PASSED=$((PASSED + 1))
    else
        echo "✗ FAIL"
        FAILED=$((FAILED + 1))
        FAILED_FILES+=("$basename")
    fi
done

echo ""
echo "=== RESULTS ==="
echo "Passed: $PASSED"
echo "Failed: $FAILED"
echo "Total: $((PASSED + FAILED))"
echo "Success Rate: $(echo "scale=1; $PASSED * 100 / ($PASSED + $FAILED)" | bc)%"

if [ $FAILED -gt 0 ]; then
    echo ""
    echo "=== FAILED FILES ==="
    for file in "${FAILED_FILES[@]}"; do
        echo "- $file"
    done
fi