#!/bin/bash

# Test first 10 files and capture failures
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

echo "=== TESTING FIRST 10 FILES TO FIND FAILURES ==="
count=0
for file in tests/clean_files/*.cln; do
    if [ $count -ge 10 ]; then
        break
    fi
    echo ""
    echo "--- Testing: $file ---"
    if cargo run --bin clean-language-compiler --features="runtime" compile -i "$file" -o /tmp/test.wasm 2>&1 | grep -q "Successfully compiled"; then
        echo "✅ PASS"
    else
        echo "❌ FAIL - Details:"
        cargo run --bin clean-language-compiler --features="runtime" compile -i "$file" -o /tmp/test.wasm 2>&1 | grep -E "(Parse error|Semantic error|Error [0-9]+:|Syntax error)" | head -2
    fi
    count=$((count + 1))
done