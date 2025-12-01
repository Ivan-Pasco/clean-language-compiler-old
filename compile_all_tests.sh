#!/bin/bash

# Comprehensive test compilation script
# Compiles all .cln files and tracks success/failure

COMPILER="./target/release/cln"
TEST_DIR="tests/cln"
OUTPUT_DIR="tests/output"
RESULTS_FILE="compilation_results.txt"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Counters
total=0
success=0
failed=0

# Clear results file
> "$RESULTS_FILE"

echo "========================================="
echo "COMPREHENSIVE TEST COMPILATION"
echo "========================================="
echo ""

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

# Find all .cln files and compile them
while IFS= read -r cln_file; do
    ((total++))

    # Get relative path for output
    rel_path="${cln_file#$TEST_DIR/}"
    base_name=$(basename "$cln_file" .cln)
    dir_name=$(dirname "$rel_path")

    # Create output path (flattened to avoid directory issues)
    output_file="$OUTPUT_DIR/${dir_name//\//_}_${base_name}.wasm"

    # Compile
    output=$($COMPILER compile "$cln_file" -o "$output_file" 2>&1)
    if echo "$output" | grep -q "Successfully compiled"; then
        echo -e "${GREEN}✓${NC} $rel_path"
        echo "SUCCESS: $cln_file" >> "$RESULTS_FILE"
        ((success++))
    else
        echo -e "${RED}✗${NC} $rel_path"
        echo "FAILED: $cln_file" >> "$RESULTS_FILE"
        echo "$output" >> "$RESULTS_FILE"
        ((failed++))
    fi
done < <(find "$TEST_DIR" -name "*.cln" -type f | sort)

echo ""
echo "========================================="
echo "COMPILATION RESULTS"
echo "========================================="
echo "Total files:    $total"
echo -e "${GREEN}Compiled:       $success${NC}"
echo -e "${RED}Failed:         $failed${NC}"
echo "Success rate:   $(awk "BEGIN {printf \"%.2f\", ($success/$total)*100}")%"
echo ""

# Show failed files if any
if [ $failed -gt 0 ]; then
    echo "Failed files:"
    grep "FAILED:" "$RESULTS_FILE" | sed 's/FAILED: /  - /'
fi
