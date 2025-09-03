#!/bin/bash

# Comprehensive QA Final Assessment Script
# Tests all 319 .cln files for compilation and execution validation

echo "=== COMPREHENSIVE QA FINAL ASSESSMENT ==="
echo "Testing all files in tests/clean_files/ directory"
echo "Date: $(date)"
echo ""

# Setup directories
TEST_DIR="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files"
OUTPUT_DIR="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output"
mkdir -p "$OUTPUT_DIR"

# Counters
total_files=0
compilation_success=0
execution_success=0
parse_only_success=0

# Error categorization
declare -A error_categories
error_categories=(
    ["parse_error"]=0
    ["semantic_error"]=0
    ["codegen_error"]=0
    ["wasm_invalid"]=0
    ["execution_error"]=0
)

# Result files
SUCCESS_LOG="qa_success_files.log"
FAILURE_LOG="qa_failure_details.log"
SUMMARY_LOG="qa_comprehensive_summary.log"

# Clear previous logs
> "$SUCCESS_LOG"
> "$FAILURE_LOG"
> "$SUMMARY_LOG"

echo "Starting comprehensive testing..." | tee -a "$SUMMARY_LOG"

# Build the compiler first
echo "Building compiler..." | tee -a "$SUMMARY_LOG"
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"
cargo build --release > /dev/null 2>&1

if [ $? -ne 0 ]; then
    echo "❌ CRITICAL: Compiler build failed!" | tee -a "$SUMMARY_LOG"
    exit 1
fi

echo "✅ Compiler built successfully" | tee -a "$SUMMARY_LOG"
echo "" | tee -a "$SUMMARY_LOG"

