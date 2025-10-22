#!/bin/bash

# Comprehensive test script for all .cln files
# Tracks success/failure rates and error patterns

SUCCESS_COUNT=0
FAIL_COUNT=0
TOTAL_COUNT=0

FAILED_FILES=()
ERROR_PATTERNS=()

echo "🧪 Clean Language Compiler - Comprehensive Test Suite"
echo "======================================================"
echo ""

# Create output directory
mkdir -p tests/output

# Find all .cln files
CLN_FILES=$(find tests/cln -name "*.cln" | sort)
TOTAL_COUNT=$(echo "$CLN_FILES" | wc -l | tr -d ' ')

echo "📊 Found $TOTAL_COUNT test files"
echo ""

# Compile each file
for file in $CLN_FILES; do
    filename=$(basename "$file" .cln)
    output="tests/output/${filename}.wasm"

    # Attempt compilation
    if cargo run --bin clean-language-compiler compile -i "$file" -o "$output" 2>&1 | grep -q "Successfully compiled"; then
        SUCCESS_COUNT=$((SUCCESS_COUNT + 1))
        echo "✅ $(basename "$file")"
    else
        FAIL_COUNT=$((FAIL_COUNT + 1))
        FAILED_FILES+=("$file")
        echo "❌ $(basename "$file")"

        # Capture error for pattern analysis
        ERROR=$(cargo run --bin clean-language-compiler compile -i "$file" -o "$output" 2>&1 | head -5)
        ERROR_PATTERNS+=("$ERROR")
    fi
done

echo ""
echo "======================================================"
echo "📈 RESULTS SUMMARY"
echo "======================================================"
echo "Total Tests:    $TOTAL_COUNT"
echo "✅ Passed:      $SUCCESS_COUNT"
echo "❌ Failed:      $FAIL_COUNT"

if [ $TOTAL_COUNT -gt 0 ]; then
    SUCCESS_RATE=$((SUCCESS_COUNT * 100 / TOTAL_COUNT))
    echo "📊 Success Rate: ${SUCCESS_RATE}%"
fi

echo ""

if [ $FAIL_COUNT -gt 0 ]; then
    echo "======================================================"
    echo "❌ FAILED FILES"
    echo "======================================================"
    for failed in "${FAILED_FILES[@]}"; do
        echo "  - $failed"
    done
fi

exit 0
