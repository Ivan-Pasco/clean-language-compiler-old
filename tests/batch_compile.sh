#!/bin/bash
# Batch compilation script for Clean Language test files

# Change to project root directory
cd "$(dirname "$0")/.."

TEST_DIR="tests/clean_files"
WASM_DIR="tests/wasm"
LOG_FILE="tests/batch_compilation_results.log"

# Initialize counters
TOTAL=0
SUCCESS=0
FAILED=0

echo "Clean Language Batch Compilation Results" > $LOG_FILE
echo "=======================================" >> $LOG_FILE
echo "Start time: $(date)" >> $LOG_FILE
echo "" >> $LOG_FILE

# Compile all .cln files
for file in $TEST_DIR/*.cln; do
    if [ -f "$file" ]; then
        filename=$(basename "$file" .cln)
        echo "Compiling $filename..."
        
        TOTAL=$((TOTAL + 1))
        
        # Attempt compilation with longer timeout for Rust builds
        if timeout 120s cargo run --bin clean-language-compiler compile -i "$file" -o "$WASM_DIR/$filename.wasm" &> /tmp/compile_output.txt; then
            if [ -f "$WASM_DIR/$filename.wasm" ]; then
                echo "✅ SUCCESS: $filename" >> $LOG_FILE
                SUCCESS=$((SUCCESS + 1))
            else
                echo "❌ FAILED: $filename (no output file)" >> $LOG_FILE
                FAILED=$((FAILED + 1))
            fi
        else
            echo "❌ FAILED: $filename" >> $LOG_FILE
            # Include error details
            echo "   Error details:" >> $LOG_FILE
            grep -E "(Error|error:|failed)" /tmp/compile_output.txt | head -3 | sed 's/^/   /' >> $LOG_FILE
            FAILED=$((FAILED + 1))
        fi
    fi
done

# Calculate success rate
if [ $TOTAL -gt 0 ]; then
    SUCCESS_RATE=$((SUCCESS * 100 / TOTAL))
else
    SUCCESS_RATE=0
fi

echo "" >> $LOG_FILE
echo "COMPILATION SUMMARY" >> $LOG_FILE
echo "==================" >> $LOG_FILE
echo "Total files: $TOTAL" >> $LOG_FILE
echo "Successful: $SUCCESS" >> $LOG_FILE
echo "Failed: $FAILED" >> $LOG_FILE
echo "Success rate: $SUCCESS_RATE%" >> $LOG_FILE
echo "End time: $(date)" >> $LOG_FILE

# Output to console as well
echo ""
echo "BATCH COMPILATION COMPLETE"
echo "=========================="
echo "Total files: $TOTAL"
echo "Successful: $SUCCESS" 
echo "Failed: $FAILED"
echo "Success rate: $SUCCESS_RATE%"
echo ""
echo "See $LOG_FILE for detailed results"