#!/bin/bash

# Test all .cln files and categorize errors
# Usage: ./test_with_errors.sh

COMPILER="target/release/clean-language-compiler"
TEST_DIR="tests/cln"
OUTPUT_DIR="tests/output"

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

# Counters
total=0
compiled=0
validated=0
failed_compilation=0
failed_validation=0
function_index_errors=0

# Arrays to track specific error types
declare -a function_index_error_files

echo "=========================================="
echo "TESTING FOR FUNCTION INDEX ERRORS"
echo "=========================================="
echo ""

# Find all .cln files
while IFS= read -r file; do
    ((total++))

    # Get relative path for output
    rel_path="${file#$TEST_DIR/}"
    output_file="$OUTPUT_DIR/${rel_path%.cln}.wasm"

    # Create output directory structure
    output_dir=$(dirname "$output_file")
    mkdir -p "$output_dir"

    # Try to compile and capture output
    compile_output=$("$COMPILER" compile -i "$file" -o "$output_file" 2>&1)
    compile_result=$?

    if [ $compile_result -eq 0 ]; then
        ((compiled++))

        # Try to validate
        if wasm-validate "$output_file" > /dev/null 2>&1; then
            ((validated++))
        else
            ((failed_validation++))
        fi
    else
        ((failed_compilation++))

        # Check for function index mismatch error
        if echo "$compile_output" | grep -q "pre-registered at index.*but generated at index"; then
            ((function_index_errors++))
            function_index_error_files+=("$file")
        fi
    fi

    # Progress indicator
    if ((total % 10 == 0)); then
        echo -ne "\rProcessed: $total files..."
    fi
done < <(find "$TEST_DIR" -name "*.cln" -type f | sort)

echo -e "\n"
echo "=========================================="
echo "FUNCTION INDEX ERROR ANALYSIS"
echo "=========================================="
echo "Total files:                       $total"
echo "Compiled successfully:             $compiled"
echo "Failed compilation:                $failed_compilation"
echo "Function index mismatch errors:    $function_index_errors"
echo ""

if [ $function_index_errors -gt 0 ]; then
    echo "Files with function index errors:"
    for file in "${function_index_error_files[@]}"; do
        echo "  - $file"
    done
else
    echo "NO FUNCTION INDEX MISMATCH ERRORS FOUND!"
    echo "This issue has been completely resolved."
fi

echo ""
echo "=========================================="
