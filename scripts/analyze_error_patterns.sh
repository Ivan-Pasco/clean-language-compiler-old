#!/bin/bash

# Error Pattern Analysis Script
# Categorizes and counts error types from compilation failures

echo "🔍 Error Pattern Analysis"
echo "======================================================"
echo ""

declare -A ERROR_PATTERNS
declare -A ERROR_FILES

# Failed files from baseline
FAILED_FILES=(
    "tests/cln/advanced/async/20_async_parallel.cln"
    "tests/cln/advanced/async/52_async_keywords.cln"
    "tests/cln/core/basics/61_multiline_expressions.cln"
    "tests/cln/core/types/46_matrix_literals.cln"
    "tests/cln/language/classes/14_classes_basic.cln"
    "tests/cln/language/classes/15_classes_inheritance.cln"
    "tests/cln/language/classes/16_classes_polymorphism.cln"
    "tests/cln/language/control_flow/18_control_flow_loops.cln"
    "tests/cln/language/control_flow/36_conditionals.cln"
    "tests/cln/functions/declarations/06_function_definitions.cln"
    "tests/cln/stdlib/string/43_string_interpolation.cln"
    "tests/cln/debug/test_constructor_with_base.cln"
    "tests/cln/debug/test_inheritance.cln"
    "tests/cln/debug/test_method_override.cln"
    "tests/cln/parser_compliance/04_type_system.cln"
)

# Analyze each failed file
for file in "${FAILED_FILES[@]}"; do
    if [ -f "$file" ]; then
        echo "Analyzing: $(basename "$file")"

        # Compile and capture error
        ERROR_OUTPUT=$(cargo run --bin clean-language-compiler compile -i "$file" -o "/tmp/test.wasm" 2>&1)

        # Categorize error type
        if echo "$ERROR_OUTPUT" | grep -q "Cannot call non-function type: Class"; then
            ERROR_TYPE="Class Constructor Call Issue"
        elif echo "$ERROR_OUTPUT" | grep -q "Undefined variable"; then
            ERROR_TYPE="Undefined Variable (Scope)"
        elif echo "$ERROR_OUTPUT" | grep -q "Expected name.*found Colon"; then
            ERROR_TYPE="Parser: Function Signature"
        elif echo "$ERROR_OUTPUT" | grep -q "Unexpected token.*InterpolationStart"; then
            ERROR_TYPE="Parser: String Interpolation"
        elif echo "$ERROR_OUTPUT" | grep -q "Cannot unify types.*Matrix"; then
            ERROR_TYPE="Type: Matrix Literal"
        elif echo "$ERROR_OUTPUT" | grep -q "Syntax error"; then
            ERROR_TYPE="Parser: Syntax Error"
        elif echo "$ERROR_OUTPUT" | grep -q "Type error"; then
            ERROR_TYPE="Type Error (General)"
        elif echo "$ERROR_OUTPUT" | grep -q "Undefined"; then
            ERROR_TYPE="Semantic: Undefined Symbol"
        else
            ERROR_TYPE="Other"
        fi

        # Count occurrences
        if [ -z "${ERROR_PATTERNS[$ERROR_TYPE]}" ]; then
            ERROR_PATTERNS[$ERROR_TYPE]=0
        fi
        ERROR_PATTERNS[$ERROR_TYPE]=$((${ERROR_PATTERNS[$ERROR_TYPE]} + 1))

        # Store example file
        ERROR_FILES[$ERROR_TYPE]="$file"
    fi
done

echo ""
echo "======================================================"
echo "📊 ERROR PATTERN SUMMARY"
echo "======================================================"
echo ""

# Sort and display patterns by frequency
for pattern in "${!ERROR_PATTERNS[@]}"; do
    count=${ERROR_PATTERNS[$pattern]}
    example=${ERROR_FILES[$pattern]}
    echo "❌ $pattern: $count occurrences"
    echo "   Example: $(basename "$example")"
    echo ""
done

echo "======================================================"
echo "🎯 PRIORITY CLASSIFICATION"
echo "======================================================"

# Calculate priorities based on impact
for pattern in "${!ERROR_PATTERNS[@]}"; do
    count=${ERROR_PATTERNS[$pattern]}

    if [ $count -ge 5 ]; then
        priority="🔴 CRITICAL"
    elif [ $count -ge 3 ]; then
        priority="🟡 HIGH"
    else
        priority="🟢 MEDIUM"
    fi

    echo "$priority - $pattern ($count files affected)"
done

echo ""
echo "======================================================"
