#!/bin/bash

# Comprehensive validation test for Clean Language compiler
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

echo "🔍 Clean Language Compiler - Comprehensive QA Validation"
echo "========================================================"
echo "Testing core language features across representative files..."
echo

# Test categories with representative files
declare -A test_categories=(
    ["BasicVariables"]="00_minimal.cln 01_hello_world.cln 02_variables_basic.cln"
    ["ArithmeticOperations"]="03_arithmetic_operations.cln 04_comparison_operations.cln 05_logical_operations.cln"
    ["Functions"]="10_functions_basic.cln 11_functions_overloading.cln"
    ["ClassesAndOOP"]="14_classes_basic.cln 15_classes_inheritance.cln"
    ["ControlFlow"]="17_control_flow_if.cln 18_control_flow_loops.cln"
    ["ListsAndCollections"]="07_lists_basic.cln 34_list_behaviors_simple.cln"
    ["StringOperations"]="43_string_interpolation.cln 47_string_interpolation.cln"
    ["MethodCalls"]="35_method_style_simple.cln 49_static_method_calls_simple.cln"
    ["TypeSystem"]="44_type_precision_simple.cln 45_numeric_literals_simple.cln"
    ["StandardLibrary"]="25_stdlib_functions.cln"
)

# Create output directory
mkdir -p tests/wasm

total_files=0
passed_files=0
failed_files=0
category_results=""

for category_key in "${!test_categories[@]}"; do
    # Convert key to readable format
    category_display=$(echo "$category_key" | sed 's/\([A-Z]\)/ \1/g' | sed 's/^ //')
    echo "📂 Testing: $category_display"
    echo "   Files: ${test_categories[$category_key]}"
    
    category_passed=0
    category_total=0
    
    for file in ${test_categories[$category_key]}; do
        if [[ -f "tests/clean_files/$file" ]]; then
            total_files=$((total_files + 1))
            category_total=$((category_total + 1))
            
            filename=$(basename "$file" .cln)
            output_file="tests/wasm/${filename}.wasm"
            
            # Remove existing output file
            rm -f "$output_file"
            
            echo -n "   • $file... "
            
            # Compile with output suppression and check for success
            if cargo run --bin clean-language-compiler -- compile -i "tests/clean_files/$file" -o "$output_file" >/dev/null 2>&1; then
                if [[ -f "$output_file" ]] && [[ -s "$output_file" ]]; then
                    echo "✅ PASS ($(stat -f%z "$output_file") bytes)"
                    passed_files=$((passed_files + 1))
                    category_passed=$((category_passed + 1))
                else
                    echo "❌ FAIL (no output)"
                    failed_files=$((failed_files + 1))
                fi
            else
                echo "❌ FAIL (compilation error)"
                failed_files=$((failed_files + 1))
            fi
        else
            echo "   • $file... ⚠️  NOT FOUND"
            failed_files=$((failed_files + 1))
            total_files=$((total_files + 1))
            category_total=$((category_total + 1))
        fi
    done
    
    if [[ $category_total -gt 0 ]]; then
        category_success_rate=$(echo "scale=1; $category_passed * 100 / $category_total" | bc -l)
        category_results="$category_results   $category_display: $category_passed/$category_total (${category_success_rate}%)\n"
    fi
    echo
done

# Calculate overall success rate
if [[ $total_files -gt 0 ]]; then
    success_rate=$(echo "scale=1; $passed_files * 100 / $total_files" | bc -l)
else
    success_rate="0.0"
fi

echo "========================================================"
echo "🏁 FINAL QA VALIDATION RESULTS"
echo "========================================================"
echo "Overall Statistics:"
echo "   Total files tested: $total_files"
echo "   Successful compilations: $passed_files"
echo "   Failed compilations: $failed_files"
echo "   Overall success rate: ${success_rate}%"
echo
echo "Category Breakdown:"
echo -e "$category_results"

# Determine status based on success rate
if (( $(echo "$success_rate >= 95.0" | bc -l) )); then
    echo "🎉 STATUS: EXCELLENT - Compiler is ready for production!"
elif (( $(echo "$success_rate >= 80.0" | bc -l) )); then
    echo "✅ STATUS: GOOD - Minor issues may remain"
elif (( $(echo "$success_rate >= 60.0" | bc -l) )); then
    echo "⚠️  STATUS: NEEDS WORK - Significant issues present"
else
    echo "❌ STATUS: CRITICAL ISSUES - Major problems need addressing"
fi

echo "========================================================"

# WASM validation check
echo "🔧 WASM File Validation:"
wasm_count=$(find tests/wasm -name "*.wasm" -type f | wc -l | xargs)
wasm_size=$(find tests/wasm -name "*.wasm" -type f -exec stat -f%z {} \; | awk '{sum+=$1} END {print sum}')

if [[ $wasm_count -gt 0 ]]; then
    avg_size=$(echo "scale=0; $wasm_size / $wasm_count" | bc -l)
    echo "   Generated WASM files: $wasm_count"
    echo "   Total WASM size: $(echo "scale=1; $wasm_size / 1024" | bc -l) KB"
    echo "   Average file size: $(echo "scale=1; $avg_size / 1024" | bc -l) KB"
    
    # Check for suspiciously small files (potential compilation issues)
    small_files=$(find tests/wasm -name "*.wasm" -type f -size -1000c | wc -l | xargs)
    if [[ $small_files -gt 0 ]]; then
        echo "   ⚠️  Warning: $small_files files are unusually small (<1KB)"
    fi
else
    echo "   ❌ No WASM files found!"
fi

echo "========================================================"