#!/bin/bash

# Categorize WASM Validation Errors
# Analyzes all failing tests and groups by error type

set -euo pipefail

COMPILER="./target/debug/clean-language-compiler"
RESULTS_JSON="tests/results/true_comprehensive_20251007_154345.json"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║          WASM VALIDATION ERROR CATEGORIZATION              ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Get all invalid_wasm files
INVALID_FILES=$(cat "$RESULTS_JSON" | grep '"result": "invalid_wasm"' | sed 's/.*"file": "\(.*\)", "result".*/\1/')

TOTAL_INVALID=$(echo "$INVALID_FILES" | wc -l | tr -d ' ')
echo "Analyzing $TOTAL_INVALID files with invalid WASM..."
echo ""

# Sample 20 random files to analyze
echo "Sampling 20 random failing files..."
SAMPLE_FILES=$(echo "$INVALID_FILES" | gshuf -n 20 2>/dev/null || echo "$INVALID_FILES" | head -20)

# Count variables
CALL_MISMATCH=0
LOCAL_SET_MISMATCH=0
RETURN_MISMATCH=0
END_FUNCTION_MISMATCH=0
I64_ADD_MISMATCH=0
I32_CONV_MISMATCH=0
OTHER_MISMATCH=0

for FILE in $SAMPLE_FILES; do
    FULL_PATH=$(find tests/cln -name "$FILE" -type f | head -1)
    if [ -z "$FULL_PATH" ]; then
        continue
    fi

    WASM_FILE="/tmp/$(basename ${FILE%.cln}).wasm"

    # Compile and get validation errors
    $COMPILER compile -i "$FULL_PATH" -o "$WASM_FILE" >/dev/null 2>&1 || true
    ERRORS=$(wasm-validate "$WASM_FILE" 2>&1 || true)

    # Count each error type
    if echo "$ERRORS" | grep -q "type mismatch in call"; then
        CALL_MISMATCH=$((CALL_MISMATCH + 1))
    fi
    if echo "$ERRORS" | grep -q "type mismatch in local.set"; then
        LOCAL_SET_MISMATCH=$((LOCAL_SET_MISMATCH + 1))
    fi
    if echo "$ERRORS" | grep -q "type mismatch in return"; then
        RETURN_MISMATCH=$((RETURN_MISMATCH + 1))
    fi
    if echo "$ERRORS" | grep -q "type mismatch at end of function"; then
        END_FUNCTION_MISMATCH=$((END_FUNCTION_MISMATCH + 1))
    fi
    if echo "$ERRORS" | grep -q "type mismatch in i64.add"; then
        I64_ADD_MISMATCH=$((I64_ADD_MISMATCH + 1))
    fi
    if echo "$ERRORS" | grep -q "type mismatch in i32"; then
        I32_CONV_MISMATCH=$((I32_CONV_MISMATCH + 1))
    fi
done

echo "═══════════════════════════════════════════════════════════"
echo "  ERROR FREQUENCY (from 20 sample files)"
echo "═══════════════════════════════════════════════════════════"
echo ""
printf "%-40s: %d files (%.0f%%)\n" "Function call type mismatches" $CALL_MISMATCH $((CALL_MISMATCH * 100 / 20))
printf "%-40s: %d files (%.0f%%)\n" "local.set type mismatches" $LOCAL_SET_MISMATCH $((LOCAL_SET_MISMATCH * 100 / 20))
printf "%-40s: %d files (%.0f%%)\n" "Return type mismatches" $RETURN_MISMATCH $((RETURN_MISMATCH * 100 / 20))
printf "%-40s: %d files (%.0f%%)\n" "End-of-function mismatches" $END_FUNCTION_MISMATCH $((END_FUNCTION_MISMATCH * 100 / 20))
printf "%-40s: %d files (%.0f%%)\n" "i64.add type mismatches" $I64_ADD_MISMATCH $((I64_ADD_MISMATCH * 100 / 20))
printf "%-40s: %d files (%.0f%%)\n" "i32 conversion mismatches" $I32_CONV_MISMATCH $((I32_CONV_MISMATCH * 100 / 20))

echo ""
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "Detailed error example from a failing file:"
echo ""

# Show detailed errors from first file
FIRST_FILE=$(echo "$SAMPLE_FILES" | head -1)
FULL_PATH=$(find tests/cln -name "$FIRST_FILE" -type f | head -1)
WASM_FILE="/tmp/$(basename ${FIRST_FILE%.cln}).wasm"
$COMPILER compile -i "$FULL_PATH" -o "$WASM_FILE" >/dev/null 2>&1 || true

echo "File: $FIRST_FILE"
echo "Path: $FULL_PATH"
echo "---"
wasm-validate "$WASM_FILE" 2>&1 | head -20 || true
