#!/bin/bash
# Comprehensive QA Testing Script for Clean Language Compiler
# Tests all .cln files, categorizes errors, and generates priority report

set -e

# Configuration
BASE_DIR="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"
TEST_DIR="$BASE_DIR/tests/cln"
OUTPUT_DIR="$BASE_DIR/tests/output"
RESULTS_FILE="$BASE_DIR/system-documents/qa_test_results.json"
COMPILER_BIN="$BASE_DIR/target/debug/clean-language-compiler"
RUNNER_BIN="$BASE_DIR/target/debug/wasmtime_runner"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

# Initialize results
echo "{" > "$RESULTS_FILE"
echo '  "timestamp": "'$(date -u +"%Y-%m-%dT%H:%M:%SZ")'",' >> "$RESULTS_FILE"
echo '  "total_files": 0,' >> "$RESULTS_FILE"
echo '  "passed": 0,' >> "$RESULTS_FILE"
echo '  "compile_failed": 0,' >> "$RESULTS_FILE"
echo '  "exec_failed": 0,' >> "$RESULTS_FILE"
echo '  "results": [' >> "$RESULTS_FILE"

# Build compiler first
echo "Building compiler..."
cd "$BASE_DIR"
cargo build --bin clean-language-compiler --bin wasmtime_runner 2>&1 | tail -20

total=0
passed=0
compile_failed=0
exec_failed=0
first_result=true

# Find all .cln files
while IFS= read -r file; do
    total=$((total + 1))

    # Get relative path
    rel_path="${file#$TEST_DIR/}"

    # Create output path preserving directory structure
    output_file="$OUTPUT_DIR/${rel_path%.cln}.wasm"
    output_dir=$(dirname "$output_file")
    mkdir -p "$output_dir"

    echo -e "\n[${total}] Testing: $rel_path"

    # Add comma separator for JSON
    if [ "$first_result" = false ]; then
        echo "," >> "$RESULTS_FILE"
    fi
    first_result=false

    # Try to compile
    compile_output=$("$COMPILER_BIN" compile -i "$file" -o "$output_file" 2>&1)
    compile_status=$?

    if [ $compile_status -ne 0 ]; then
        # Compilation failed
        compile_failed=$((compile_failed + 1))
        echo -e "  ${RED}COMPILE_FAIL${NC}"
        echo "  Error: $compile_output" | head -3

        # Extract error type
        error_type="unknown"
        if echo "$compile_output" | grep -q "parse error"; then
            error_type="parse_error"
        elif echo "$compile_output" | grep -q "semantic error"; then
            error_type="semantic_error"
        elif echo "$compile_output" | grep -q "codegen error"; then
            error_type="codegen_error"
        elif echo "$compile_output" | grep -q "thread.*panicked"; then
            error_type="panic"
        fi

        # JSON entry
        cat >> "$RESULTS_FILE" << EOF
    {
      "file": "$rel_path",
      "status": "COMPILE_FAIL",
      "error_type": "$error_type",
      "error_message": $(echo "$compile_output" | head -1 | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read().strip()))')
    }
EOF
    else
        # Compilation succeeded, try execution
        exec_output=$("$RUNNER_BIN" "$output_file" 2>&1)
        exec_status=$?

        if [ $exec_status -ne 0 ]; then
            # Execution failed
            exec_failed=$((exec_failed + 1))
            echo -e "  ${YELLOW}EXEC_FAIL${NC}"
            echo "  Error: $exec_output" | head -3

            # JSON entry
            cat >> "$RESULTS_FILE" << EOF
    {
      "file": "$rel_path",
      "status": "EXEC_FAIL",
      "error_type": "runtime_error",
      "error_message": $(echo "$exec_output" | head -1 | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read().strip()))')
    }
EOF
        else
            # Success
            passed=$((passed + 1))
            echo -e "  ${GREEN}PASS${NC}"

            # JSON entry
            cat >> "$RESULTS_FILE" << EOF
    {
      "file": "$rel_path",
      "status": "PASS",
      "error_type": null,
      "error_message": null
    }
EOF
        fi
    fi
done < <(find "$TEST_DIR" -name "*.cln" -type f | sort)

# Close JSON
echo "" >> "$RESULTS_FILE"
echo "  ]" >> "$RESULTS_FILE"
echo "}" >> "$RESULTS_FILE"

# Update summary counts
sed -i.bak "s/\"total_files\": 0/\"total_files\": $total/" "$RESULTS_FILE"
sed -i.bak "s/\"passed\": 0/\"passed\": $passed/" "$RESULTS_FILE"
sed -i.bak "s/\"compile_failed\": 0/\"compile_failed\": $compile_failed/" "$RESULTS_FILE"
sed -i.bak "s/\"exec_failed\": 0/\"exec_failed\": $exec_failed/" "$RESULTS_FILE"
rm "$RESULTS_FILE.bak"

# Print summary
echo -e "\n========================================="
echo "COMPREHENSIVE QA TEST RESULTS"
echo "========================================="
echo -e "Total Files:        $total"
echo -e "${GREEN}Passed:            $passed ($(( passed * 100 / total ))%)${NC}"
echo -e "${RED}Compile Failed:    $compile_failed ($(( compile_failed * 100 / total ))%)${NC}"
echo -e "${YELLOW}Exec Failed:       $exec_failed ($(( exec_failed * 100 / total ))%)${NC}"
echo "========================================="
echo "Results saved to: $RESULTS_FILE"
