#!/bin/bash

# Impact Measurement Script for Instance Method Resolution Fix
# Baseline: 79.93% (255/319 tests passing)

set -e

CLEAN_FILES_DIR="tests/clean_files"
OUTPUT_DIR="tests/output"
RESULTS_FILE="impact_measurement_results.txt"
SUCCESS_LIST="success_list.txt"
FAILURE_LIST="failure_list.txt"
ERROR_ANALYSIS="error_analysis.txt"

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

echo "=== IMPACT MEASUREMENT: Instance Method Resolution Fix ===" > "$RESULTS_FILE"
echo "Baseline: 79.93% (255/319 tests passing)" >> "$RESULTS_FILE"
echo "Testing all 319 Clean files..." >> "$RESULTS_FILE"
echo "Started: $(date)" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"

# Initialize counters
success_count=0
failure_count=0
total_count=0

# Clear previous results
> "$SUCCESS_LIST"
> "$FAILURE_LIST" 
> "$ERROR_ANALYSIS"

echo "Processing all Clean files..."

# Process all .cln files
for cln_file in "$CLEAN_FILES_DIR"/*.cln; do
    if [[ -f "$cln_file" ]]; then
        filename=$(basename "$cln_file")
        base_name="${filename%.cln}"
        output_file="$OUTPUT_DIR/${base_name}.wasm"
        
        total_count=$((total_count + 1))
        
        echo "Testing $filename ($total_count/319)..."
        
        # Compile the file and capture output
        if timeout 10s cargo run --bin clean-language-compiler compile -i "$cln_file" -o "$output_file" 2>&1 | tee temp_output.txt; then
            # Check if compilation actually succeeded (WASM file created)
            if [[ -f "$output_file" ]]; then
                echo "$filename" >> "$SUCCESS_LIST"
                success_count=$((success_count + 1))
                echo "✅ $filename"
            else
                echo "$filename" >> "$FAILURE_LIST"
                failure_count=$((failure_count + 1))
                echo "❌ $filename (no WASM output)"
                echo "=== ERROR: $filename ===" >> "$ERROR_ANALYSIS"
                cat temp_output.txt >> "$ERROR_ANALYSIS"
                echo "" >> "$ERROR_ANALYSIS"
            fi
        else
            echo "$filename" >> "$FAILURE_LIST" 
            failure_count=$((failure_count + 1))
            echo "❌ $filename"
            echo "=== ERROR: $filename ===" >> "$ERROR_ANALYSIS"
            cat temp_output.txt >> "$ERROR_ANALYSIS"
            echo "" >> "$ERROR_ANALYSIS"
        fi
        
        # Clean up temp file
        rm -f temp_output.txt
    fi
done

# Calculate success rate
success_rate=$(echo "scale=2; $success_count * 100 / $total_count" | bc -l)
baseline_rate=79.93
improvement=$(echo "scale=2; $success_rate - $baseline_rate" | bc -l)
improvement_tests=$((success_count - 255))

# Write summary
echo "" >> "$RESULTS_FILE"
echo "=== FINAL RESULTS ===" >> "$RESULTS_FILE"
echo "Total files tested: $total_count" >> "$RESULTS_FILE"
echo "Successful compilations: $success_count" >> "$RESULTS_FILE"
echo "Failed compilations: $failure_count" >> "$RESULTS_FILE"
echo "Success rate: $success_rate%" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"
echo "=== IMPACT ANALYSIS ===" >> "$RESULTS_FILE"
echo "Baseline success rate: $baseline_rate%" >> "$RESULTS_FILE"
echo "New success rate: $success_rate%" >> "$RESULTS_FILE"
echo "Improvement: $improvement percentage points" >> "$RESULTS_FILE"
echo "Additional tests passing: $improvement_tests" >> "$RESULTS_FILE"
echo "Completed: $(date)" >> "$RESULTS_FILE"

# Display summary
echo ""
echo "=== IMPACT MEASUREMENT COMPLETE ==="
echo "Total files: $total_count"
echo "Successful: $success_count"
echo "Failed: $failure_count" 
echo "Success rate: $success_rate%"
echo ""
echo "=== IMPACT ANALYSIS ==="
echo "Baseline: $baseline_rate%"
echo "New rate: $success_rate%"
echo "Improvement: $improvement percentage points"
echo "Additional tests: $improvement_tests"
echo ""
echo "Results saved to: $RESULTS_FILE"
echo "Success list: $SUCCESS_LIST"
echo "Failure list: $FAILURE_LIST"
echo "Error analysis: $ERROR_ANALYSIS"