#!/bin/bash

# Quick error analysis script for failing tests
# Tests a sample of files and categorizes errors

cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

OUTPUT_FILE="system-documents/development-scripts/error_analysis_$(date +%Y%m%d_%H%M%S).txt"

echo "=== QUICK ERROR ANALYSIS ===" > "$OUTPUT_FILE"
echo "Date: $(date)" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"

# Function to test and categorize a file
test_file() {
    local file=$1
    echo "Testing: $file" >> "$OUTPUT_FILE"

    result=$(cargo run --bin clean-language-compiler compile -i "$file" -o tests/output/temp.wasm 2>&1)

    if echo "$result" | grep -q "Successfully compiled"; then
        echo "  ✓ SUCCESS" >> "$OUTPUT_FILE"
    elif echo "$result" | grep -q "Namespace function.*not found"; then
        echo "  ✗ MISSING NAMESPACE FUNCTION" >> "$OUTPUT_FILE"
        echo "$result" | grep "Namespace function" | head -1 >> "$OUTPUT_FILE"
    elif echo "$result" | grep -q "Function expects.*arguments"; then
        echo "  ✗ DEFAULT PARAMETER ERROR" >> "$OUTPUT_FILE"
        echo "$result" | grep "Function expects" | head -1 >> "$OUTPUT_FILE"
    elif echo "$result" | grep -q "Cannot unify types: null"; then
        echo "  ✗ NULL TYPE INFERENCE ERROR" >> "$OUTPUT_FILE"
        echo "$result" | grep "Cannot unify types: null" | head -1 >> "$OUTPUT_FILE"
    elif echo "$result" | grep -q "Undefined variable"; then
        echo "  ✗ UNDEFINED VARIABLE" >> "$OUTPUT_FILE"
        echo "$result" | grep "Undefined variable" | head -1 >> "$OUTPUT_FILE"
    else
        echo "  ✗ OTHER ERROR" >> "$OUTPUT_FILE"
        echo "$result" | grep "Error" | head -3 >> "$OUTPUT_FILE"
    fi
    echo "" >> "$OUTPUT_FILE"
}

# Test sample files
echo "Testing sample files..." >&2

test_file "tests/cln/stdlib/console/50_input_method_syntax.cln"
test_file "tests/cln/language/functions/64_default_parameters_spec.cln"
test_file "tests/cln/language/control_flow/36_conditionals.cln"
test_file "tests/cln/debug/test_no_return_type.cln"
test_file "tests/cln/debug/test_static_method.cln"
test_file "tests/cln/stdlib/string/69_string_interpolation_comprehensive.cln"
test_file "tests/cln/core/basics/56_apply_blocks_comprehensive.cln"

echo "=== ERROR CATEGORY SUMMARY ===" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"
echo "Missing Namespace Functions:" >> "$OUTPUT_FILE"
grep -c "MISSING NAMESPACE FUNCTION" "$OUTPUT_FILE" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"
echo "Default Parameter Errors:" >> "$OUTPUT_FILE"
grep -c "DEFAULT PARAMETER ERROR" "$OUTPUT_FILE" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"
echo "Null Type Inference Errors:" >> "$OUTPUT_FILE"
grep -c "NULL TYPE INFERENCE ERROR" "$OUTPUT_FILE" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"

echo "Analysis complete. Results in: $OUTPUT_FILE" >&2
cat "$OUTPUT_FILE"
