#!/bin/bash

# Final Comprehensive QA Evaluation Script
# Evaluates all 319 Clean Language test files for production readiness

echo "🚀 FINAL COMPREHENSIVE QA EVALUATION - Clean Language Compiler"
echo "================================================================"
echo "Target: 100% Success Rate (319/319 tests)"
echo "Previous Status: 82.13% (262/319) before critical WASM fixes"
echo ""

# Initialize counters
TOTAL_TESTS=0
SUCCESS_COUNT=0
COMPILE_FAILURES=0
WASM_VALIDATION_FAILURES=0
EXECUTION_FAILURES=0

# Create output directories
mkdir -p tests/output
mkdir -p tests/qa_results

# Results files
SUCCESS_FILE="tests/qa_results/final_success_list.txt"
FAILURE_FILE="tests/qa_results/final_failure_list.txt"
DETAILED_LOG="tests/qa_results/final_comprehensive_log.txt"

# Clear previous results
> "$SUCCESS_FILE"
> "$FAILURE_FILE"
> "$DETAILED_LOG"

echo "🔍 Phase 1: Compilation and WASM Generation" | tee -a "$DETAILED_LOG"
echo "============================================" | tee -a "$DETAILED_LOG"

# Test each Clean Language file
for cln_file in tests/clean_files/*.cln; do
    if [[ -f "$cln_file" ]]; then
        filename=$(basename "$cln_file" .cln)
        wasm_file="tests/output/${filename}.wasm"
        
        TOTAL_TESTS=$((TOTAL_TESTS + 1))
        
        echo -n "Testing [$TOTAL_TESTS/319] $filename... " | tee -a "$DETAILED_LOG"
        
        # Phase 1: Compilation
        if cargo run --bin clean-language-compiler compile -i "$cln_file" -o "$wasm_file" > /dev/null 2>&1; then
            
            # Phase 2: WASM Validation
            if command -v wasm-validate >/dev/null 2>&1; then
                if wasm-validate "$wasm_file" > /dev/null 2>&1; then
                    
                    # Phase 3: Execution Test (if possible)
                    if cargo run --bin wasmtime_runner "$wasm_file" > /dev/null 2>&1; then
                        echo "✅ SUCCESS (Compile + Validate + Execute)" | tee -a "$DETAILED_LOG"
                        echo "$filename" >> "$SUCCESS_FILE"
                        SUCCESS_COUNT=$((SUCCESS_COUNT + 1))
                    else
                        echo "❌ EXECUTION FAILURE" | tee -a "$DETAILED_LOG"
                        echo "$filename - EXECUTION_FAILURE" >> "$FAILURE_FILE"
                        EXECUTION_FAILURES=$((EXECUTION_FAILURES + 1))
                    fi
                else
                    echo "❌ WASM VALIDATION FAILURE" | tee -a "$DETAILED_LOG"
                    echo "$filename - WASM_VALIDATION_FAILURE" >> "$FAILURE_FILE"
                    WASM_VALIDATION_FAILURES=$((WASM_VALIDATION_FAILURES + 1))
                fi
            else
                # Skip WASM validation if tool not available
                if cargo run --bin wasmtime_runner "$wasm_file" > /dev/null 2>&1; then
                    echo "✅ SUCCESS (Compile + Execute)" | tee -a "$DETAILED_LOG"
                    echo "$filename" >> "$SUCCESS_FILE"
                    SUCCESS_COUNT=$((SUCCESS_COUNT + 1))
                else
                    echo "❌ EXECUTION FAILURE" | tee -a "$DETAILED_LOG"
                    echo "$filename - EXECUTION_FAILURE" >> "$FAILURE_FILE"
                    EXECUTION_FAILURES=$((EXECUTION_FAILURES + 1))
                fi
            fi
        else
            echo "❌ COMPILE FAILURE" | tee -a "$DETAILED_LOG"
            echo "$filename - COMPILE_FAILURE" >> "$FAILURE_FILE"
            COMPILE_FAILURES=$((COMPILE_FAILURES + 1))
        fi
    fi
done

echo "" | tee -a "$DETAILED_LOG"
echo "📊 FINAL COMPREHENSIVE QA RESULTS" | tee -a "$DETAILED_LOG"
echo "==================================" | tee -a "$DETAILED_LOG"

# Calculate success rate
SUCCESS_RATE=$(echo "scale=2; $SUCCESS_COUNT * 100 / $TOTAL_TESTS" | bc -l)

echo "Total Tests: $TOTAL_TESTS" | tee -a "$DETAILED_LOG"
echo "Successful: $SUCCESS_COUNT" | tee -a "$DETAILED_LOG"
echo "Success Rate: ${SUCCESS_RATE}%" | tee -a "$DETAILED_LOG"
echo "" | tee -a "$DETAILED_LOG"
echo "Failure Breakdown:" | tee -a "$DETAILED_LOG"
echo "  Compilation Failures: $COMPILE_FAILURES" | tee -a "$DETAILED_LOG"
echo "  WASM Validation Failures: $WASM_VALIDATION_FAILURES" | tee -a "$DETAILED_LOG"
echo "  Execution Failures: $EXECUTION_FAILURES" | tee -a "$DETAILED_LOG"
echo "  Total Failures: $((TOTAL_TESTS - SUCCESS_COUNT))" | tee -a "$DETAILED_LOG"

echo "" | tee -a "$DETAILED_LOG"
echo "🎯 PRODUCTION READINESS ASSESSMENT" | tee -a "$DETAILED_LOG"
echo "===================================" | tee -a "$DETAILED_LOG"

if (( $(echo "$SUCCESS_RATE >= 100" | bc -l) )); then
    echo "🏆 STATUS: PRODUCTION READY - 100% Success Rate Achieved!" | tee -a "$DETAILED_LOG"
elif (( $(echo "$SUCCESS_RATE >= 95" | bc -l) )); then
    echo "🟢 STATUS: NEAR PRODUCTION READY - ${SUCCESS_RATE}% Success Rate" | tee -a "$DETAILED_LOG"
elif (( $(echo "$SUCCESS_RATE >= 90" | bc -l) )); then
    echo "🟡 STATUS: HIGH QUALITY - ${SUCCESS_RATE}% Success Rate" | tee -a "$DETAILED_LOG"
elif (( $(echo "$SUCCESS_RATE >= 80" | bc -l) )); then
    echo "🟠 STATUS: GOOD PROGRESS - ${SUCCESS_RATE}% Success Rate" | tee -a "$DETAILED_LOG"
else
    echo "🔴 STATUS: NEEDS IMPROVEMENT - ${SUCCESS_RATE}% Success Rate" | tee -a "$DETAILED_LOG"
fi

echo "" | tee -a "$DETAILED_LOG"
echo "📁 Results saved to:" | tee -a "$DETAILED_LOG"
echo "  Success List: $SUCCESS_FILE" | tee -a "$DETAILED_LOG"
echo "  Failure List: $FAILURE_FILE" | tee -a "$DETAILED_LOG"
echo "  Detailed Log: $DETAILED_LOG" | tee -a "$DETAILED_LOG"

echo ""
echo "Final Success Rate: ${SUCCESS_RATE}% ($SUCCESS_COUNT/$TOTAL_TESTS)"