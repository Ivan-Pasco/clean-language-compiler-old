#!/bin/bash

# Simple success rate counting script
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

TOTAL=0
PASSED=0
FAILED=0
FAILED_FILES=()

# Test each file quickly
for file in tests/clean_files/*.cln; do
    basename=$(basename "$file" .cln)
    output_file="tests/wasm/${basename}.wasm"
    
    TOTAL=$((TOTAL + 1))
    
    # Quick compilation test (no output)
    if cargo run --bin clean-language-compiler compile -i "$file" -o "$output_file" &>/dev/null; then
        PASSED=$((PASSED + 1))
    else
        FAILED=$((FAILED + 1))
        FAILED_FILES+=("$basename")
    fi
    
    # Show progress every 10 files
    if [ $((TOTAL % 10)) -eq 0 ]; then
        echo "Progress: $TOTAL/$PASSED files processed/passed"
    fi
done

echo ""
echo "=== FINAL RESULTS ==="
echo "Total files: $TOTAL"
echo "Passed: $PASSED"
echo "Failed: $FAILED"
SUCCESS_RATE=$(echo "scale=1; $PASSED * 100 / $TOTAL" | bc)
echo "Success Rate: ${SUCCESS_RATE}%"

echo ""
echo "=== FAILED FILES (first 20) ==="
for i in $(seq 0 $((${#FAILED_FILES[@]} < 20 ? ${#FAILED_FILES[@]} - 1 : 19))); do
    echo "- ${FAILED_FILES[$i]}"
done

if [ ${#FAILED_FILES[@]} -gt 20 ]; then
    echo "... and $((${#FAILED_FILES[@]} - 20)) more"
fi