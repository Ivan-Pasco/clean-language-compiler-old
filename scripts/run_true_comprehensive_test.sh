#!/bin/bash

# TRUE Comprehensive Test Runner
# Tests ALL .cln files in tests/cln/ with WASM validation
# NO OPTIMISTIC LIES - Brutally honest reporting only

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Counters
TOTAL=0
COMPILED=0
WASM_VALID=0
COMPILE_FAILED=0
WASM_INVALID=0

# Output
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_DIR="tests/results"
mkdir -p "$RESULTS_DIR"
RESULTS_FILE="$RESULTS_DIR/true_comprehensive_${TIMESTAMP}.json"
OUTPUT_DIR="tests/output"
mkdir -p "$OUTPUT_DIR"

echo -e "${BLUE}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║   TRUE COMPREHENSIVE TEST - NO LIES, ONLY FACTS          ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""

# Build compiler first
echo -e "${YELLOW}Building compiler...${NC}"
cargo build --quiet 2>&1 || {
    echo -e "${RED}FATAL: Compiler build failed${NC}"
    exit 1
}
echo -e "${GREEN}✓ Compiler built${NC}"
echo ""

COMPILER="./target/debug/cln"

# Find all test files
echo -e "${YELLOW}Scanning for test files...${NC}"
TEST_FILES=$(find tests/cln -name "*.cln" -type f -not -path "*/future/*" | sort)
TOTAL=$(echo "$TEST_FILES" | wc -l | tr -d ' ')

echo -e "${BLUE}Found ${TOTAL} test files${NC}"
echo ""

# Results array
echo "{" > "$RESULTS_FILE"
echo "  \"timestamp\": \"$TIMESTAMP\"," >> "$RESULTS_FILE"
echo "  \"total_files\": $TOTAL," >> "$RESULTS_FILE"
echo "  \"results\": [" >> "$RESULTS_FILE"

FIRST=true

# Test each file
while IFS= read -r TEST_FILE; do
    FILENAME=$(basename "$TEST_FILE")
    BASENAME="${FILENAME%.cln}"
    WASM_FILE="$OUTPUT_DIR/${BASENAME}.wasm"

    printf "[%3d/%3d] %-50s " "$((COMPILED + COMPILE_FAILED + 1))" "$TOTAL" "$FILENAME"

    # Check for expected-error annotation: "// Expected: compile error ERRCODE"
    EXPECTED_ERROR=$(grep -m1 '^// Expected: compile error ' "$TEST_FILE" 2>/dev/null || true)
    EXPECTED_ERROR=$(echo "$EXPECTED_ERROR" | sed 's|// Expected: compile error ||' | awk '{print $1}')

    # Compile (guard against set -e exiting when compiler returns non-zero)
    COMPILE_OUTPUT=$($COMPILER compile "$TEST_FILE" -o "$WASM_FILE" 2>&1) && COMPILE_EXIT=0 || COMPILE_EXIT=$?

    if [ $COMPILE_EXIT -eq 0 ]; then
        COMPILED=$((COMPILED + 1))

        # Validate WASM
        if wasm-validate "$WASM_FILE" >/dev/null 2>&1; then
            WASM_VALID=$((WASM_VALID + 1))
            echo -e "${GREEN}✓ PASS${NC}"
            RESULT="pass"
        else
            WASM_INVALID=$((WASM_INVALID + 1))
            echo -e "${YELLOW}⚠ INVALID_WASM${NC}"
            RESULT="invalid_wasm"
        fi
    elif [ -n "$EXPECTED_ERROR" ]; then
        # This test is expected to fail with a specific error code
        if echo "$COMPILE_OUTPUT" | grep -q "\[$EXPECTED_ERROR\]\|error\[$EXPECTED_ERROR\]"; then
            # Correct error emitted — count as pass
            WASM_VALID=$((WASM_VALID + 1))
            echo -e "${GREEN}✓ PASS (expected error: $EXPECTED_ERROR)${NC}"
            RESULT="pass"
        else
            COMPILE_FAILED=$((COMPILE_FAILED + 1))
            echo -e "${RED}✗ WRONG_ERROR (expected $EXPECTED_ERROR)${NC}"
            RESULT="compile_failed"
        fi
    else
        COMPILE_FAILED=$((COMPILE_FAILED + 1))
        echo -e "${RED}✗ COMPILE_FAILED${NC}"
        RESULT="compile_failed"
    fi

    # Write JSON result
    if [ "$FIRST" = true ]; then
        FIRST=false
    else
        echo "," >> "$RESULTS_FILE"
    fi

    echo -n "    {\"file\": \"$FILENAME\", \"result\": \"$RESULT\"}" >> "$RESULTS_FILE"

done <<< "$TEST_FILES"

# Close JSON
echo "" >> "$RESULTS_FILE"
echo "  ]," >> "$RESULTS_FILE"
echo "  \"summary\": {" >> "$RESULTS_FILE"
echo "    \"total\": $TOTAL," >> "$RESULTS_FILE"
echo "    \"compiled\": $COMPILED," >> "$RESULTS_FILE"
echo "    \"wasm_valid\": $WASM_VALID," >> "$RESULTS_FILE"
echo "    \"compile_failed\": $COMPILE_FAILED," >> "$RESULTS_FILE"
echo "    \"wasm_invalid\": $WASM_INVALID" >> "$RESULTS_FILE"
echo "  }" >> "$RESULTS_FILE"
echo "}" >> "$RESULTS_FILE"

# Calculate percentages
if [ $TOTAL -gt 0 ]; then
    COMPILE_RATE=$((COMPILED * 100 / TOTAL))
    VALID_RATE=$((WASM_VALID * 100 / TOTAL))
else
    COMPILE_RATE=0
    VALID_RATE=0
fi

# Report
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}                  HONEST RESULTS REPORT                     ${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo ""
echo "Total test files:        $TOTAL"
echo ""
echo -e "Compilation:"
echo -e "  ${GREEN}✓ Success:${NC}             $COMPILED ($COMPILE_RATE%)"
echo -e "  ${RED}✗ Failed:${NC}              $COMPILE_FAILED"
echo ""
echo -e "WASM Validation:"
echo -e "  ${GREEN}✓ Valid WASM:${NC}          $WASM_VALID ($VALID_RATE%)"
echo -e "  ${YELLOW}⚠ Invalid WASM:${NC}        $WASM_INVALID"
echo ""
echo -e "Results saved to: ${BLUE}$RESULTS_FILE${NC}"
echo ""

# Final verdict
if [ $WASM_VALID -eq $TOTAL ]; then
    echo -e "${GREEN}╔═══════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║          🎉 100% SUCCESS - ALL TESTS PASS! 🎉            ║${NC}"
    echo -e "${GREEN}╚═══════════════════════════════════════════════════════════╝${NC}"
    exit 0
else
    FAIL_COUNT=$((TOTAL - WASM_VALID))
    echo -e "${RED}╔═══════════════════════════════════════════════════════════╗${NC}"
    echo -e "${RED}║       ⚠️  $FAIL_COUNT/$TOTAL TESTS FAILED - FIX REQUIRED ⚠️      ║${NC}"
    echo -e "${RED}╚═══════════════════════════════════════════════════════════╝${NC}"
    exit 1
fi
