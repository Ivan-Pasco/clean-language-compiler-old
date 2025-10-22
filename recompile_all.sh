#!/bin/bash
# Recompile all .cln test files

echo "=== Recompiling all .cln files with fixed compiler ==="

for cln_file in tests/cln/**/*.cln; do
    if [ -f "$cln_file" ]; then
        # Get basename without extension
        basename=$(basename "$cln_file" .cln)

        # Compile to tests/output
        cargo run --release --bin clean-language-compiler compile \
            -i "$cln_file" \
            -o "tests/output/${basename}.wasm" 2>&1 | grep -q "Successfully compiled" && echo -n "." || echo -n "x"
    fi
done

echo ""
echo "Compilation complete"
