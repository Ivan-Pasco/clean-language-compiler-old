#!/bin/bash
success=0
fail=0
for file in $(find tests/cln -name "*.cln" -type f); do
    if ./target/release/clean-language-compiler compile -i "$file" -o "tests/output/$(basename "$file" .cln).wasm" 2>&1 | grep -q "Successfully compiled"; then
        success=$((success+1))
    else
        fail=$((fail+1))
    fi
done
echo "Success: $success, Failed: $fail, Total: $((success+fail))"
echo "Success rate: $(echo "scale=2; $success * 100 / ($success + $fail)" | bc)%"
