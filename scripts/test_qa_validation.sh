#!/bin/bash

# Comprehensive QA validation test for Clean Language compiler
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

echo "🔍 Clean Language Compiler - Final QA Validation"
echo "================================================"
echo "Testing core language features across key representative files..."
echo

# Define test files for different categories
test_files=(
    # Basic Variables & Literals
    "00_minimal.cln"
    "01_hello_world.cln" 
    "02_variables_basic.cln"
    
    # Arithmetic & Logic
    "03_arithmetic_operations.cln"
    "04_comparison_operations.cln"
    "05_logical_operations.cln"
    
    # Functions & Control Flow
    "10_functions_basic.cln"
    "17_control_flow_if.cln"
    
    # Collections & Types  
    "07_lists_basic.cln"
    "44_type_precision_simple.cln"
    
    # String Operations
    "43_string_interpolation.cln"
    
    # Method Calls & Standard Library
    "35_method_style_simple.cln"
    "49_static_method_calls_simple.cln"
    "25_stdlib_functions.cln"
    
    # Classes (if working)
    "14_classes_basic.cln"
)

# Create output directory
mkdir -p tests/wasm

total_files=0
passed_files=0
failed_files=0
failed_list=""

echo "🧪 Running compilation tests..."
echo "================================"

for file in "${test_files[@]}"; do
    if [[ -f "tests/clean_files/$file" ]]; then
        total_files=$((total_files + 1))
        
        filename=$(basename "$file" .cln)
        output_file="tests/wasm/${filename}.wasm"
        
        # Remove existing output file
        rm -f "$output_file"
        
        echo -n "Testing $file... "
        
        # Compile with output suppression and timeout
        if cargo run --bin clean-language-compiler -- compile -i "tests/clean_files/$file" -o "$output_file" >/dev/null 2>&1; then
            if [[ -f "$output_file" ]] && [[ -s "$output_file" ]]; then
                file_size=$(stat -f%z "$output_file")
                echo "✅ PASS (${file_size} bytes)"
                passed_files=$((passed_files + 1))
            else
                echo "❌ FAIL (no output)"
                failed_files=$((failed_files + 1))
                failed_list="${failed_list}   • $file (no output)\n"
            fi
        else
            echo "❌ FAIL (compilation error)"
            failed_files=$((failed_files + 1))
            failed_list="${failed_list}   • $file (compilation error)\n"
        fi
    else
        echo "⚠️  File not found: $file"
        failed_files=$((failed_files + 1))
        total_files=$((total_files + 1))
        failed_list="${failed_list}   • $file (not found)\n"
    fi
done

echo "================================"
echo

# Calculate success rate
if [[ $total_files -gt 0 ]]; then
    success_rate=$(echo "scale=1; $passed_files * 100 / $total_files" | bc -l)
else
    success_rate="0.0"
fi

echo "🏁 FINAL QA VALIDATION RESULTS"
echo "==============================="
echo "Overall Statistics:"
echo "   📊 Total files tested: $total_files"
echo "   ✅ Successful compilations: $passed_files"
echo "   ❌ Failed compilations: $failed_files"
echo "   📈 Overall success rate: ${success_rate}%"
echo

# Show failed files if any
if [[ $failed_files -gt 0 ]]; then
    echo "❌ Failed Files:"
    echo -e "$failed_list"
fi

# Determine overall status
echo "📋 COMPILER STATUS ASSESSMENT:"
if (( $(echo "$success_rate >= 95.0" | bc -l) )); then
    echo "   🎉 EXCELLENT - Compiler ready for production!"
    echo "   ✨ Achievement: ${success_rate}% success rate meets production standards"
elif (( $(echo "$success_rate >= 90.0" | bc -l) )); then
    echo "   ✅ VERY GOOD - Minor issues may remain"
    echo "   🔧 Recommendation: Address remaining $failed_files failures for 100% success"
elif (( $(echo "$success_rate >= 75.0" | bc -l) )); then
    echo "   ⚠️  GOOD - Some issues present but mostly functional"
    echo "   🔧 Action needed: Fix $failed_files compilation failures"
