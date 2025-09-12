#!/bin/bash

# Comprehensive Clean Language Compilation Test
# Tests all .cln files individually to avoid file locking issues

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TESTS_DIR="$SCRIPT_DIR/tests/clean_files"
OUTPUT_DIR="$SCRIPT_DIR/tests/output"
LOGS_DIR="$SCRIPT_DIR/tests/logs"
RESULTS_DIR="$SCRIPT_DIR/tests/results"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Clean Language Comprehensive Compilation Test ===${NC}"
echo "Test files: $TESTS_DIR"
echo "Output directory: $OUTPUT_DIR"
echo "Logs directory: $LOGS_DIR"
echo ""

# Initialize counters and arrays
total_files=0
successful_compilations=0
failed_compilations=0
declare -a compilation_failures=()
declare -a parser_errors=()
declare -a semantic_errors=()
declare -a codegen_errors=()

# Create results summary file
SUMMARY_FILE="$RESULTS_DIR/compilation_summary_$(date +%Y%m%d_%H%M%S).txt"
echo "Clean Language Comprehensive Compilation Test" > "$SUMMARY_FILE"
echo "Started: $(date)" >> "$SUMMARY_FILE"
echo "=========================================" >> "$SUMMARY_FILE"
echo "" >> "$SUMMARY_FILE"

# Function to categorize error type
categorize_error() {
    local error_log="$1"
    if grep -q -E "(Expected|parsing failed|grammar|syntax)" "$error_log"; then
        return 1  # Parser error
    elif grep -q -E "(type|semantic|undefined|symbol)" "$error_log"; then
        return 2  # Semantic error
    elif grep -q -E "(codegen|wasm|generation)" "$error_log"; then
        return 3  # Codegen error
    else
        return 0  # Other/unknown error
    fi
}

echo "Processing test files..."
echo ""

# Test each .cln file individually
for cln_file in "$TESTS_DIR"/*.cln; do
    if [ ! -f "$cln_file" ]; then
        continue
    fi
    
    filename=$(basename "$cln_file")
    filename_no_ext="${filename%.*}"
    wasm_output="$OUTPUT_DIR/${filename_no_ext}.wasm"
    compile_log="$LOGS_DIR/${filename_no_ext}_compile.log"
    
    total_files=$((total_files + 1))
    
    echo -n "[$total_files] Testing: $filename... "
    
    # Compile the file
    if cargo run --release --bin clean-language-compiler compile -i "$cln_file" -o "$wasm_output" >"$compile_log" 2>&1; then
        echo -e "${GREEN}✓ SUCCESS${NC}"
        successful_compilations=$((successful_compilations + 1))
        echo "SUCCESS: $filename" >> "$SUMMARY_FILE"
    else
        echo -e "${RED}✗ FAILED${NC}"
        failed_compilations=$((failed_compilations + 1))
        compilation_failures+=("$filename")
        
        # Categorize the error
        categorize_error "$compile_log"
        error_type=$?
        
        case $error_type in
            1)
                parser_errors+=("$filename")
                echo "PARSER ERROR: $filename" >> "$SUMMARY_FILE"
                ;;
            2)
                semantic_errors+=("$filename")
                echo "SEMANTIC ERROR: $filename" >> "$SUMMARY_FILE"
                ;;
            3)
                codegen_errors+=("$filename")
                echo "CODEGEN ERROR: $filename" >> "$SUMMARY_FILE"
                ;;
            *)
                echo "OTHER ERROR: $filename" >> "$SUMMARY_FILE"
                ;;
        esac
        
        # Add first few lines of error to summary
        echo "  Error details:" >> "$SUMMARY_FILE"
        head -3 "$compile_log" | sed 's/^/    /' >> "$SUMMARY_FILE"
        echo "" >> "$SUMMARY_FILE"
    fi
done

# Calculate statistics
success_rate=$((successful_compilations * 100 / total_files))

echo ""
echo -e "${BLUE}=== COMPILATION RESULTS ===${NC}"
echo -e "Total files tested: ${YELLOW}$total_files${NC}"
echo -e "Successful compilations: ${GREEN}$successful_compilations${NC}"
echo -e "Failed compilations: ${RED}$failed_compilations${NC}"
echo -e "Success rate: ${YELLOW}${success_rate}%${NC}"

echo ""
echo -e "${BLUE}=== ERROR BREAKDOWN ===${NC}"
echo -e "Parser errors: ${RED}${#parser_errors[@]}${NC}"
echo -e "Semantic errors: ${RED}${#semantic_errors[@]}${NC}"
echo -e "Codegen errors: ${RED}${#codegen_errors[@]}${NC}"

# Write summary to file
echo "" >> "$SUMMARY_FILE"
echo "FINAL STATISTICS:" >> "$SUMMARY_FILE"
echo "=================" >> "$SUMMARY_FILE"
echo "Total files: $total_files" >> "$SUMMARY_FILE"
echo "Successful: $successful_compilations" >> "$SUMMARY_FILE"
echo "Failed: $failed_compilations" >> "$SUMMARY_FILE"
echo "Success rate: ${success_rate}%" >> "$SUMMARY_FILE"
echo "" >> "$SUMMARY_FILE"
echo "ERROR BREAKDOWN:" >> "$SUMMARY_FILE"
echo "Parser errors: ${#parser_errors[@]}" >> "$SUMMARY_FILE"
echo "Semantic errors: ${#semantic_errors[@]}" >> "$SUMMARY_FILE"
echo "Codegen errors: ${#codegen_errors[@]}" >> "$SUMMARY_FILE"
echo "" >> "$SUMMARY_FILE"
echo "Completed: $(date)" >> "$SUMMARY_FILE"

if [ $failed_compilations -gt 0 ]; then
    echo ""
    echo -e "${YELLOW}Failed files details saved to: $SUMMARY_FILE${NC}"
    echo -e "${YELLOW}Individual error logs in: $LOGS_DIR${NC}"
fi

echo ""
echo -e "${BLUE}Compilation test completed!${NC}"

# Set exit code based on results
if [ $failed_compilations -eq 0 ]; then
    exit 0
else
    exit 1
fi