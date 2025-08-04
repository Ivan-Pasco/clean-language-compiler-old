#\!/bin/bash

echo "Running comprehensive test suite on all 96 Clean files..."

# Colors for output
RED="[0;31m"
GREEN="[0;32m"
YELLOW="[1;33m"
NC="[0m" # No Color

PASSED=0
FAILED=0
TOTAL=0

cd "/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"

# Ensure output directory exists
mkdir -p tests/wasm

# Find all .cln files and sort them
for file in $(find tests/clean_files -name "*.cln" | sort); do
    TOTAL=$((TOTAL + 1))
    basename=$(basename "$file" .cln)
    output="tests/wasm/${basename}.wasm"
    
    echo -n "Testing $basename: "
    
    # Run the compiler
    if cargo run --bin clean-language-compiler compile -i "$file" -o "$output" 2>/dev/null >/dev/null; then
        echo -e "${GREEN}✓${NC}"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗${NC}"
        FAILED=$((FAILED + 1))
        echo "    Error details for $basename:"
        cargo run --bin clean-language-compiler compile -i "$file" -o "$output" 2>&1 | head -10 | sed "s/^/    /"
        echo ""
    fi
done

echo ""
echo "========================================="
echo "TEST RESULTS:"
echo "Passed: $PASSED"
echo "Failed: $FAILED" 
echo "Total: $TOTAL"
echo "Success Rate: $(echo "scale=1; $PASSED * 100 / $TOTAL" | bc)%"
echo "========================================="

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}🎉 ALL TESTS PASSED\!${NC}"
    exit 0
else
    echo -e "${RED}❌ $FAILED tests failed${NC}"
    exit 1
fi
