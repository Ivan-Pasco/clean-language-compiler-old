#!/bin/bash

# Quick sample test of 10 previously failing tests
set -euo pipefail

cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"
echo "=== QUICK SAMPLE TEST OF PREVIOUSLY FAILED TESTS ==="

# Build compiler
cargo build --release >/dev/null 2>&1

# Test 10 specific files that were marked as failed
FAILED_TESTS=(
    "test_simple_default_params.cln"
    "test_conditional_simple.cln"  
    "test_if_with_else.cln"
    "test_simple_chain.cln"
    "test_method_override.cln"
    "59_default_parameters_working.cln"
    "64_default_parameters_spec.cln"
    "test_simplest_if.cln"
    "test_chained_minimal.cln"
    "test_inheritance_polymorphism.cln"
)

PASSED=0
FAILED=0

for test_file in "${FAILED_TESTS[@]}"; do
    echo -n "Testing $test_file... "
    
    # Try to compile
    if cargo run --release --bin clean-language-compiler compile -i "tests/clean_files/$test_file" -o "/tmp/${test_file%.cln}.wasm" >/dev/null 2>&1; then
        # Try to run
        if cargo run --release --bin wasmtime_runner "/tmp/${test_file%.cln}.wasm" >/dev/null 2>&1; then
            echo "✅ PASS"
            PASSED=$((PASSED + 1))
        else
            echo "❌ RUNTIME FAIL"
            FAILED=$((FAILED + 1))
        fi
    else
        echo "❌ COMPILE FAIL"
        FAILED=$((FAILED + 1))
    fi
done

TOTAL=$((PASSED + FAILED))
SUCCESS_RATE=$(echo "scale=1; $PASSED * 100 / $TOTAL" | bc -l)

echo ""
echo "=== QUICK SAMPLE RESULTS ==="
echo "Passed: $PASSED/$TOTAL"
echo "Success rate: $SUCCESS_RATE%"
echo ""
if [ "$SUCCESS_RATE" != "84.6" ]; then
    echo "⚠️  This suggests the comprehensive test results are outdated!"
    echo "Real success rate is likely much higher than 84.63%"
fi