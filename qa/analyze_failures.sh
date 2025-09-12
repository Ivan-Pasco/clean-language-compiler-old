#!/bin/bash

# Failure Analysis Script for Clean Language Compiler QA
# Categorizes errors by type and provides detailed analysis

echo "=== FAILURE ANALYSIS REPORT ==="
echo ""

# Initialize counters
type_errors=0
parsing_errors=0
method_chaining_errors=0
default_param_errors=0
list_operation_errors=0
stdlib_errors=0
async_errors=0
http_errors=0
inheritance_errors=0
other_errors=0

# Create detailed analysis file
analysis_file="tests/qa_results/failure_analysis.txt"
> "$analysis_file"

echo "=== FAILURE ANALYSIS $(date) ===" >> "$analysis_file"
echo "" >> "$analysis_file"

echo "Analyzing 57 failed tests..."
echo ""

# Process each failed test
while IFS= read -r test_name; do
    if [ -n "$test_name" ]; then
        test_file="tests/clean_files/${test_name}.cln"
        
        if [ -f "$test_file" ]; then
            echo "Analyzing: $test_name" >> "$analysis_file"
            
            # Run compilation and capture error
            error_output=$(cargo run --bin clean-language-compiler compile -i "$test_file" -o "tests/output/${test_name}.wasm" 2>&1)
            echo "Error: $error_output" >> "$analysis_file"
            echo "---" >> "$analysis_file"
            
            # Categorize error
            if echo "$error_output" | grep -q "Cannot initialize variable.*with expression of type"; then
                type_errors=$((type_errors + 1))
                echo "  TYPE_ERROR: $test_name"
            elif echo "$error_output" | grep -q "Function expects return value.*but got void return"; then
                type_errors=$((type_errors + 1))
                echo "  TYPE_ERROR (return): $test_name"
            elif echo "$error_output" | grep -q "Variable.*not found"; then
                type_errors=$((type_errors + 1))
                echo "  TYPE_ERROR (variable): $test_name"
            elif echo "$error_output" | grep -q "Method.*not found"; then
                method_chaining_errors=$((method_chaining_errors + 1))
                echo "  METHOD_ERROR: $test_name"
            elif echo "$error_output" | grep -q -E "(default|parameter)"; then
                default_param_errors=$((default_param_errors + 1))
                echo "  DEFAULT_PARAM_ERROR: $test_name"
            elif echo "$error_output" | grep -q -E "(list|array)"; then
                list_operation_errors=$((list_operation_errors + 1))
                echo "  LIST_ERROR: $test_name"
            elif echo "$error_output" | grep -q -E "(stdlib|Math|String)"; then
                stdlib_errors=$((stdlib_errors + 1))
                echo "  STDLIB_ERROR: $test_name"
            elif echo "$error_output" | grep -q -E "(async|await)"; then
                async_errors=$((async_errors + 1))
                echo "  ASYNC_ERROR: $test_name"
            elif echo "$error_output" | grep -q -E "(http|HTTP)"; then
                http_errors=$((http_errors + 1))
                echo "  HTTP_ERROR: $test_name"
            elif echo "$error_output" | grep -q -E "(inherit|base|super)"; then
                inheritance_errors=$((inheritance_errors + 1))
                echo "  INHERITANCE_ERROR: $test_name"
            else
                other_errors=$((other_errors + 1))
                echo "  OTHER_ERROR: $test_name"
            fi
        fi
    fi
done < tests/qa_results/failed_tests.txt

echo ""
echo "=== ERROR CATEGORIZATION SUMMARY ==="
echo "Type Errors:           $type_errors"
echo "Method Chaining:       $method_chaining_errors"  
echo "Default Parameters:    $default_param_errors"
echo "List Operations:       $list_operation_errors"
echo "Standard Library:      $stdlib_errors"
echo "Async/Await:           $async_errors"
echo "HTTP Operations:       $http_errors"
echo "Inheritance:           $inheritance_errors"
echo "Other:                 $other_errors"
echo ""

# Calculate percentages
total_failures=57
echo "=== ERROR DISTRIBUTION ===" 
echo "Type Errors:           $(echo "scale=1; $type_errors * 100 / $total_failures" | bc -l)%"
echo "Method Chaining:       $(echo "scale=1; $method_chaining_errors * 100 / $total_failures" | bc -l)%"
echo "Default Parameters:    $(echo "scale=1; $default_param_errors * 100 / $total_failures" | bc -l)%"
echo "List Operations:       $(echo "scale=1; $list_operation_errors * 100 / $total_failures" | bc -l)%"
echo "Standard Library:      $(echo "scale=1; $stdlib_errors * 100 / $total_failures" | bc -l)%"
echo "Other Categories:      $(echo "scale=1; ($async_errors + $http_errors + $inheritance_errors + $other_errors) * 100 / $total_failures" | bc -l)%"
echo ""

# Write summary to analysis file
echo "" >> "$analysis_file"
echo "=== SUMMARY ===" >> "$analysis_file"
echo "Total Failed Tests: $total_failures" >> "$analysis_file"
echo "Type Errors: $type_errors" >> "$analysis_file"
echo "Method Chaining: $method_chaining_errors" >> "$analysis_file"
echo "Default Parameters: $default_param_errors" >> "$analysis_file"
echo "List Operations: $list_operation_errors" >> "$analysis_file"
echo "Standard Library: $stdlib_errors" >> "$analysis_file"
echo "Async/Await: $async_errors" >> "$analysis_file"
echo "HTTP Operations: $http_errors" >> "$analysis_file"
echo "Inheritance: $inheritance_errors" >> "$analysis_file"
echo "Other: $other_errors" >> "$analysis_file"

echo "Detailed analysis saved to: $analysis_file"
