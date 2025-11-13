#!/bin/bash

# Recompile all test files

echo "Recompiling all test files..."
echo ""

success=0
failed=0

for cln_file in tests/cln/**/*.cln; do
    if [ ! -f "$cln_file" ]; then
        continue
    fi

    filename=$(basename "$cln_file" .cln)
    output_file="tests/output/${filename}.wasm"

    echo -n "Compiling $filename... "

    if cargo run --release --bin cln -- compile -i "$cln_file" -o "$output_file" > /dev/null 2>&1; then
        echo "✅"
        ((success++))
    else
        echo "❌"
        ((failed++))
    fi
done

echo ""
echo "Summary:"
echo "✅ Success: $success"
echo "❌ Failed: $failed"