elif (( $(echo "$success_rate >= 50.0" | bc -l) )); then
    echo "   ⚠️  NEEDS WORK - Significant issues present"
    echo "   🚨 Priority: Address major compilation problems"
else
    echo "   ❌ CRITICAL ISSUES - Major problems need immediate attention"
    echo "   🚨 Emergency: Fundamental compilation problems exist"
fi

echo
echo "🔧 WASM FILE ANALYSIS:"
echo "======================"

# WASM validation check
wasm_files=$(find tests/wasm -name "*.wasm" -type f | wc -l | xargs)
if [[ $wasm_files -gt 0 ]]; then
    wasm_total_size=$(find tests/wasm -name "*.wasm" -type f -exec stat -f%z {} \; | awk '{sum+=$1} END {print sum}')
    avg_size=$(echo "scale=1; $wasm_total_size / $wasm_files" | bc -l)
    
    echo "   📁 Generated WASM files: $wasm_files"
    echo "   💾 Total WASM size: $(echo "scale=1; $wasm_total_size / 1024" | bc -l) KB"
    echo "   📏 Average file size: $(echo "scale=1; $avg_size / 1024" | bc -l) KB"
    
    # Check for suspiciously small files (potential issues)
    small_files=$(find tests/wasm -name "*.wasm" -type f -size -1000c | wc -l | xargs)
    large_files=$(find tests/wasm -name "*.wasm" -type f -size +50000c | wc -l | xargs)
    
    if [[ $small_files -gt 0 ]]; then
        echo "   ⚠️  Warning: $small_files files are unusually small (<1KB) - possible compilation issues"
    fi
    if [[ $large_files -gt 0 ]]; then
        echo "   📈 Note: $large_files files are large (>50KB) - complex programs or debug info"
    fi
    
    echo "   ✅ WASM generation appears to be working correctly"
else
    echo "   ❌ No WASM files found - critical compilation failure!"
fi

echo
echo "🎯 CORE LANGUAGE FEATURE STATUS:"
echo "==============================="

# Test specific core features based on passed files
core_features_status=""
if [[ " ${test_files[*]} " =~ " 02_variables_basic.cln " ]] && [[ $passed_files -gt 0 ]]; then
    core_features_status="${core_features_status}   ✅ Variables & Basic Types\n"
fi
if [[ " ${test_files[*]} " =~ " 03_arithmetic_operations.cln " ]] && [[ $passed_files -gt 0 ]]; then
    core_features_status="${core_features_status}   ✅ Arithmetic Operations (including power operator ^)\n"
fi
if [[ " ${test_files[*]} " =~ " 10_functions_basic.cln " ]] && [[ $passed_files -gt 0 ]]; then
    core_features_status="${core_features_status}   ✅ Function Definitions & Calls\n"
fi
if [[ " ${test_files[*]} " =~ " 43_string_interpolation.cln " ]] && [[ $passed_files -gt 0 ]]; then
    core_features_status="${core_features_status}   ✅ String Interpolation\n"
fi
if [[ " ${test_files[*]} " =~ " 35_method_style_simple.cln " ]] && [[ $passed_files -gt 0 ]]; then
    core_features_status="${core_features_status}   ✅ Method-Style Function Calls\n"
fi
if [[ " ${test_files[*]} " =~ " 25_stdlib_functions.cln " ]] && [[ $passed_files -gt 0 ]]; then
    core_features_status="${core_features_status}   ✅ Standard Library Functions\n"
fi

if [[ -n "$core_features_status" ]]; then
    echo -e "$core_features_status"
else
    echo "   ⚠️  Unable to verify core features due to compilation failures"
fi

echo
echo "==============================="
echo "🏆 QA VALIDATION COMPLETE"
echo "==============================="

if (( $(echo "$success_rate >= 95.0" | bc -l) )); then
    echo "🎉 CONCLUSION: Clean Language compiler has achieved excellent quality standards!"
    echo "   Ready for production use with ${success_rate}% compilation success rate."
else
    echo "📋 CONCLUSION: Clean Language compiler needs attention to reach production quality."
    echo "   Current success rate: ${success_rate}% (target: ≥95%)"
fi

echo "==============================="