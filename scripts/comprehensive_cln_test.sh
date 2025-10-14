#!/bin/bash
# Comprehensive Clean Language Test Suite
# Tests all .cln files individually: compilation and execution

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Directories
CLN_DIR="tests/cln"
OUTPUT_DIR="tests/output"
RESULTS_FILE="/tmp/comprehensive_test_results.json"

# Counters
total_files=0
compile_success=0
compile_fail=0
exec_success=0
exec_fail=0

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Initialize results file
echo "{" > "$RESULTS_FILE"
echo '  "timestamp": "'$(date -u +"%Y-%m-%dT%H:%M:%SZ")'",' >> "$RESULTS_FILE"
echo '  "tests": [' >> "$RESULTS_FILE"

first_entry=true

echo -e "${BLUE}=== Comprehensive Clean Language Test Suite ===${NC}"
echo -e "${BLUE}Testing all .cln files in $CLN_DIR${NC}"
echo ""

# Find all .cln files
while IFS= read -r -d '' file; do
    total_files=$((total_files + 1))

    # Get relative path and base name
    rel_path="${file#$CLN_DIR/}"
    base_name=$(basename "$file" .cln)
    dir_name=$(dirname "$rel_path")

    # Create output path
    output_wasm="$OUTPUT_DIR/${dir_name}/${base_name}.wasm"
    mkdir -p "$(dirname "$output_wasm")"

    echo -ne "${YELLOW}[$total_files]${NC} Testing: $rel_path ... "

    # Add comma for JSON array (except first entry)
    if [ "$first_entry" = false ]; then
        echo "," >> "$RESULTS_FILE"
    fi
    first_entry=false

    # Try to compile
    compile_error=""
    if cargo run --bin clean-language-compiler compile -i "$file" -o "$output_wasm" 2>&1 >/dev/null | grep -v "^warning:" > /tmp/compile_error.txt 2>&1; then
        # Compilation successful
        compile_success=$((compile_success + 1))

        # Try to execute with wasmtime_runner
        if cargo run --bin wasmtime_runner "$output_wasm" > /tmp/exec_output.txt 2>&1; then
            exec_success=$((exec_success + 1))
            echo -e "${GREEN}✓ PASS${NC} (compile + exec)"

            # Record success in JSON
            echo -n "    {\"file\": \"$rel_path\", \"compile\": \"success\", \"execute\": \"success\"}" >> "$RESULTS_FILE"
        else
            exec_fail=$((exec_fail + 1))
            exec_error=$(cat /tmp/exec_output.txt | head -5 | tr '\n' ' ')
            echo -e "${YELLOW}⚠ PARTIAL${NC} (compile ok, exec failed)"

            # Record partial success in JSON
            echo -n "    {\"file\": \"$rel_path\", \"compile\": \"success\", \"execute\": \"failed\", \"exec_error\": \"${exec_error//\"/\\\"}\"}" >> "$RESULTS_FILE"
        fi
    else
        compile_fail=$((compile_fail + 1))
        compile_error=$(cat /tmp/compile_error.txt | head -3 | tr '\n' ' ')
        echo -e "${RED}✗ FAIL${NC} (compilation failed)"

        # Record failure in JSON
        echo -n "    {\"file\": \"$rel_path\", \"compile\": \"failed\", \"compile_error\": \"${compile_error//\"/\\\"}\"}" >> "$RESULTS_FILE"
    fi

done < <(find "$CLN_DIR" -name "*.cln" -type f -print0 | sort -z)

# Close JSON
echo "" >> "$RESULTS_FILE"
echo "  ]," >> "$RESULTS_FILE"
echo "  \"summary\": {" >> "$RESULTS_FILE"
echo "    \"total_files\": $total_files," >> "$RESULTS_FILE"
echo "    \"compile_success\": $compile_success," >> "$RESULTS_FILE"
echo "    \"compile_fail\": $compile_fail," >> "$RESULTS_FILE"
echo "    \"exec_success\": $exec_success," >> "$RESULTS_FILE"
echo "    \"exec_fail\": $exec_fail," >> "$RESULTS_FILE"
echo "    \"compile_rate\": \"$(awk "BEGIN {printf \"%.1f\", ($compile_success/$total_files)*100}")%\"," >> "$RESULTS_FILE"
echo "    \"exec_rate\": \"$(awk "BEGIN {printf \"%.1f\", ($exec_success/$total_files)*100}")%\"" >> "$RESULTS_FILE"
echo "  }" >> "$RESULTS_FILE"
echo "}" >> "$RESULTS_FILE"

# Print summary
echo ""
echo -e "${BLUE}=== Test Summary ===${NC}"
echo -e "Total files:       ${BLUE}$total_files${NC}"
echo -e "Compile success:   ${GREEN}$compile_success${NC}"
echo -e "Compile fail:      ${RED}$compile_fail${NC}"
echo -e "Execute success:   ${GREEN}$exec_success${NC}"
echo -e "Execute fail:      ${YELLOW}$exec_fail${NC}"
echo ""
echo -e "Compile rate:      $(awk "BEGIN {printf \"%.1f%%\", ($compile_success/$total_files)*100}")"
echo -e "Execute rate:      $(awk "BEGIN {printf \"%.1f%%\", ($exec_success/$total_files)*100}")"
echo ""
echo -e "Detailed results saved to: ${BLUE}$RESULTS_FILE${NC}"

# Exit with error if any tests failed
if [ $compile_fail -gt 0 ] || [ $exec_fail -gt 0 ]; then
    exit 1
fi

exit 0
