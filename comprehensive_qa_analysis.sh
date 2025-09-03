#!/bin/bash

# Comprehensive QA Analysis Script for Clean Language Compiler
# Tests all .cln files and categorizes errors for systematic analysis

CLEAN_FILES_DIR="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files"
OUTPUT_DIR="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/output"
RESULTS_FILE="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/qa_analysis_results.txt"
ERROR_PATTERNS_FILE="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/error_patterns.txt"

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

# Initialize counters
TOTAL_TESTS=0
SUCCESSFUL_TESTS=0
FAILED_TESTS=0

# Initialize error pattern tracking
declare -A ERROR_PATTERNS
declare -A ERROR_FILES

echo "=== COMPREHENSIVE QA ANALYSIS $(date) ===" > "$RESULTS_FILE"
echo "Testing Clean Language Compiler with all test files..." >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"

echo "" > "$ERROR_PATTERNS_FILE"

# Function to categorize errors
categorize_error() {
    local error_msg="$1"
    local file="$2"
    
    if [[ $error_msg == *"unexpected token"* ]]; then
        ERROR_PATTERNS["PARSER_UNEXPECTED_TOKEN"]=$((${ERROR_PATTERNS["PARSER_UNEXPECTED_TOKEN"]} + 1))
        ERROR_FILES["PARSER_UNEXPECTED_TOKEN"]+="$file "
    elif [[ $error_msg == *"expected"* ]]; then
        ERROR_PATTERNS["PARSER_EXPECTED_TOKEN"]=$((${ERROR_PATTERNS["PARSER_EXPECTED_TOKEN"]} + 1))
        ERROR_FILES["PARSER_EXPECTED_TOKEN"]+="$file "
    elif [[ $error_msg == *"undefined function"* || $error_msg == *"unknown function"* ]]; then
        ERROR_PATTERNS["UNDEFINED_FUNCTION"]=$((${ERROR_PATTERNS["UNDEFINED_FUNCTION"]} + 1))
        ERROR_FILES["UNDEFINED_FUNCTION"]+="$file "
    elif [[ $error_msg == *"undefined variable"* || $error_msg == *"unknown variable"* ]]; then
        ERROR_PATTERNS["UNDEFINED_VARIABLE"]=$((${ERROR_PATTERNS["UNDEFINED_VARIABLE"]} + 1))
        ERROR_FILES["UNDEFINED_VARIABLE"]+="$file "
    elif [[ $error_msg == *"type mismatch"* || $error_msg == *"type error"* ]]; then
        ERROR_PATTERNS["TYPE_ERROR"]=$((${ERROR_PATTERNS["TYPE_ERROR"]} + 1))
        ERROR_FILES["TYPE_ERROR"]+="$file "
    elif [[ $error_msg == *"semantic"* ]]; then
        ERROR_PATTERNS["SEMANTIC_ERROR"]=$((${ERROR_PATTERNS["SEMANTIC_ERROR"]} + 1))
        ERROR_FILES["SEMANTIC_ERROR"]+="$file "
    elif [[ $error_msg == *"codegen"* || $error_msg == *"wasm"* ]]; then
        ERROR_PATTERNS["CODEGEN_ERROR"]=$((${ERROR_PATTERNS["CODEGEN_ERROR"]} + 1))
        ERROR_FILES["CODEGEN_ERROR"]+="$file "
    elif [[ $error_msg == *"stdlib"* || $error_msg == *"builtin"* ]]; then
        ERROR_PATTERNS["STDLIB_MISSING"]=$((${ERROR_PATTERNS["STDLIB_MISSING"]} + 1))
        ERROR_FILES["STDLIB_MISSING"]+="$file "
    else
        ERROR_PATTERNS["OTHER_ERROR"]=$((${ERROR_PATTERNS["OTHER_ERROR"]} + 1))
        ERROR_FILES["OTHER_ERROR"]+="$file "
    fi
}

# Test each .cln file
echo "Running comprehensive tests..."
for cln_file in "$CLEAN_FILES_DIR"/*.cln; do
    if [[ -f "$cln_file" ]]; then
        TOTAL_TESTS=$((TOTAL_TESTS + 1))
        
        filename=$(basename "$cln_file" .cln)
        output_file="$OUTPUT_DIR/${filename}.wasm"
        
        echo -n "Testing $filename... "
        
        # Run compilation
        cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"
        result=$(cargo run --bin clean-language-compiler compile -i "$cln_file" -o "$output_file" 2>&1)
        exit_code=$?
        
        if [[ $exit_code -eq 0 && -f "$output_file" ]]; then
            echo "✅ SUCCESS"
            SUCCESSFUL_TESTS=$((SUCCESSFUL_TESTS + 1))
            echo "SUCCESS: $filename" >> "$RESULTS_FILE"
        else
            echo "❌ FAILED"
            FAILED_TESTS=$((FAILED_TESTS + 1))
            echo "FAILED: $filename" >> "$RESULTS_FILE"
            echo "  Error: $result" >> "$RESULTS_FILE"
            echo "" >> "$RESULTS_FILE"
            
            # Categorize the error
            categorize_error "$result" "$filename"
            
            # Add to detailed error log
            echo "=== ERROR in $filename ===" >> "$ERROR_PATTERNS_FILE"
            echo "$result" >> "$ERROR_PATTERNS_FILE"
            echo "" >> "$ERROR_PATTERNS_FILE"
        fi
    fi
done

# Calculate success rate
SUCCESS_RATE=$(echo "scale=2; $SUCCESSFUL_TESTS * 100 / $TOTAL_TESTS" | bc -l)

echo "" >> "$RESULTS_FILE"
echo "=== SUMMARY STATISTICS ===" >> "$RESULTS_FILE"
echo "Total Tests: $TOTAL_TESTS" >> "$RESULTS_FILE"
echo "Successful: $SUCCESSFUL_TESTS" >> "$RESULTS_FILE"
echo "Failed: $FAILED_TESTS" >> "$RESULTS_FILE"
echo "Success Rate: $SUCCESS_RATE%" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"

echo "=== ERROR PATTERN ANALYSIS ===" >> "$RESULTS_FILE"
for pattern in "${!ERROR_PATTERNS[@]}"; do
    count=${ERROR_PATTERNS[$pattern]}
    echo "$pattern: $count occurrences" >> "$RESULTS_FILE"
    echo "  Files: ${ERROR_FILES[$pattern]}" >> "$RESULTS_FILE"
    echo "" >> "$RESULTS_FILE"
done

echo ""
echo "=== QA ANALYSIS COMPLETE ==="
echo "Total Tests: $TOTAL_TESTS"
echo "Successful: $SUCCESSFUL_TESTS"
echo "Failed: $FAILED_TESTS"
echo "Success Rate: $SUCCESS_RATE%"
echo ""
echo "Detailed results saved to: $RESULTS_FILE"
echo "Error patterns saved to: $ERROR_PATTERNS_FILE"