#!/bin/bash
# Validate all compiled WASM files

echo "=== WASM Validation Results (Fresh Compilation) ==="

total=0
valid=0
invalid=0

for wasm_file in tests/output/*.wasm; do
    if [ -f "$wasm_file" ]; then
        ((total++))
        if wasm-validate "$wasm_file" 2>/dev/null; then
            ((valid++))
        else
            ((invalid++))
        fi
    fi
done

echo "Total files: $total"
echo "Valid: $valid"
echo "Invalid: $invalid"

if [ $total -gt 0 ]; then
    percentage=$((valid * 100 / total))
    echo "Success rate: ${percentage}%"
fi
