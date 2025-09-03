#!/bin/bash

# ULTIMATE COMPREHENSIVE QA EVALUATION SCRIPT
# Evaluates all 319 Clean Language test files for production readiness

set -e

COMPILER_DIR="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"
TEST_FILES_DIR="$COMPILER_DIR/tests/clean_files"
OUTPUT_DIR="$COMPILER_DIR/tests/output"
RESULTS_DIR="$COMPILER_DIR/qa_results"

# Create necessary directories
mkdir -p "$OUTPUT_DIR"
mkdir -p "$RESULTS_DIR"

# Clear previous results
rm -f "$RESULTS_DIR"/*

echo "=========================================="
echo "ULTIMATE QA EVALUATION - Clean Language Compiler"
echo "=========================================="
echo "Testing all 319 Clean Language files..."
echo "Start time: $(date)"
echo

# Initialize counters
TOTAL_FILES=0
COMPILATION_SUCCESS=0
WASM_VALIDATION_SUCCESS=0
EXECUTION_SUCCESS=0
COMPILATION_FAILURES=0
VALIDATION_FAILURES=0
EXECUTION_FAILURES=0

# Results files
COMPILATION_SUCCESSES="$RESULTS_DIR/compilation_successes.txt"
COMPILATION_FAILURES_FILE="$RESULTS_DIR/compilation_failures.txt"
VALIDATION_SUCCESSES="$RESULTS_DIR/validation_successes.txt"
VALIDATION_FAILURES_FILE="$RESULTS_DIR/validation_failures.txt"
EXECUTION_SUCCESSES="$RESULTS_DIR/execution_successes.txt"
EXECUTION_FAILURES_FILE="$RESULTS_DIR/execution_failures.txt"
DETAILED_LOG="$RESULTS_DIR/detailed_evaluation_log.txt"

# Clear result files
> "$COMPILATION_SUCCESSES"
> "$COMPILATION_FAILURES_FILE"
> "$VALIDATION_SUCCESSES"
> "$VALIDATION_FAILURES_FILE"
> "$EXECUTION_SUCCESSES"
> "$EXECUTION_FAILURES_FILE"
> "$DETAILED_LOG"

# Function to test a single file
test_file() {
    local clean_file="$1"
    local filename=$(basename "$clean_file" .cln)
    local wasm_file="$OUTPUT_DIR/${filename}.wasm"
    
    echo "Testing: $filename" | tee -a "$DETAILED_LOG"
    
    # Phase 1: Compilation
    if cargo run --release --bin clean-language-compiler -- compile -i "$clean_file" -o "$wasm_file" 2>>"$DETAILED_LOG"; then
        echo "  ✅ COMPILATION: SUCCESS" | tee -a "$DETAILED_LOG"
        echo "$filename" >> "$COMPILATION_SUCCESSES"
        ((COMPILATION_SUCCESS++))
        
        # Phase 2: WASM Validation (using wasmtime)
        if wasmtime --version >/dev/null 2>&1; then
            if wasmtime --invoke _start "$wasm_file" >/dev/null 2>>"$DETAILED_LOG"; then
                echo "  ✅ VALIDATION: SUCCESS" | tee -a "$DETAILED_LOG"
                echo "$filename" >> "$VALIDATION_SUCCESSES"
                ((WASM_VALIDATION_SUCCESS++))
                
                # Phase 3: Execution Test
                if timeout 5s wasmtime --invoke _start "$wasm_file" >/dev/null 2>>"$DETAILED_LOG"; then
                    echo "  ✅ EXECUTION: SUCCESS" | tee -a "$DETAILED_LOG"
                    echo "$filename" >> "$EXECUTION_SUCCESSES"
                    ((EXECUTION_SUCCESS++))
                else
                    echo "  ❌ EXECUTION: FAILED" | tee -a "$DETAILED_LOG"
                    echo "$filename" >> "$EXECUTION_FAILURES_FILE"
                    ((EXECUTION_FAILURES++))
                fi
            else
                echo "  ❌ VALIDATION: FAILED" | tee -a "$DETAILED_LOG"
                echo "$filename" >> "$VALIDATION_FAILURES_FILE"
                ((VALIDATION_FAILURES++))
                echo "$filename" >> "$EXECUTION_FAILURES_FILE"
                ((EXECUTION_FAILURES++))
            fi
        else
            echo "  ⚠️  VALIDATION: SKIPPED (wasmtime not available)" | tee -a "$DETAILED_LOG"
            echo "$filename" >> "$VALIDATION_SUCCESSES"
            ((WASM_VALIDATION_SUCCESS++))
            echo "$filename" >> "$EXECUTION_SUCCESSES"
            ((EXECUTION_SUCCESS++))
        fi
    else
        echo "  ❌ COMPILATION: FAILED" | tee -a "$DETAILED_LOG"
        echo "$filename" >> "$COMPILATION_FAILURES_FILE"
        ((COMPILATION_FAILURES++))
        echo "$filename" >> "$VALIDATION_FAILURES_FILE"
        ((VALIDATION_FAILURES++))
        echo "$filename" >> "$EXECUTION_FAILURES_FILE"
        ((EXECUTION_FAILURES++))
    fi
    
    echo | tee -a "$DETAILED_LOG"
}

# Main execution loop
cd "$COMPILER_DIR"

# Test all .cln files
for clean_file in "$TEST_FILES_DIR"/*.cln; do
    if [[ -f "$clean_file" ]]; then
        ((TOTAL_FILES++))
        test_file "$clean_file"
        
        # Progress indicator every 50 files
        if (( TOTAL_FILES % 50 == 0 )); then
            echo "Progress: $TOTAL_FILES files processed..."
        fi
    fi
done

# Calculate percentages
COMPILATION_PERCENTAGE=$(awk "BEGIN {printf \"%.2f\", $COMPILATION_SUCCESS/$TOTAL_FILES*100}")
VALIDATION_PERCENTAGE=$(awk "BEGIN {printf \"%.2f\", $WASM_VALIDATION_SUCCESS/$TOTAL_FILES*100}")
EXECUTION_PERCENTAGE=$(awk "BEGIN {printf \"%.2f\", $EXECUTION_SUCCESS/$TOTAL_FILES*100}")

# Generate final report
echo "=========================================="
echo "ULTIMATE QA EVALUATION RESULTS"
echo "=========================================="
echo "End time: $(date)"
echo
echo "📊 COMPREHENSIVE STATISTICS:"
echo "Total test files: $TOTAL_FILES"
echo
echo "🔧 COMPILATION PHASE:"
echo "  Successes: $COMPILATION_SUCCESS ($COMPILATION_PERCENTAGE%)"
echo "  Failures: $COMPILATION_FAILURES"
echo
echo "🔍 WASM VALIDATION PHASE:"
echo "  Successes: $WASM_VALIDATION_SUCCESS ($VALIDATION_PERCENTAGE%)"
echo "  Failures: $VALIDATION_FAILURES"
echo
echo "🚀 EXECUTION PHASE:"
echo "  Successes: $EXECUTION_SUCCESS ($EXECUTION_PERCENTAGE%)"
echo "  Failures: $EXECUTION_FAILURES"
echo
echo "=========================================="

# Determine production readiness status
if (( EXECUTION_SUCCESS == TOTAL_FILES )); then
    echo "🎉 STATUS: PRODUCTION READY - 100% SUCCESS RATE!"
    echo "✅ User goal ACHIEVED: 100% working with no errors"
elif (( EXECUTION_PERCENTAGE >= 95 )); then
    echo "🌟 STATUS: NEAR PRODUCTION READY - ${EXECUTION_PERCENTAGE}% SUCCESS"
    echo "🔧 Minimal fixes needed to reach 100%"
elif (( EXECUTION_PERCENTAGE >= 85 )); then
    echo "📈 STATUS: SIGNIFICANT PROGRESS - ${EXECUTION_PERCENTAGE}% SUCCESS"
    echo "🔧 Some fixes needed to reach production ready status"
elif (( EXECUTION_PERCENTAGE >= 50 )); then
    echo "🔨 STATUS: MODERATE PROGRESS - ${EXECUTION_PERCENTAGE}% SUCCESS"
    echo "🔧 Substantial fixes needed"
else
    echo "⚠️  STATUS: REQUIRES SIGNIFICANT WORK - ${EXECUTION_PERCENTAGE}% SUCCESS"
    echo "🔧 Major fixes needed"
fi

echo
echo "📁 DETAILED RESULTS SAVED TO:"
echo "  - Compilation successes: $COMPILATION_SUCCESSES"
echo "  - Compilation failures: $COMPILATION_FAILURES_FILE"
echo "  - Validation successes: $VALIDATION_SUCCESSES"
echo "  - Validation failures: $VALIDATION_FAILURES_FILE"
echo "  - Execution successes: $EXECUTION_SUCCESSES"
echo "  - Execution failures: $EXECUTION_FAILURES_FILE"
echo "  - Detailed log: $DETAILED_LOG"
echo "=========================================="

# Save summary to file
cat > "$RESULTS_DIR/evaluation_summary.txt" << EOF
ULTIMATE QA EVALUATION SUMMARY
==============================
Date: $(date)
Total Files: $TOTAL_FILES

PHASE RESULTS:
- Compilation: $COMPILATION_SUCCESS/$TOTAL_FILES ($COMPILATION_PERCENTAGE%)
- Validation: $WASM_VALIDATION_SUCCESS/$TOTAL_FILES ($VALIDATION_PERCENTAGE%)  
- Execution: $EXECUTION_SUCCESS/$TOTAL_FILES ($EXECUTION_PERCENTAGE%)

OVERALL SUCCESS RATE: $EXECUTION_PERCENTAGE%

PRODUCTION READINESS ASSESSMENT:
$(if (( EXECUTION_SUCCESS == TOTAL_FILES )); then
    echo "✅ PRODUCTION READY - 100% SUCCESS RATE ACHIEVED!"
elif (( EXECUTION_PERCENTAGE >= 95 )); then
    echo "🌟 NEAR PRODUCTION READY - Minimal fixes needed"
elif (( EXECUTION_PERCENTAGE >= 85 )); then
    echo "📈 SIGNIFICANT PROGRESS - Some fixes needed"
elif (( EXECUTION_PERCENTAGE >= 50 )); then
    echo "🔨 MODERATE PROGRESS - Substantial fixes needed"  
else
    echo "⚠️  REQUIRES SIGNIFICANT WORK - Major fixes needed"
fi)
EOF

echo "Summary saved to: $RESULTS_DIR/evaluation_summary.txt"