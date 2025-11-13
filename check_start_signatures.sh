#!/bin/bash

# Check _start function signatures in all compiled WASM files

echo "Checking _start signatures in compiled WASM files..."
echo ""

correct=0
wrong=0
missing=0

for wasm_file in tests/output/*.wasm; do
    if [ ! -f "$wasm_file" ]; then
        continue
    fi

    filename=$(basename "$wasm_file")

    # Get the _start function signature
    start_func=$(wasm-objdump -x "$wasm_file" 2>&1 | grep "_start" | head -1)

    if [ -z "$start_func" ]; then
        echo "❌ $filename: No _start function found"
        ((missing++))
        continue
    fi

    # Extract function index and signature index
    func_index=$(echo "$start_func" | sed -n 's/.*func\[\([0-9]*\)\].*/\1/p')
    sig_index=$(echo "$start_func" | sed -n 's/.*sig=\([0-9]*\).*/\1/p')

    if [ -z "$sig_index" ]; then
        echo "❌ $filename: Could not extract signature index"
        ((missing++))
        continue
    fi

    # Get the actual type signature
    type_sig=$(wasm-objdump -x "$wasm_file" 2>&1 | grep "type\[$sig_index\]")

    # Check if it's () -> nil
    if echo "$type_sig" | grep -q "() -> nil"; then
        echo "✅ $filename: Correct signature - $type_sig"
        ((correct++))
    else
        echo "❌ $filename: WRONG signature - $type_sig"
        ((wrong++))
    fi
done

echo ""
echo "Summary:"
echo "✅ Correct: $correct"
echo "❌ Wrong: $wrong"
echo "⚠️  Missing: $missing"
