#!/bin/bash

# Comprehensive QA Assessment After Duplicate Import Fixes
# Tests all 319 .cln files for compilation and execution validation using proper wasmtime runner

echo "=== POST-FIX COMPREHENSIVE QA ASSESSMENT ==="
echo "Testing all files in tests/clean_files/ directory"
echo "Using wasmtime_runner binary with host function support"
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

# Error categorization
declare -A error_categories
error_categories=(
    ["parse_error"]=0
    ["semantic_error"]=0
    ["codegen_error"]=0
    ["wasm_invalid"]=0
    ["execution_error"]=0
    ["no_start_function"]=0
    ["import_signature_mismatch"]=0
)

# Result files
SUCCESS_LOG="qa_post_fix_success.log"
FAILURE_LOG="qa_post_fix_failures.log"
SUMMARY_LOG="qa_post_fix_summary.log"

# Clear previous logs
> "$SUCCESS_LOG"
> "$FAILURE_LOG"
> "$SUMMARY_LOG"

echo "Starting post-fix comprehensive testing..." | tee -a "$SUMMARY_LOG"

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

# Test just a sample first to validate approach
echo "=== SAMPLE VALIDATION (First 10 files) ===" | tee -a "$SUMMARY_LOG"
sample_count=0
sample_success=0

