#!/bin/bash

# Test specific files that are likely to fail based on complex features
cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

echo "=== TESTING COMPLEX FEATURE FILES ==="

# Test files with specific features
test_files=(
    "tests/clean_files/40_onerror_test.cln"
    "tests/clean_files/debug_conditional.cln"
    "tests/clean_files/47_string_interpolation.cln"
    "tests/clean_files/test_method_chaining.cln"
    "tests/clean_files/test_inheritance.cln"
    "tests/clean_files/test_static_methods.cln"
)

for file in "${test_files[@]}"; do
    if [ -f "$file" ]; then
        echo ""
        echo "--- Testing: $file ---"
        if cargo run --bin clean-language-compiler --features="runtime" compile -i "$file" -o /tmp/test.wasm 2>&1 | grep -q "Successfully compiled"; then
            echo "✅ PASS"
        else
            echo "❌ FAIL - Details:"
            cargo run --bin clean-language-compiler --features="runtime" compile -i "$file" -o /tmp/test.wasm 2>&1 | grep -E "(Parse error|Semantic error|Error [0-9]+:|Syntax error)" | head -3
        fi
    else
        echo "File not found: $file"
    fi
done