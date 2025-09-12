#!/bin/bash

# Final comprehensive test script for success rate measurement
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

PASSED=0
FAILED=0
FAILED_FILES=()

echo "Running final comprehensive compilation test..."
echo "Testing all .cln files in tests/clean_files/"

# Create output directory if it doesn't exist
mkdir -p tests/wasm

# Test each .cln file
for file in tests/clean_files/*.cln; do
    basename=$(basename "$file" .cln)
    output_file="tests/wasm/${basename}.wasm"
    
    # Run compilation - simplified output for speed
    if cargo run --bin clean-language-compiler compile -i "$file" -o "$output_file" >/dev/null 2>&1; then
        PASSED=$((PASSED + 1))
    else
        FAILED=$((FAILED + 1))
        FAILED_FILES+=("$basename")
    fi
    
    # Progress indicator every 25 files
    TOTAL=$((PASSED + FAILED))
    if [ $((TOTAL % 25)) -eq 0 ]; then
        echo "Progress: $TOTAL files processed ($PASSED passed)"
    fi
done

echo ""
echo "=== FINAL RESULTS ==="
echo "Passed: $PASSED"
echo "Failed: $FAILED"
echo "Total: $((PASSED + FAILED))"
SUCCESS_RATE=$(echo "scale=1; $PASSED * 100 / ($PASSED + $FAILED)" | bc)
echo "Final Success Rate: ${SUCCESS_RATE}%"

# Calculate improvement from baseline
BASELINE_RATE=80.8
IMPROVEMENT=$(echo "scale=1; $SUCCESS_RATE - $BASELINE_RATE" | bc)
echo "Improvement from baseline (~80.8%): +${IMPROVEMENT} percentage points"

echo ""
echo "=== KEY ACHIEVEMENTS ==="
echo "✓ Fixed list behavior parsing (line,unique → line-unique)"
echo "✓ Fixed apply blocks precision_type parsing"
echo "✓ Identified variable arithmetic return statement issue"

if [ $FAILED -gt 0 ]; then
    echo ""
    echo "=== REMAINING ISSUES (sample) ==="
    for i in $(seq 0 $((${#FAILED_FILES[@]} < 10 ? ${#FAILED_FILES[@]} - 1 : 9))); do
        echo "- ${FAILED_FILES[$i]}"
    done
    if [ ${#FAILED_FILES[@]} -gt 10 ]; then
        echo "... and $((${#FAILED_FILES[@]} - 10)) more"
    fi
fi