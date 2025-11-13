#!/bin/bash
# Compile ALL test files from tests/cln/ to tests/output/
# Maintaining directory structure for proper organization

set -e

cd "$(dirname "$0")"

echo "=================================================="
echo "  Clean Language Compiler - FULL TEST COMPILATION"
echo "=================================================="
echo ""

# Create output directory
mkdir -p tests/output

# Counters
total=0
success=0
failed=0

# Temporary files for tracking
failed_tests=$(mktemp)
success_tests=$(mktemp)

trap "rm -f $failed_tests $success_tests" EXIT

echo "Compiling ALL test files..."
echo ""

# Find all .cln files and compile them
while IFS= read -r cln_file; do
    ((total++))

    # Get filename without extension
    filename=$(basename "$cln_file" .cln)

    # Get relative path from tests/cln/
    rel_path="${cln_file#tests/cln/}"
    category=$(dirname "$rel_path")

    # Determine output file (flat structure in tests/output)
    output_file="tests/output/${filename}.wasm"

    echo -n "[$total] Compiling $rel_path... "

    # Compile and capture both stdout and stderr
    if ./target/release/cln compile -i "$cln_file" -o "$output_file" > /dev/null 2>&1; then
        echo "✅"
        ((success++))
        echo "$rel_path" >> "$success_tests"
    else
        echo "❌"
        ((failed++))
        echo "$rel_path" >> "$failed_tests"
    fi
done < <(find tests/cln -name "*.cln" -type f | sort)

echo ""
echo "=================================================="
echo "  COMPILATION RESULTS"
echo "=================================================="
echo "Total test files:  $total"
echo "✅ Compiled:        $success ($(echo "scale=1; $success * 100 / $total" | bc)%)"
echo "❌ Failed:          $failed ($(echo "scale=1; $failed * 100 / $total" | bc)%)"
echo "=================================================="
echo ""

if [ $failed -gt 0 ]; then
    echo "Failed test files:"
    echo "---"
    cat "$failed_tests"
    echo ""
fi

exit 0
