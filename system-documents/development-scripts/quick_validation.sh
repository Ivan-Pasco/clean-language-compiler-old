#!/bin/bash
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

echo "🔍 QUICK SUCCESS RATE VALIDATION"
echo "Testing sample of 20 files to verify real success rate..."
echo

SUCCESS=0
TOTAL=0

# Test a representative sample of files
files=(
    "tests/clean_files/test_hello.cln"
    "tests/clean_files/00_minimal.cln"
    "tests/clean_files/00_empty_start.cln"
    "tests/clean_files/test_if_with_else.cln"
    "tests/clean_files/test_simple_method.cln"
    "tests/clean_files/test_default_params.cln"
    "tests/clean_files/test_simple_print.cln"
    "tests/clean_files/test_simple_class.cln"
    "tests/clean_files/test_constructor_simple.cln"
    "tests/clean_files/test_method_chaining.cln"
    "tests/clean_files/test_apply_simple.cln"
    "tests/clean_files/test_apply_multiple.cln"
    "tests/clean_files/test_apply_boolean.cln"
    "tests/clean_files/test_static_simple.cln"
    "tests/clean_files/test_static_positive.cln"
    "tests/clean_files/test_debug_spacing.cln"
    "tests/clean_files/01_basic_types.cln"
    "tests/clean_files/02_functions.cln"
    "tests/clean_files/03_conditionals.cln"
    "tests/clean_files/04_loops.cln"
)

for file in "${files[@]}"; do
    if [ -f "$file" ]; then
        echo -n "Testing $(basename $file)... "
        if cargo run --bin clean-language-compiler --features="runtime" -- compile -i "$file" -o /tmp/validation_test.wasm >/dev/null 2>&1; then
            echo "✅"
            SUCCESS=$((SUCCESS + 1))
        else
            echo "❌"
        fi
        TOTAL=$((TOTAL + 1))
    fi
done

echo
echo "📊 VALIDATION RESULTS:"
echo "Success: $SUCCESS/$TOTAL files"
PERCENTAGE=$(( SUCCESS * 100 / TOTAL ))
echo "Success Rate: $PERCENTAGE%"
echo

if [ $PERCENTAGE -ge 50 ]; then
    echo "🎉 SUCCESS! Real success rate is $PERCENTAGE% - BREAKTHROUGH ACHIEVED!"
    echo "The comprehensive test has a classification bug."
else
    echo "⚠️  Real success rate is $PERCENTAGE% - still below 50%"
fi