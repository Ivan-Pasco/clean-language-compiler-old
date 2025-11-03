#!/bin/bash

# Analyze all compilation and validation errors
# Usage: ./analyze_errors.sh

COMPILER="target/release/clean-language-compiler"
TEST_DIR="tests/cln"
OUTPUT_DIR="tests/output"
ERROR_LOG="error_analysis.txt"

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"
> "$ERROR_LOG"

# Counters
total=0
compiled=0
validated=0

echo "=========================================="
echo "COMPREHENSIVE ERROR ANALYSIS"
echo "=========================================="
echo ""
echo "Analyzing all test files..."

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

        # Try to validate and capture output
        validate_output=$(wasm-validate "$output_file" 2>&1)
        validate_result=$?

        if [ $validate_result -eq 0 ]; then
            ((validated++))
        else
            # Log validation error
            echo "=== VALIDATION ERROR ===" >> "$ERROR_LOG"
            echo "File: $file" >> "$ERROR_LOG"
            echo "$validate_output" >> "$ERROR_LOG"
            echo "" >> "$ERROR_LOG"
        fi
    else
        # Log compilation error
        echo "=== COMPILATION ERROR ===" >> "$ERROR_LOG"
        echo "File: $file" >> "$ERROR_LOG"
        echo "$compile_output" | tail -20 >> "$ERROR_LOG"
        echo "" >> "$ERROR_LOG"
    fi

    # Progress indicator
    if ((total % 10 == 0)); then
        echo -ne "\rProcessed: $total files..."
    fi
done < <(find "$TEST_DIR" -name "*.cln" -type f | sort)

echo -e "\n"
echo "=========================================="
echo "RESULTS"
echo "=========================================="
echo "Total files:              $total"
echo "Compiled successfully:    $compiled ($(echo "scale=1; $compiled * 100 / $total" | bc)%)"
echo "Validated successfully:   $validated ($(echo "scale=1; $validated * 100 / $total" | bc)%)"
echo "Failed compilation:       $((total - compiled))"
echo "Failed validation:        $((compiled - validated))"
echo ""
echo "Detailed errors logged to: $ERROR_LOG"
echo ""

# Analyze error patterns
echo "=========================================="
echo "ERROR PATTERN ANALYSIS"
echo "=========================================="
echo ""

echo "Top compilation error types:"
grep "Error:" "$ERROR_LOG" | sed 's/.*Error: //' | sort | uniq -c | sort -rn | head -10

echo ""
echo "Top validation error types:"
grep "error:" "$ERROR_LOG" | sed 's/.*error: //' | cut -d: -f1 | sort | uniq -c | sort -rn | head -10

echo ""
echo "=========================================="
