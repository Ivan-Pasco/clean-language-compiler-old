#!/bin/bash

# Test a sample of Clean Language files to verify compilation success
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

echo "Testing sample of Clean Language files..."
echo "=========================================="

# Test basic files to check core functionality
test_files=(
    "00_minimal.cln"
    "01_hello_world.cln"
    "02_variables_basic.cln"
    "03_arithmetic_operations.cln"
    "10_functions_basic.cln"
    "25_stdlib_functions.cln"
    "35_method_style_simple.cln"
    "43_string_interpolation.cln"
    "49_static_method_calls_simple.cln"
)

passed=0
failed=0

# Create output directory if it doesn't exist
mkdir -p tests/wasm

for file in "${test_files[@]}"; do
    if [[ -f "tests/clean_files/$file" ]]; then
        filename=$(basename "$file" .cln)
        output_file="tests/wasm/${filename}.wasm"
        
        echo -n "Testing $file... "
        
        # Remove existing output file
        rm -f "$output_file"
        
        # Compile the file with longer timeout
        if timeout 120s cargo run --bin clean-language-compiler -- compile -i "tests/clean_files/$file" -o "$output_file" >/dev/null 2>&1; then
            if [[ -f "$output_file" ]] && [[ -s "$output_file" ]]; then
                echo "✓ PASS ($(stat -f%z "$output_file") bytes)"
                passed=$((passed + 1))
            else
                echo "✗ FAIL (no output file)"
                failed=$((failed + 1))
            fi
        else
            echo "✗ FAIL (compilation timeout/error)"
            failed=$((failed + 1))
        fi
    else
        echo "File tests/clean_files/$file not found"
        failed=$((failed + 1))
    fi
done

echo "=========================================="
echo "Sample Test Results:"
echo "  Tested: $((passed + failed)) files"
echo "  Passed: $passed"
echo "  Failed: $failed"
if [[ $((passed + failed)) -gt 0 ]]; then
    echo "  Success rate: $(echo "scale=1; $passed * 100 / ($passed + $failed)" | bc -l)%"
fi
echo "=========================================="