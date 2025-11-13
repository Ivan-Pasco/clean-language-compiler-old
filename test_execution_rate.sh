#!/bin/bash

# Test execution rate of all recompiled WASM files

echo "Testing execution of recompiled WASM files..."
echo ""

success=0
failed=0
no_start=0

for wasm_file in $(find tests/output -name "*.wasm" -newer src/codegen/type_manager.rs); do
    filename=$(basename "$wasm_file")

    # Check if it has _start
    start_func=$(wasm-objdump -x "$wasm_file" 2>&1 | grep "_start" | head -1)

    if [ -z "$start_func" ]; then
        ((no_start++))
        continue
    fi

    # Try to execute
    if cargo run --release --bin wasmtime_runner -- "$wasm_file" > /dev/null 2>&1; then
        echo "✅ $filename"
        ((success++))
    else
        echo "❌ $filename"
        ((failed++))
    fi
done

echo ""
echo "Summary:"
echo "✅ Executed successfully: $success"
echo "❌ Execution failed: $failed"
echo "⚠️  No _start function: $no_start"
echo ""
echo "Execution rate: $success / $(($success + $failed)) = $(awk "BEGIN {printf \"%.1f%%\", 100*$success/($success+$failed)}")"
