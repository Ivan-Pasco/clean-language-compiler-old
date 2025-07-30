#!/bin/bash

cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

# Array to store results
declare -a results=()
declare -a errors=()

echo "Compiling all Clean Language test files..."
echo "=========================================="

for file in tests/clean_files/*.cln; do
    filename=$(basename "$file" .cln)
    echo "Testing: $filename"
    
    # Attempt compilation and capture output
    if output=$(cargo run --bin clean-language-compiler compile -i "$file" -o "tests/wasm/${filename}.wasm" 2>&1); then
        if echo "$output" | grep -q "Successfully compiled"; then
            echo "✅ $filename - SUCCESS"
            results+=("✅ $filename - SUCCESS")
        else
            echo "❌ $filename - FAILED (no success message)"
            results+=("❌ $filename - FAILED (no success message)")
            errors+=("$filename: No success message")
        fi
    else
        echo "❌ $filename - FAILED"
        results+=("❌ $filename - FAILED")
        # Extract error message
        error_msg=$(echo "$output" | grep -E "Error:|error:" | head -1)
        if [ -z "$error_msg" ]; then
            error_msg="Compilation failed"
        fi
        errors+=("$filename: $error_msg")
    fi
    echo
done

echo "=========================================="
echo "SUMMARY:"
echo "=========================================="
for result in "${results[@]}"; do
    echo "$result"
done

if [ ${#errors[@]} -gt 0 ]; then
    echo
    echo "ERRORS FOUND:"
    echo "============="
    for error in "${errors[@]}"; do
        echo "$error"
    done
fi