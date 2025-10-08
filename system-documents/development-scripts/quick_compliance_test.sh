#!/bin/bash
passed=0
failed=0
total=0

echo "Quick Compiler Compliance Test"
echo "================================"

for file in tests/clean_files/*.cln; do
    total=$((total+1))
    filename=$(basename "$file")
    
    if [ $total -le 50 ]; then  # Test first 50 files to avoid long runtime
        if cargo run --bin clean-language-compiler --features runtime compile -i "$file" -o "/tmp/test_$total.wasm" 2>/dev/null >/dev/null; then
            passed=$((passed+1))
            echo "✅ $filename"
        else
            failed=$((failed+1))
            echo "❌ $filename"
        fi
    fi
done

echo ""
echo "RESULTS (first 50 files):"
echo "✅ Passed: $passed"
echo "❌ Failed: $failed"
echo "📊 Success Rate: $((passed * 100 / (passed + failed)))%"
echo "🎯 Remaining to 100%: $failed files need fixes"

