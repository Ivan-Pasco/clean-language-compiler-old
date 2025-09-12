#!/bin/bash
echo "=== Clean Language Compiler - Comprehensive QA Test Suite ==="
echo "Testing 274 Clean Language files..."
echo ""

TOTAL=0
SUCCESS=0
FAILED=0
FAILED_FILES=()

for file in tests/clean_files/*.cln; do
    if [[ -f "$file" ]]; then
        TOTAL=$((TOTAL + 1))
        filename=$(basename "$file")
        
        if cargo run --bin clean-language-compiler compile -i "$file" -o "temp_test_${filename}.wasm" >/dev/null 2>&1; then
            SUCCESS=$((SUCCESS + 1))
            echo "✅ $filename"
        else
            FAILED=$((FAILED + 1))
            FAILED_FILES+=("$filename")
            echo "❌ $filename"
        fi
        
        # Clean up temp file
        rm -f "temp_test_${filename}.wasm" 2>/dev/null
    fi
done

echo ""
echo "=== QA RESULTS SUMMARY ==="
echo "Total Files Tested: $TOTAL"
echo "Successful Compilations: $SUCCESS"
echo "Failed Compilations: $FAILED"
echo "Success Rate: $(echo "scale=1; $SUCCESS * 100 / $TOTAL" | bc -l)%"

if [[ $FAILED -gt 0 ]]; then
    echo ""
    echo "=== FAILED FILES ==="
    for failed_file in "${FAILED_FILES[@]}"; do
        echo "- $failed_file"
    done
fi

echo ""
echo "=== QA Test Complete ==="
