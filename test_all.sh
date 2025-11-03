#!/bin/bash

# Test all .cln files and measure success rate
# Usage: ./test_all.sh

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

# Arrays to track failures
declare -a compilation_failures
declare -a validation_failures

echo "=========================================="
echo "COMPREHENSIVE COMPILER TEST SUITE"
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

    # Try to compile
    if "$COMPILER" compile -i "$file" -o "$output_file" > /dev/null 2>&1; then
        ((compiled++))

        # Try to validate
        if wasm-validate "$output_file" > /dev/null 2>&1; then
            ((validated++))
        else
            ((failed_validation++))
            validation_failures+=("$file")
        fi
    else
        ((failed_compilation++))
        compilation_failures+=("$file")
    fi

    # Progress indicator
    if ((total % 10 == 0)); then
        echo -ne "\rProcessed: $total files..."
    fi
done < <(find "$TEST_DIR" -name "*.cln" -type f | sort)

echo -e "\n"
echo "=========================================="
echo "TEST RESULTS"
echo "=========================================="
echo "Total files:              $total"
echo "Compiled successfully:    $compiled ($(awk "BEGIN {printf \"%.1f\", ($compiled/$total)*100}")%)"
echo "Validated successfully:   $validated ($(awk "BEGIN {printf \"%.1f\", ($validated/$total)*100}")%)"
echo "Failed compilation:       $failed_compilation"
echo "Failed validation:        $failed_validation"
echo ""

if [ $failed_compilation -gt 0 ]; then
    echo "Compilation failures (first 10):"
    for ((i=0; i<10 && i<${#compilation_failures[@]}; i++)); do
        echo "  - ${compilation_failures[$i]}"
    done
    if [ ${#compilation_failures[@]} -gt 10 ]; then
        echo "  ... and $((${#compilation_failures[@]} - 10)) more"
    fi
    echo ""
fi

if [ $failed_validation -gt 0 ]; then
    echo "Validation failures (first 10):"
    for ((i=0; i<10 && i<${#validation_failures[@]}; i++)); do
        echo "  - ${validation_failures[$i]}"
    done
    if [ ${#validation_failures[@]} -gt 10 ]; then
        echo "  ... and $((${#validation_failures[@]} - 10)) more"
    fi
fi

echo ""
echo "=========================================="
