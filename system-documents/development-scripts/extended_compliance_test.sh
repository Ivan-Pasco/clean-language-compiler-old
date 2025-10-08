#!/bin/bash
passed=0
failed=0
total=0

echo "Extended Compiler Compliance Test"  
echo "Testing remaining files to find more wins..."
echo "================================="

for file in tests/clean_files/*.cln; do
    total=$((total+1))
    filename=$(basename "$file")
    
    if [ $total -gt 50 ] && [ $total -le 100 ]; then  # Test files 51-100
        if timeout 5s cargo run --bin clean-language-compiler --features runtime compile -i "$file" -o "/tmp/test_$total.wasm" 2>/dev/null >/dev/null; then
            passed=$((passed+1))
            echo "✅ $filename"
        else
            failed=$((failed+1))
            echo "❌ $filename"
        fi
    fi
done

echo ""
echo "RESULTS (files 51-100):"
echo "✅ Passed: $passed"
echo "❌ Failed: $failed" 
if [ $((passed + failed)) -gt 0 ]; then
    echo "📊 Success Rate: $((passed * 100 / (passed + failed)))%"
fi

