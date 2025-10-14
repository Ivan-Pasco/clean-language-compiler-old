#!/bin/bash

# Quick Error Scanner - Tests files and categorizes errors
# Faster than comprehensive test, focuses on error discovery

COMPILER="./target/debug/clean-language-compiler"

# Build if needed
if [ ! -f "$COMPILER" ]; then
    cargo build --bin clean-language-compiler 2>&1 | tail -1
fi

mkdir -p tests/output

declare -A error_types
total=0

echo "Scanning for error patterns..."
echo ""

# Sample test files from each category
test_files=(
    "tests/cln/core/basics/00_minimal.cln"
    "tests/cln/core/types/02_precision_modifiers.cln"
    "tests/cln/core/types/06_type_conversions.cln"
    "tests/cln/core/types/07_lists_basic.cln"
    "tests/cln/core/variables/01_variable_declaration.cln"
    "tests/cln/core/variables/02_variables_basic.cln"
    "tests/cln/functions/declarations/01_simple_functions.cln"
    "tests/cln/functions/calls/01_function_calls.cln"
    "tests/cln/control/conditionals/01_if_statements.cln"
    "tests/cln/control/loops/01_while_loops.cln"
    "tests/cln/language/classes/01_basic_classes.cln"
    "tests/cln/language/functions/01_function_basics.cln"
    "tests/cln/stdlib/math/01_math_functions.cln"
    "tests/cln/stdlib/string/01_string_functions.cln"
)

for file in "${test_files[@]}"; do
    if [ ! -f "$file" ]; then
        echo "⚠️  File not found: $file"
        continue
    fi

    total=$((total + 1))
    wasm_file="tests/output/$(basename ${file%.cln}).wasm"

    # Compile and capture error
    output=$($COMPILER compile -i "$file" -o "$wasm_file" 2>&1)

    if echo "$output" | grep -q "Successfully compiled"; then
        echo "✅ $(basename $file)"
    else
        # Extract error type
        error_msg=$(echo "$output" | grep -E "Error|error:" | head -1)

        if echo "$error_msg" | grep -q "Unsupported statement type"; then
            error_type="unsupported_statement"
        elif echo "$error_msg" | grep -q "Syntax error"; then
            error_type="syntax_error"
        elif echo "$error_msg" | grep -q "Type error"; then
            error_type="type_error"
        elif echo "$error_msg" | grep -q "Undefined"; then
            error_type="undefined_reference"
        elif echo "$error_msg" | grep -q "No such file"; then
            error_type="file_not_found"
        elif echo "$error_msg" | grep -q "semantic"; then
            error_type="semantic_error"
        else
            error_type="other"
        fi

        error_types["$error_type"]=$((${error_types["$error_type"]:-0} + 1))

        echo "❌ $(basename $file): $error_type"
        echo "   └─ $error_msg"
    fi
done

echo ""
echo "========================================="
echo "Error Pattern Summary"
echo "========================================="
echo "Total files tested: $total"
echo ""
echo "Error Categories:"
for error in "${!error_types[@]}"; do
    echo "  $error: ${error_types[$error]}"
done
