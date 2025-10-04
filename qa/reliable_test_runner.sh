#!/bin/bash

# Reliable Test Runner for Clean Language Compiler
# Eliminates false negatives and provides accurate error categorization

set -euo pipefail

# Configuration
TEST_DIR="tests/clean_files"
OUTPUT_DIR="/tmp/clean_test_results"
VERBOSE=false

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

echo "🧪 Clean Language Compiler - Reliable Test Suite"
echo "================================================="
echo ""

# Test counters
total=0
passed=0
failed=0
declare -a failing_tests=()
declare -a error_categories=()

# Function to categorize error type
categorize_error() {
    local error_output="$1"
    local filename="$2"
    
    if echo "$error_output" | grep -q "Expected one of: end of input, program_item"; then
        echo "PARSE_ERROR_PROGRAM_BOUNDARY"
    elif echo "$error_output" | grep -q "Parsing failed"; then
        echo "PARSE_ERROR_GENERAL"
    elif echo "$error_output" | grep -q "Syntax"; then
        echo "SYNTAX_ERROR"
    elif echo "$error_output" | grep -q "Type"; then
        echo "TYPE_ERROR"
    elif echo "$error_output" | grep -q "Semantic"; then
        echo "SEMANTIC_ERROR"
    elif echo "$error_output" | grep -q "Codegen"; then
        echo "CODEGEN_ERROR"
    else
        echo "UNKNOWN_ERROR"
    fi
}

# Test all .cln files
echo "Testing all .cln files in $TEST_DIR..."
echo ""

for file in "$TEST_DIR"/*.cln; do
    if [ ! -f "$file" ]; then
        continue
    fi
    
    total=$((total + 1))
    filename=$(basename "$file")
    output_file="$OUTPUT_DIR/${filename}.wasm"
    
    # Clean previous output
    rm -f "$output_file"
    
    # Attempt compilation with error capture
    error_output=$(cargo run --bin clean-language-compiler compile -i "$file" -o "$output_file" 2>&1) || compilation_failed=true
    
    if [ "${compilation_failed:-false}" = true ]; then
        # Genuine failure detected
        failed=$((failed + 1))
        failing_tests+=("$filename")
        
        # Categorize error type
        error_category=$(categorize_error "$error_output" "$filename")
        error_categories+=("$error_category")
        
        echo -e "${RED}❌ FAIL${NC} $filename ($error_category)"
        
        if [ "$VERBOSE" = true ]; then
            echo "   Error: $(echo "$error_output" | head -3 | tail -1)"
        fi
    else
        # Success
        passed=$((passed + 1))
        echo -e "${GREEN}✅ PASS${NC} $filename"
        
        # Verify WASM file was actually generated
        if [ ! -f "$output_file" ]; then
            echo -e "${YELLOW}⚠️  WARNING${NC} $filename: Reported success but no WASM file generated"
        fi
    fi
    
    # Reset for next iteration
    unset compilation_failed
done

echo ""
echo "================================================="
echo "📊 TEST RESULTS SUMMARY"
echo "================================================="
echo "Total tests: $total"
echo -e "Passed: ${GREEN}$passed${NC} ($(( passed * 100 / total ))%)"
echo -e "Failed: ${RED}$failed${NC} ($(( failed * 100 / total ))%)"
echo ""

if [ $failed -gt 0 ]; then
    echo "================================================="
    echo "🔍 ERROR ANALYSIS"
    echo "================================================="
    
    # Count error categories
    declare -A category_counts
    for category in "${error_categories[@]}"; do
        category_counts["$category"]=$((${category_counts["$category"]:-0} + 1))
    done
    
    echo "Error Distribution:"
    for category in "${!category_counts[@]}"; do
        count=${category_counts["$category"]}
        percentage=$(( count * 100 / failed ))
        echo "  $category: $count failures (${percentage}%)"
    done
    
    echo ""
    echo "Detailed Failures:"
    for i in "${!failing_tests[@]}"; do
        echo "  ${failing_tests[$i]} - ${error_categories[$i]}"
    done
fi

echo ""
echo "================================================="
echo "✅ Test run completed successfully"
echo "Results saved to: $OUTPUT_DIR"
echo "================================================="

# Exit with appropriate code
if [ $failed -gt 0 ]; then
    exit 1
else
    exit 0
fi