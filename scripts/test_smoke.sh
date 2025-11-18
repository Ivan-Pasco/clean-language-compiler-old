#!/bin/bash
# Smoke Test: Fast validation with 20 representative tests
# Target: <30 seconds
# Usage: ./scripts/test_smoke.sh

set -e

RUNNER="./target/release/wasmtime_runner"
OUTPUT_DIR="tests/output"

# ANSI color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "🔥 Running Smoke Test (20 representative files)..."
echo ""

# Representative test files (hand-picked from each category)
SMOKE_TESTS=(
    # Core language features (5)
    "01_simple_variables"
    "03_arithmetic_operations"
    "04_if_else"
    "06_statements"
    "08_string_operations"

    # Loop constructs (3)
    "05_loop_constructs"
    "36_while_loop"
    "65_iterate_scope_shadowing"

    # Classes and methods (3)
    "35_method_style"
    "50_input_method_syntax"
    "58_class_properties"

    # Error handling (2)
    "21_error_handling_try_catch"
    "30_error_propagation"

    # Lists and arrays (3)
    "68_list_behaviors_comprehensive"
    "37_lists"
    "69_list_with_arithmetic"

    # Functions (2)
    "59_default_parameters_simple"
    "72_default_parameters_comprehensive"

    # Integration tests (2)
    "10_comprehensive_features"
    "54_integration_test"
)

PASS=0
FAIL=0
START_TIME=$(date +%s)

for test in "${SMOKE_TESTS[@]}"; do
    WASM_FILE="$OUTPUT_DIR/${test}.wasm"

    if [ ! -f "$WASM_FILE" ]; then
        echo -e "${YELLOW}⚠️  SKIP${NC}: $test (file not found)"
        continue
    fi

    echo -n "Testing: $test ... "

    if $RUNNER "$WASM_FILE" >/dev/null 2>&1; then
        echo -e "${GREEN}✅ PASS${NC}"
        ((PASS++))
    else
        echo -e "${RED}❌ FAIL${NC}"
        ((FAIL++))
    fi
done

END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

TOTAL=$((PASS + FAIL))
if [ $TOTAL -gt 0 ]; then
    RATE=$(awk "BEGIN {printf \"%.1f\", ($PASS * 100.0 / $TOTAL)}")
else
    RATE=0.0
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📊 Smoke Test Results"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Total tested: $TOTAL"
echo -e "Passed: ${GREEN}$PASS${NC}"
echo -e "Failed: ${RED}$FAIL${NC}"
echo "Success Rate: $RATE%"
echo "Duration: ${DURATION}s"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [ $FAIL -eq 0 ]; then
    echo -e "${GREEN}✅ All smoke tests passed!${NC}"
    exit 0
else
    echo -e "${RED}❌ Some tests failed${NC}"
    exit 1
fi