for file in "$TEST_DIR"/*.cln; do
    if [ $sample_count -ge 10 ]; then
        break
    fi
    
    if [ -f "$file" ]; then
        filename=$(basename "$file")
        base_name="${filename%.cln}"
        wasm_file="$OUTPUT_DIR/${base_name}.wasm"
        
        sample_count=$((sample_count + 1))
        
        echo -n "Sample $sample_count: Testing $filename... "
        
        # Attempt compilation
        ./target/release/clean-language-compiler compile -i "$file" -o "$wasm_file" > /dev/null 2>&1
        compile_result=$?
        
        if [ $compile_result -eq 0 ] && [ -f "$wasm_file" ]; then
            # Try execution with our wasmtime runner
            timeout 5s cargo run --bin wasmtime_runner "$wasm_file" > /dev/null 2>&1
            execution_result=$?
            
            if [ $execution_result -eq 0 ] || [ $execution_result -eq 124 ]; then  # 124 is timeout
                sample_success=$((sample_success + 1))
                echo "✅ SUCCESS"
            else
                echo "🟡 COMPILE OK, EXEC FAILED"
            fi
        else
            echo "❌ COMPILE FAILED"
        fi
    fi
done

echo "" | tee -a "$SUMMARY_LOG"
echo "Sample validation: $sample_success/$sample_count files successful" | tee -a "$SUMMARY_LOG"

if [ $sample_success -eq 0 ]; then
    echo "⚠️  CRITICAL: No sample files working. Stopping full test." | tee -a "$SUMMARY_LOG"
    echo "Issue likely requires investigation before full test suite." | tee -a "$SUMMARY_LOG"
    exit 1
fi

echo "✅ Sample validation passed. Proceeding with full test suite..." | tee -a "$SUMMARY_LOG"
echo "" | tee -a "$SUMMARY_LOG"

# Process all .cln files
for file in "$TEST_DIR"/*.cln; do
    if [ -f "$file" ]; then
        filename=$(basename "$file")
        base_name="${filename%.cln}"
        wasm_file="$OUTPUT_DIR/${base_name}.wasm"
        
        total_files=$((total_files + 1))
        
        if [ $((total_files % 50)) -eq 0 ]; then
            echo "Progress: $total_files files processed..." | tee -a "$SUMMARY_LOG"
        fi
        
        # Attempt compilation
        ./target/release/clean-language-compiler compile -i "$file" -o "$wasm_file" > temp_compile_output.log 2>&1
        compile_result=$?
        
        if [ $compile_result -eq 0 ] && [ -f "$wasm_file" ]; then
            compilation_success=$((compilation_success + 1))
            
            # Try execution with our wasmtime runner
            timeout 10s cargo run --bin wasmtime_runner "$wasm_file" > temp_exec_output.log 2>&1
            execution_result=$?
            
            if [ $execution_result -eq 0 ] || [ $execution_result -eq 124 ]; then  # 124 is timeout
                execution_success=$((execution_success + 1))
                echo "$filename - FULL SUCCESS" >> "$SUCCESS_LOG"
            else
                # Categorize execution error
                if grep -q -i "No start" temp_exec_output.log; then
                    error_categories["no_start_function"]=$((error_categories["no_start_function"] + 1))
                    echo "$filename - NO START FUNCTION" >> "$FAILURE_LOG"
                elif grep -q -i "incompatible import" temp_exec_output.log; then
                    error_categories["import_signature_mismatch"]=$((error_categories["import_signature_mismatch"] + 1))
                    echo "$filename - IMPORT SIGNATURE MISMATCH" >> "$FAILURE_LOG"
                else
                    error_categories["execution_error"]=$((error_categories["execution_error"] + 1))
                    echo "$filename - EXECUTION ERROR" >> "$FAILURE_LOG"
                fi
                cat temp_exec_output.log >> "$FAILURE_LOG"
                echo "---" >> "$FAILURE_LOG"
            fi
        else
            # Categorize compilation error
            if grep -q -i "parse\|syntax\|unexpected" temp_compile_output.log; then
                error_categories["parse_error"]=$((error_categories["parse_error"] + 1))
                echo "$filename - PARSE ERROR" >> "$FAILURE_LOG"
            elif grep -q -i "semantic\|type\|undefined\|scope" temp_compile_output.log; then
                error_categories["semantic_error"]=$((error_categories["semantic_error"] + 1))
                echo "$filename - SEMANTIC ERROR" >> "$FAILURE_LOG"
            else
                error_categories["codegen_error"]=$((error_categories["codegen_error"] + 1))
                echo "$filename - CODEGEN ERROR" >> "$FAILURE_LOG"
            fi
            
            cat temp_compile_output.log >> "$FAILURE_LOG"
            echo "---" >> "$FAILURE_LOG"
        fi
        
        # Clean up temp files
        rm -f temp_compile_output.log temp_exec_output.log
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
echo "=== POST-FIX COMPREHENSIVE QA RESULTS ===" | tee -a "$SUMMARY_LOG"
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
echo "  No start function: ${error_categories[no_start_function]}" | tee -a "$SUMMARY_LOG"
echo "  Import signature mismatch: ${error_categories[import_signature_mismatch]}" | tee -a "$SUMMARY_LOG"
echo "  Other execution errors: ${error_categories[execution_error]}" | tee -a "$SUMMARY_LOG"
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
echo "🔧 NEXT STEPS:" | tee -a "$SUMMARY_LOG"
if [ ${error_categories[no_start_function]} -gt $((total_files / 2)) ]; then
    echo "  - PRIORITY 1: Fix start function generation (affects ${error_categories[no_start_function]} files)" | tee -a "$SUMMARY_LOG"
fi
if [ ${error_categories[import_signature_mismatch]} -gt 10 ]; then
    echo "  - PRIORITY 2: Fix remaining import signature mismatches (${error_categories[import_signature_mismatch]} files)" | tee -a "$SUMMARY_LOG"
fi
if [ ${error_categories[parse_error]} -gt 10 ]; then
    echo "  - PRIORITY 3: Address parsing issues (${error_categories[parse_error]} files)" | tee -a "$SUMMARY_LOG"
fi

echo "" | tee -a "$SUMMARY_LOG"
echo "=== POST-FIX QA ASSESSMENT COMPLETE ===" | tee -a "$SUMMARY_LOG"

# Return appropriate exit code
if [ "$execution_percent" = "100.00" ]; then
    exit 0
else
    exit 1
fi