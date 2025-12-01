#!/bin/bash

# WASM validation script
# Validates all .wasm files using wasm-validate

OUTPUT_DIR="tests/output"
RESULTS_FILE="wasm_validation_results.txt"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Counters
total=0
valid=0
invalid=0

# Clear results
> "$RESULTS_FILE"

echo "========================================="
echo "WASM VALIDATION"
echo "========================================="
echo ""

# Find and validate all .wasm files
while IFS= read -r wasm_file; do
    ((total++))

    filename=$(basename "$wasm_file")

    if wasm-validate "$wasm_file" > /dev/null 2>&1; then
        echo -e "${GREEN}✓${NC} $filename"
        echo "VALID: $wasm_file" >> "$RESULTS_FILE"
        ((valid++))
    else
        echo -e "${RED}✗${NC} $filename"
        echo "INVALID: $wasm_file" >> "$RESULTS_FILE"
        wasm-validate "$wasm_file" 2>&1 >> "$RESULTS_FILE"
        ((invalid++))
    fi
done < <(find "$OUTPUT_DIR" -name "*.wasm" -type f | sort)

echo ""
echo "========================================="
echo "VALIDATION RESULTS"
echo "========================================="
echo "Total WASM files:  $total"
echo -e "${GREEN}Valid:             $valid${NC}"
echo -e "${RED}Invalid:           $invalid${NC}"

if [ $invalid -gt 0 ]; then
    echo ""
    echo "Invalid files:"
    grep "INVALID:" "$RESULTS_FILE" | sed 's/INVALID: /  - /'
fi
