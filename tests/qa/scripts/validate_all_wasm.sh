#!/bin/bash
# Validate all WASM files in tests/output/

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
OUTPUT_DIR="$PROJECT_ROOT/tests/output"

echo "=== WASM Validation Report ==="
echo "Date: $(date)"
echo "Directory: $OUTPUT_DIR"
echo ""

TOTAL=0
VALID=0
INVALID=0

for wasm in "$OUTPUT_DIR"/*.wasm; do
    if [ -f "$wasm" ]; then
        TOTAL=$((TOTAL + 1))
        if wasm-validate "$wasm" 2>/dev/null; then
            VALID=$((VALID + 1))
        else
            INVALID=$((INVALID + 1))
            echo "INVALID: $(basename "$wasm")"
        fi
    fi
done

echo ""
echo "=== Summary ==="
echo "Total:   $TOTAL"
echo "Valid:   $VALID"
echo "Invalid: $INVALID"

if [ $TOTAL -gt 0 ]; then
    RATE=$((VALID * 100 / TOTAL))
    echo "Rate:    ${RATE}%"
fi

if [ $INVALID -gt 0 ]; then
    exit 1
else
    echo ""
    echo "All WASM files are valid!"
    exit 0
fi