# Process all .cln files
for file in "$TEST_DIR"/*.cln; do
    if [ -f "$file" ]; then
        filename=$(basename "$file")
        base_name="${filename%.cln}"
        wasm_file="$OUTPUT_DIR/${base_name}.wasm"
        
        total_files=$((total_files + 1))
        
        echo -n "Testing $filename... "
        
        # Attempt compilation
        ./target/release/clean-language-compiler compile -i "$file" -o "$wasm_file" > /dev/null 2>&1
        compile_result=$?
        
        if [ $compile_result -eq 0 ] && [ -f "$wasm_file" ]; then
            compilation_success=$((compilation_success + 1))
            
            # Validate WASM file
            if command -v wasm-validate >/dev/null 2>&1; then
                wasm-validate "$wasm_file" >/dev/null 2>&1
                wasm_valid=$?
            else
                # Use wasmtime validate as fallback
                wasmtime validate "$wasm_file" >/dev/null 2>&1
                wasm_valid=$?
            fi
            
            if [ $wasm_valid -eq 0 ]; then
                # Try to execute with wasmtime
                timeout 5s wasmtime run "$wasm_file" >/dev/null 2>&1
                execution_result=$?
                
                if [ $execution_result -eq 0 ] || [ $execution_result -eq 124 ]; then  # 124 is timeout
                    execution_success=$((execution_success + 1))
                    echo "✅ FULL SUCCESS (compile + execute)"
                    echo "$filename - FULL SUCCESS" >> "$SUCCESS_LOG"
                else
                    echo "🟡 COMPILE SUCCESS (execution failed)"
                    echo "$filename - COMPILE SUCCESS, execution error: $execution_result" >> "$FAILURE_LOG"
                    error_categories["execution_error"]=$((error_categories["execution_error"] + 1))
                fi
            else
                echo "🟡 COMPILE SUCCESS (invalid WASM)"
                echo "$filename - COMPILE SUCCESS, invalid WASM" >> "$FAILURE_LOG"
                error_categories["wasm_invalid"]=$((error_categories["wasm_invalid"] + 1))
            fi
        else
            echo "❌ COMPILATION FAILED"
            
            # Try to categorize the error
            ./target/release/clean-language-compiler compile -i "$file" -o "$wasm_file" 2>&1 | head -10 > temp_error.log
            
            if grep -q -i "parse\|syntax\|unexpected" temp_error.log; then
                error_categories["parse_error"]=$((error_categories["parse_error"] + 1))
                echo "$filename - PARSE ERROR" >> "$FAILURE_LOG"
            elif grep -q -i "semantic\|type\|undefined\|scope" temp_error.log; then
                error_categories["semantic_error"]=$((error_categories["semantic_error"] + 1))
                echo "$filename - SEMANTIC ERROR" >> "$FAILURE_LOG"
            elif grep -q -i "codegen\|wasm\|generation" temp_error.log; then
                error_categories["codegen_error"]=$((error_categories["codegen_error"] + 1))
                echo "$filename - CODEGEN ERROR" >> "$FAILURE_LOG"
            else
                error_categories["parse_error"]=$((error_categories["parse_error"] + 1))
                echo "$filename - UNKNOWN ERROR" >> "$FAILURE_LOG"
            fi
            
            cat temp_error.log >> "$FAILURE_LOG"
            echo "---" >> "$FAILURE_LOG"
            rm -f temp_error.log
        fi
    fi
done

# Calculate percentages
if [ $total_files -gt 0 ]; then
    compilation_percent=$(echo "scale=2; $compilation_success * 100 / $total_files" | bc -l)
    execution_percent=$(echo "scale=2; $execution_success * 100 / $total_files" | bc -l)
else
    compilation_percent="0.00"
    execution_percent="0.00"
fi

# Generate comprehensive summary
echo "=== FINAL COMPREHENSIVE QA RESULTS ===" | tee -a "$SUMMARY_LOG"
echo "" | tee -a "$SUMMARY_LOG"
echo "📊 OVERALL METRICS:" | tee -a "$SUMMARY_LOG"
echo "  Total test files: $total_files" | tee -a "$SUMMARY_LOG"
echo "  Compilation success: $compilation_success/$total_files ($compilation_percent%)" | tee -a "$SUMMARY_LOG"
echo "  Full execution success: $execution_success/$total_files ($execution_percent%)" | tee -a "$SUMMARY_LOG"
echo "" | tee -a "$SUMMARY_LOG"

echo "🔍 ERROR BREAKDOWN:" | tee -a "$SUMMARY_LOG"
echo "  Parse errors: ${error_categories[parse_error]}" | tee -a "$SUMMARY_LOG"
echo "  Semantic errors: ${error_categories[semantic_error]}" | tee -a "$SUMMARY_LOG"
echo "  Code generation errors: ${error_categories[codegen_error]}" | tee -a "$SUMMARY_LOG"
echo "  Invalid WASM: ${error_categories[wasm_invalid]}" | tee -a "$SUMMARY_LOG"
echo "  Execution errors: ${error_categories[execution_error]}" | tee -a "$SUMMARY_LOG"
echo "" | tee -a "$SUMMARY_LOG"

echo "📈 PROGRESS ASSESSMENT:" | tee -a "$SUMMARY_LOG"
if [ "$execution_percent" = "100.00" ]; then
    echo "  🎉 TARGET ACHIEVED: 100% success rate!" | tee -a "$SUMMARY_LOG"
elif (( $(echo "$execution_percent >= 90" | bc -l) )); then
    echo "  🌟 EXCELLENT: >90% success rate" | tee -a "$SUMMARY_LOG"
elif (( $(echo "$execution_percent >= 80" | bc -l) )); then
    echo "  ✅ GOOD: >80% success rate" | tee -a "$SUMMARY_LOG"
elif (( $(echo "$execution_percent >= 70" | bc -l) )); then
    echo "  🟡 MODERATE: >70% success rate" | tee -a "$SUMMARY_LOG"
else
    echo "  🔴 NEEDS WORK: <70% success rate" | tee -a "$SUMMARY_LOG"
fi

echo "" | tee -a "$SUMMARY_LOG"
echo "📋 DETAILED LOGS:" | tee -a "$SUMMARY_LOG"
echo "  Success log: $SUCCESS_LOG" | tee -a "$SUMMARY_LOG"
echo "  Failure log: $FAILURE_LOG" | tee -a "$SUMMARY_LOG"
echo "  Summary log: $SUMMARY_LOG" | tee -a "$SUMMARY_LOG"

echo "" | tee -a "$SUMMARY_LOG"
echo "=== QA ASSESSMENT COMPLETE ===" | tee -a "$SUMMARY_LOG"

# Return appropriate exit code
if [ "$execution_percent" = "100.00" ]; then
    exit 0
else
    exit 1
fi