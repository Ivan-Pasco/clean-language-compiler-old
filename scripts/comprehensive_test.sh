#!/bin/bash

# Comprehensive Test Runner for Clean Language Compiler
# Tests all .cln files, compiles to WASM, and executes them

OUTPUT_DIR="tests/output"
RESULTS_FILE="test_results_$(date +%Y%m%d_%H%M%S).json"
COMPILER="cargo run --bin clean-language-compiler"
RUNNER="cargo run --bin wasmtime_runner"

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Initialize results
echo "{" > "$RESULTS_FILE"
echo '  "timestamp": "'$(date -u +"%Y-%m-%dT%H:%M:%SZ")'",' >> "$RESULTS_FILE"
echo '  "tests": [' >> "$RESULTS_FILE"

total=0
passed=0
failed=0
first=true

# Find all .cln files
while IFS= read -r cln_file; do
    total=$((total + 1))

    # Get relative path and create output filename
    rel_path="${cln_file#tests/cln/}"
    wasm_file="$OUTPUT_DIR/${rel_path%.cln}.wasm"
    wasm_dir=$(dirname "$wasm_file")

    # Create output directory structure
    mkdir -p "$wasm_dir"

    # Add comma if not first entry
    if [ "$first" = false ]; then
        echo "," >> "$RESULTS_FILE"
    fi
    first=false

    echo "    {" >> "$RESULTS_FILE"
    echo '      "file": "'$cln_file'",' >> "$RESULTS_FILE"

    # Try to compile
    compile_output=$($COMPILER compile -i "$cln_file" -o "$wasm_file" 2>&1)
    compile_exit=$?

    if [ $compile_exit -eq 0 ]; then
        echo '      "compile": "success",' >> "$RESULTS_FILE"

        # Try to execute
        if [ -f "$wasm_file" ]; then
            exec_output=$(timeout 5 $RUNNER "$wasm_file" 2>&1)
            exec_exit=$?

            if [ $exec_exit -eq 0 ]; then
                echo '      "execute": "success",' >> "$RESULTS_FILE"
                echo '      "status": "passed"' >> "$RESULTS_FILE"
                passed=$((passed + 1))
            else
                echo '      "execute": "failed",' >> "$RESULTS_FILE"
                echo '      "exec_error": "'$(echo "$exec_output" | head -1 | sed 's/"/\\"/g')'",' >> "$RESULTS_FILE"
                echo '      "status": "exec_failed"' >> "$RESULTS_FILE"
                failed=$((failed + 1))
            fi
        else
            echo '      "execute": "skipped",' >> "$RESULTS_FILE"
            echo '      "status": "no_wasm"' >> "$RESULTS_FILE"
            failed=$((failed + 1))
        fi
    else
        echo '      "compile": "failed",' >> "$RESULTS_FILE"
        echo '      "compile_error": "'$(echo "$compile_output" | grep -E "Error|error" | head -1 | sed 's/"/\\"/g')'",' >> "$RESULTS_FILE"
        echo '      "status": "compile_failed"' >> "$RESULTS_FILE"
        failed=$((failed + 1))
    fi

    echo "    }" >> "$RESULTS_FILE"

    # Progress indicator
    echo "[$total] $cln_file: $([ $compile_exit -eq 0 ] && echo "✓ compiled" || echo "✗ compile failed")"

done < <(find tests/cln -name "*.cln" -type f | sort)

# Close JSON
echo "" >> "$RESULTS_FILE"
echo "  ]," >> "$RESULTS_FILE"
echo '  "summary": {' >> "$RESULTS_FILE"
echo '    "total": '$total',' >> "$RESULTS_FILE"
echo '    "passed": '$passed',' >> "$RESULTS_FILE"
echo '    "failed": '$failed >> "$RESULTS_FILE"
echo '  }' >> "$RESULTS_FILE"
echo "}" >> "$RESULTS_FILE"

echo ""
echo "========================================="
echo "Test Results Summary"
echo "========================================="
echo "Total:  $total"
echo "Passed: $passed"
echo "Failed: $failed"
echo "Success Rate: $(awk "BEGIN {printf \"%.2f\", ($passed/$total)*100}")%"
echo ""
echo "Results saved to: $RESULTS_FILE"
