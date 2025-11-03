#!/bin/bash

# Comprehensive WASM validation script
COMPILER="./target/release/clean-language-compiler"
TEST_DIR="tests/cln"
OUTPUT_DIR="tests/output"
RESULTS_FILE="validation_results.txt"

echo "=== WASM Validation Analysis ===" > "$RESULTS_FILE"
echo "Date: $(date)" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"

# Compile all test files
echo "Compiling all test files..."
total_files=0
compiled_files=0
validation_passed=0
validation_failed=0

# Track error categories
declare -A error_categories

for cln_file in $(find "$TEST_DIR" -name "*.cln" | sort); do
    total_files=$((total_files + 1))

    # Get relative path for output
    rel_path="${cln_file#$TEST_DIR/}"
    wasm_file="$OUTPUT_DIR/${rel_path%.cln}.wasm"

    # Create output directory if needed
    mkdir -p "$(dirname "$wasm_file")"

    # Compile
    if "$COMPILER" compile -i "$cln_file" -o "$wasm_file" 2>/dev/null; then
        compiled_files=$((compiled_files + 1))

        # Validate
        if wasm-validate "$wasm_file" 2>/dev/null; then
            validation_passed=$((validation_passed + 1))
        else
            validation_failed=$((validation_failed + 1))

            # Capture error type
            error_msg=$(wasm-validate "$wasm_file" 2>&1 | head -1)

            # Categorize error
            if echo "$error_msg" | grep -q "type mismatch.*call"; then
                category="type_mismatch_call"
            elif echo "$error_msg" | grep -q "type mismatch.*implicit return"; then
                category="type_mismatch_implicit_return"
            elif echo "$error_msg" | grep -q "type mismatch.*end of function"; then
                category="type_mismatch_end_of_function"
            elif echo "$error_msg" | grep -q "type mismatch"; then
                category="type_mismatch_other"
            elif echo "$error_msg" | grep -q "function variable out of range"; then
                category="function_index_out_of_range"
            else
                category="other"
            fi

            error_categories[$category]=$((${error_categories[$category]:-0} + 1))

            echo "FAIL: $cln_file" >> "$RESULTS_FILE"
            echo "  Error: $error_msg" >> "$RESULTS_FILE"
            echo "" >> "$RESULTS_FILE"
        fi
    fi
done

# Summary
echo "" >> "$RESULTS_FILE"
echo "=== SUMMARY ===" >> "$RESULTS_FILE"
echo "Total test files: $total_files" >> "$RESULTS_FILE"
echo "Compiled: $compiled_files" >> "$RESULTS_FILE"
echo "Validation passed: $validation_passed" >> "$RESULTS_FILE"
echo "Validation failed: $validation_failed" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"

echo "=== ERROR CATEGORIES ===" >> "$RESULTS_FILE"
for category in "${!error_categories[@]}"; do
    echo "$category: ${error_categories[$category]}" >> "$RESULTS_FILE"
done | sort

# Display summary to console
echo ""
echo "=== VALIDATION SUMMARY ==="
echo "Total: $total_files | Compiled: $compiled_files | Valid: $validation_passed | Failed: $validation_failed"
echo ""
echo "Error categories:"
for category in "${!error_categories[@]}"; do
    echo "  $category: ${error_categories[$category]}"
done | sort
echo ""
echo "Full results in: $RESULTS_FILE"
