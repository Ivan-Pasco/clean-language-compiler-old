#!/bin/bash
echo "=== PHASE 2: COMPREHENSIVE TESTING AND FIXING PROCEDURE ==="
echo "Systematic testing of ALL 274 Clean Language test files"
echo "Date: $(date)"
echo

# Initialize counters and files
total_files=0
passed_files=0
failed_files=0
failure_report="phase2_failure_analysis.txt"
success_list="phase2_success_list.txt"

# Clear previous reports
> "$failure_report"
> "$success_list"

echo "PHASE 2 - COMPREHENSIVE FAILURE ANALYSIS" >> "$failure_report"
echo "=========================================" >> "$failure_report"
echo "Date: $(date)" >> "$failure_report"
echo >> "$failure_report"

echo "PHASE 2 - SUCCESS LIST" >> "$success_list"
echo "=====================" >> "$success_list"
echo "Date: $(date)" >> "$success_list"
echo >> "$success_list"

echo "Testing all .cln files in tests/clean_files directory..."
echo "Format: [PASS/FAIL] filename (running total)"
echo

# Test each file systematically
for file in tests/clean_files/*.cln; do
    if [ -f "$file" ]; then
        total_files=$((total_files + 1))
        filename=$(basename "$file")
        
        # Test compilation - capture both stdout and stderr
        if cargo run --bin clean-language-compiler compile -i "$file" -o "/tmp/phase2_test_$(basename "$file" .cln).wasm" &>/dev/null; then
            passed_files=$((passed_files + 1))
            echo "[PASS] $filename ($passed_files/$total_files)"
            echo "$filename" >> "$success_list"
        else
            failed_files=$((failed_files + 1))
            echo "[FAIL] $filename ($passed_files/$total_files)"
            
            # Capture detailed error for analysis
            echo "FILE: $filename" >> "$failure_report"
            echo "FULL ERROR OUTPUT:" >> "$failure_report"
            cargo run --bin clean-language-compiler compile -i "$file" -o "/tmp/phase2_test_$(basename "$file" .cln).wasm" 2>&1 | grep -A 10 -B 2 "Error:" >> "$failure_report"
            echo "----------------------------------------" >> "$failure_report"
            echo >> "$failure_report"
        fi
        
        # Progress indicator every 25 files
        if [ $((total_files % 25)) -eq 0 ]; then
            echo "  ... Progress: $total_files files tested, $passed_files passed, $failed_files failed"
        fi
    fi
done

echo
echo "=== PHASE 2 COMPREHENSIVE TEST RESULTS ==="
echo "Total files tested: $total_files"
echo "Successfully compiled: $passed_files"
echo "Failed to compile: $failed_files"
echo "SUCCESS RATE: $(( passed_files * 100 / total_files ))%"
echo

# Analyze error patterns
echo "=== ERROR PATTERN ANALYSIS ==="
if [ $failed_files -gt 0 ]; then
    echo "Top 10 most common error patterns:"
    grep "Error:" "$failure_report" | sed 's/Error: [^{]*{ context: ErrorContext { message: "//' | sed 's/".*//' | sort | uniq -c | sort -nr | head -10
    echo
    echo "Error type distribution:"
    grep "error_type:" "$failure_report" | sed 's/.*error_type: //' | sed 's/,.*//' | sort | uniq -c | sort -nr
else
    echo "🎉 ALL FILES PASSED! Perfect success rate!"
fi

echo
echo "Detailed reports saved to:"
echo "  - Failures: $failure_report"
echo "  - Successes: $success_list"
echo
echo "Phase 2 baseline testing complete."