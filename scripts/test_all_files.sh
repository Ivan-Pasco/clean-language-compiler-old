#!/bin/bash

# Comprehensive test script for all Clean Language files
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

echo "Running comprehensive validation on all Clean Language test files..."
echo "Total files to test: $(find tests/clean_files -name "*.cln" | wc -l | xargs)"
echo "=========================================="

passed=0
failed=0
total=0

# Create output directory if it doesn't exist
mkdir -p tests/wasm

for file in tests/clean_files/*.cln; do
    if [[ -f "$file" ]]; then
        total=$((total + 1))
        filename=$(basename "$file" .cln)
        output_file="tests/wasm/${filename}.wasm"
        
        echo -n "Testing $(basename "$file")... "
        
        # Compile the file with timeout to prevent hanging
        if timeout 30s cargo run --bin clean-language-compiler -- compile -i "$file" -o "$output_file" >/dev/null 2>&1; then
            if [[ -f "$output_file" ]] && [[ -s "$output_file" ]]; then
                echo "✓ PASS"
                passed=$((passed + 1))
            else
                echo "✗ FAIL (no output)"
                failed=$((failed + 1))
            fi
        else
            echo "✗ FAIL (compilation error)"
            failed=$((failed + 1))
        fi
    fi
done

echo "=========================================="
echo "Test Results:"
echo "  Total files: $total"
echo "  Passed: $passed"
echo "  Failed: $failed"
echo "  Success rate: $(echo "scale=1; $passed * 100 / $total" | bc -l)%"
echo "=========================================="

if [[ $failed -gt 0 ]]; then
    echo "Failed files analysis:"
    for file in tests/clean_files/*.cln; do
        if [[ -f "$file" ]]; then
            filename=$(basename "$file" .cln)
            output_file="tests/wasm/${filename}.wasm"
            
            if ! timeout 30s cargo run --bin clean-language-compiler -- compile -i "$file" -o "$output_file" >/dev/null 2>&1 || [[ ! -f "$output_file" ]] || [[ ! -s "$output_file" ]]; then
                echo "  - $(basename "$file")"
            fi
        fi
    done
fi