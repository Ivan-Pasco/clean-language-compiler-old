#!/bin/bash

# Comprehensive QA Evaluation Script for Clean Language Compiler
# Executes all test files and provides detailed success/failure analysis

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Clean Language Compiler - Comprehensive QA Evaluation ===${NC}"
echo -e "${BLUE}Testing all files in tests/clean_files/${NC}"
echo ""

# Create output directories
mkdir -p tests/output
mkdir -p tests/qa_results

# Initialize counters
total_tests=0
successful_tests=0
failed_tests=0

# Initialize result files
success_file="tests/qa_results/successful_tests.txt"
failure_file="tests/qa_results/failed_tests.txt"
detailed_log="tests/qa_results/detailed_evaluation.log"

> "$success_file"
> "$failure_file"
> "$detailed_log"

echo "=== COMPREHENSIVE QA EVALUATION $(date) ===" >> "$detailed_log"
echo "" >> "$detailed_log"

# Test all .cln files
for test_file in tests/clean_files/*.cln; do
    if [ -f "$test_file" ]; then
        filename=$(basename "$test_file" .cln)
        output_file="tests/output/${filename}.wasm"
        
        total_tests=$((total_tests + 1))
        
        printf "Testing %-50s ... " "$filename"
        
        # Run compilation (without timeout on macOS)
        if cargo run --bin clean-language-compiler compile -i "$test_file" -o "$output_file" >> "$detailed_log" 2>&1; then
            if [ -f "$output_file" ]; then
                echo -e "${GREEN}PASS${NC}"
                echo "$filename" >> "$success_file"
                successful_tests=$((successful_tests + 1))
                echo "✅ PASS: $filename" >> "$detailed_log"
            else
                echo -e "${RED}FAIL (no output)${NC}"
                echo "$filename" >> "$failure_file"
                failed_tests=$((failed_tests + 1))
                echo "❌ FAIL: $filename (no WASM output generated)" >> "$detailed_log"
            fi
        else
            echo -e "${RED}FAIL${NC}"
            echo "$filename" >> "$failure_file"
            failed_tests=$((failed_tests + 1))
            echo "❌ FAIL: $filename (compilation error)" >> "$detailed_log"
        fi
        
        echo "---" >> "$detailed_log"
    fi
done

# Calculate success rate
if [ $total_tests -gt 0 ]; then
    success_rate=$(echo "scale=2; $successful_tests * 100 / $total_tests" | bc -l)
else
    success_rate=0
fi

# Generate summary
echo ""
echo -e "${BLUE}=== QA EVALUATION RESULTS ===${NC}"
echo -e "Total Tests:      ${YELLOW}$total_tests${NC}"
echo -e "Successful:       ${GREEN}$successful_tests${NC}"
echo -e "Failed:           ${RED}$failed_tests${NC}"
echo -e "Success Rate:     ${YELLOW}$success_rate%${NC}"
echo ""

# Previous baseline comparison
baseline_rate="80.87"
baseline_tests="258"
if (( $(echo "$success_rate > $baseline_rate" | bc -l) )); then
    improvement=$(echo "scale=2; $success_rate - $baseline_rate" | bc -l)
    echo -e "📈 ${GREEN}IMPROVEMENT: +$improvement% over baseline ($baseline_rate%)${NC}"
else
    decline=$(echo "scale=2; $baseline_rate - $success_rate" | bc -l)
    echo -e "📉 ${RED}DECLINE: -$decline% from baseline ($baseline_rate%)${NC}"
fi

# Progress toward 100% target
remaining=$(echo "scale=2; 100 - $success_rate" | bc -l)
remaining_tests=$(echo "$total_tests - $successful_tests" | bc -l)
echo -e "🎯 ${YELLOW}Remaining to 100%: $remaining% ($remaining_tests tests)${NC}"

echo ""
echo -e "${BLUE}Results saved to:${NC}"
echo -e "  • Successful tests: $success_file"
echo -e "  • Failed tests: $failure_file" 
echo -e "  • Detailed log: $detailed_log"

# Write summary to log
echo "" >> "$detailed_log"
echo "=== FINAL SUMMARY ===" >> "$detailed_log"
echo "Total Tests: $total_tests" >> "$detailed_log"
echo "Successful: $successful_tests" >> "$detailed_log"
echo "Failed: $failed_tests" >> "$detailed_log"
echo "Success Rate: $success_rate%" >> "$detailed_log"
echo "Baseline Comparison: $baseline_rate% -> $success_rate%" >> "$detailed_log"
echo "Tests Remaining for 100%: $remaining_tests" >> "$detailed_log"