#!/bin/bash

echo "=== COMPREHENSIVE QA FINAL EVALUATION ==="
echo "Date: $(date)"
echo "Testing Infrastructure Fixes Impact"
echo

# Counters
total_tests=0
compilation_success=0  
execution_success=0
compilation_failures=0
execution_failures=0

# Create output directories
mkdir -p tests/wasm
mkdir -p qa_evaluation_results

# Track results in detail
success_log="qa_evaluation_results/final_success_list.txt"
compilation_failure_log="qa_evaluation_results/final_compilation_failures.txt"
execution_failure_log="qa_evaluation_results/final_execution_failures.txt"
error_patterns_log="qa_evaluation_results/final_error_patterns.txt"

# Clear previous logs
> "$success_log"
> "$compilation_failure_log" 
> "$execution_failure_log"
> "$error_patterns_log"

echo "Phase 1: Compilation Testing"
echo "============================"

for file in tests/clean_files/*.cln; do
    if [[ -f "$file" ]]; then
        basename=$(basename "$file" .cln)
        wasm_output="tests/wasm/${basename}.wasm"
        
        total_tests=$((total_tests + 1))
        
        # Attempt compilation
        if cargo run --bin clean-language-compiler compile -i "$file" -o "$wasm_output" 2>/dev/null >/dev/null; then
            compilation_success=$((compilation_success + 1))
            echo "✅ COMPILE: $basename"
            echo "$basename" >> "$success_log"
        else
            compilation_failures=$((compilation_failures + 1))
            echo "❌ COMPILE: $basename"
            echo "$basename" >> "$compilation_failure_log"
            
            # Capture error for pattern analysis
            error_output=$(cargo run --bin clean-language-compiler compile -i "$file" -o "$wasm_output" 2>&1)
            echo "=== $basename ===" >> "$error_patterns_log"
            echo "$error_output" >> "$error_patterns_log"
            echo "" >> "$error_patterns_log"
        fi
    fi
done

compilation_rate=$(echo "scale=2; $compilation_success * 100 / $total_tests" | bc)
echo
echo "COMPILATION RESULTS:"
echo "Total Tests: $total_tests"
echo "Compilation Successes: $compilation_success"
echo "Compilation Failures: $compilation_failures"  
echo "Compilation Success Rate: ${compilation_rate}%"
echo

echo "Phase 2: Execution Testing"
echo "=========================="

for wasm_file in tests/wasm/*.wasm; do
    if [[ -f "$wasm_file" ]]; then
        basename=$(basename "$wasm_file" .wasm)
        
        # Test WASM execution with wasmtime
        if timeout 10s wasmtime "$wasm_file" 2>/dev/null >/dev/null; then
            execution_success=$((execution_success + 1))
            echo "✅ EXECUTE: $basename"
        else
            execution_failures=$((execution_failures + 1))
            echo "❌ EXECUTE: $basename"  
            echo "$basename" >> "$execution_failure_log"
        fi
    fi
done

if [[ $compilation_success -gt 0 ]]; then
    execution_rate=$(echo "scale=2; $execution_success * 100 / $compilation_success" | bc)
    overall_success_rate=$(echo "scale=2; $execution_success * 100 / $total_tests" | bc)
else
    execution_rate=0.00
    overall_success_rate=0.00
fi

echo
echo "EXECUTION RESULTS:"
echo "Compiled Programs: $compilation_success" 
echo "Execution Successes: $execution_success"
echo "Execution Failures: $execution_failures"
echo "Execution Success Rate: ${execution_rate}%"
echo
echo "OVERALL RESULTS:"
echo "================"
echo "Total Tests: $total_tests"
echo "End-to-End Successes: $execution_success"
echo "Overall Success Rate: ${overall_success_rate}%"

# Calculate improvement from baseline
baseline=11.28
if [[ $overall_success_rate != "0.00" ]]; then
    improvement=$(echo "scale=2; $overall_success_rate - $baseline" | bc)
    echo "Improvement from Baseline (11.28%): +${improvement}%"
fi

echo
echo "Result files saved in qa_evaluation_results/"
echo "- final_success_list.txt: Successfully executed tests"
echo "- final_compilation_failures.txt: Compilation failures" 
echo "- final_execution_failures.txt: Execution failures"
echo "- final_error_patterns.txt: Detailed error analysis"

