#!/bin/bash
echo "=== Clean Language Compiler - Systematic Failure Analysis ==="
echo "Testing all .cln files and identifying specific failures..."
echo

failed_count=0
total_count=0
failures_file="test_failures_report.txt"

# Clear previous report
> "$failures_file"

echo "FAILING TEST FILES ANALYSIS" >> "$failures_file"
echo "=========================" >> "$failures_file"
echo >> "$failures_file"

for file in tests/clean_files/*.cln; do
    if [ -f "$file" ]; then
        total_count=$((total_count + 1))
        filename=$(basename "$file")
        
        # Test compilation
        cargo run --bin clean-language-compiler compile -i "$file" -o "/tmp/test_$(basename "$file" .cln).wasm" 2>&1 > /tmp/compile_output.txt
        compile_result=$?
        
        if [ $compile_result -ne 0 ]; then
            failed_count=$((failed_count + 1))
            echo "FAIL [$failed_count]: $filename"
            
            # Extract error details
            echo "FILE: $filename" >> "$failures_file"
            echo "ERROR:" >> "$failures_file"
            cat /tmp/compile_output.txt | grep -A 3 -B 1 "Error:" >> "$failures_file"
            echo "----------------------------------------" >> "$failures_file"
            echo >> "$failures_file"
        else
            echo "PASS: $filename"
        fi
    fi
done

echo
echo "=== SUMMARY ==="
echo "Total files tested: $total_count"
echo "Failed files: $failed_count"
echo "Success rate: $(( (total_count - failed_count) * 100 / total_count ))%"
echo "Detailed failure report saved to: $failures_file"

if [ $failed_count -gt 0 ]; then
    echo
    echo "=== TOP FAILURE PATTERNS ==="
    echo "Most common errors:"
    grep "Error:" "$failures_file" | sort | uniq -c | sort -nr | head -5
fi