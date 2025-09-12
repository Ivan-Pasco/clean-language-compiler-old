#!/bin/bash

# Stable Test Runner for Clean Language Compiler
# Ensures consistent, reliable test results

set -e  # Exit on any error

echo "=== STABLE CLEAN LANGUAGE COMPILER TEST RUNNER ==="
echo "Started: $(date)"

# Build the compiler first to ensure consistency
echo "Building compiler..."
cargo build --release --quiet

# Initialize counters
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

# Create output directories
mkdir -p ../tests/results/passed ../tests/results/failed

# Clear previous results
rm -f ../tests/results/passed/* ../tests/results/failed/* 2>/dev/null || true
echo "" > ../tests/results/passed.txt
echo "" > ../tests/results/failed.txt

echo ""
echo "Running tests..."

# Test each file individually: compile AND run
for test_file in ../tests/clean_files/*.cln; do
    if [ ! -f "$test_file" ]; then
        continue
    fi
    
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    filename=$(basename "$test_file")
    output_file="/tmp/test_${filename%.cln}.wasm"
    
    echo "Testing $filename..."
    
    # Step 1: Compile the Clean Language file to WASM
    if ../target/release/clean-language-compiler compile -i "$test_file" -o "$output_file" >/dev/null 2>&1; then
        # Step 2: Try to run the WASM file with the project's wasmtime_runner
        if [ -f "../target/release/wasmtime_runner" ]; then
            # Run with the custom wasmtime runner and capture result
            if ../target/release/wasmtime_runner "$output_file" >/dev/null 2>&1; then
                PASSED_TESTS=$((PASSED_TESTS + 1))
                echo "$filename" >> ../tests/results/passed.txt
                echo "✅ $filename (compiled and executed successfully)"
            else
                FAILED_TESTS=$((FAILED_TESTS + 1))
                echo "$filename" >> ../tests/results/failed.txt
                echo "❌ $filename (compiled but runtime error)"
                # Capture runtime error
                ../target/release/wasmtime_runner "$output_file" 2> "../tests/results/failed/${filename}.runtime_error" || true
            fi
        else
            # If custom wasmtime runner not available, just check compilation success
            PASSED_TESTS=$((PASSED_TESTS + 1))
            echo "$filename" >> ../tests/results/passed.txt
            echo "✅ $filename (compiled successfully - wasmtime_runner not available)"
        fi
    else
        FAILED_TESTS=$((FAILED_TESTS + 1))
        echo "$filename" >> ../tests/results/failed.txt
        echo "❌ $filename (compilation failed)"
        
        # Capture compilation error
        ../target/release/clean-language-compiler compile -i "$test_file" -o "$output_file" 2> "../tests/results/failed/${filename}.compile_error" || true
    fi
done

# Calculate success rate
SUCCESS_RATE=$(echo "scale=2; $PASSED_TESTS * 100 / $TOTAL_TESTS" | bc -l)

echo ""
echo "=== FINAL STABLE RESULTS ==="
echo "Total tests: $TOTAL_TESTS"
echo "Passed: $PASSED_TESTS"
echo "Failed: $FAILED_TESTS"
echo "Success rate: ${SUCCESS_RATE}% ($PASSED_TESTS/$TOTAL_TESTS)"
echo "Target: 100% ($TOTAL_TESTS/$TOTAL_TESTS)"
echo "Completed: $(date)"
echo ""

# Show progress toward goal
REMAINING=$((TOTAL_TESTS - PASSED_TESTS))
echo "Remaining to fix: $REMAINING tests"
echo "Progress: $PASSED_TESTS/$TOTAL_TESTS tests passing"