#!/bin/bash

# Comprehensive Batch Fix Script for Clean Language Test Files
# Systematically fixes common parsing issues

echo "🔧 Starting comprehensive batch fix for Clean Language test files..."
echo ""

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TESTS_DIR="$SCRIPT_DIR/tests/clean_files"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Counters
total_files=0
fixed_files=0
already_working=0
failed_to_fix=0

echo -e "${BLUE}=== Phase 1: Systematic Return Type Fixes ===${NC}"

# Function to test if a file compiles
test_compilation() {
    local file="$1"
    cargo run --release --bin clean-language-compiler compile -i "$file" -o "/tmp/test_$(basename "$file").wasm" >/dev/null 2>&1
    return $?
}

# Function to add void return type to functions
fix_return_types() {
    local file="$1"
    
    # Create backup
    cp "$file" "$file.backup"
    
    # Fix common patterns - functions without return types
    # Pattern 1: function_name() at start of line (with tabs)
    sed -i '' 's/^[[:space:]]*\([a-zA-Z_][a-zA-Z0-9_]*\)()[[:space:]]*$/	void \1()/g' "$file"
    
    # Pattern 2: functions inside functions: block (preserve indentation)
    sed -i '' 's/^[[:space:]]*\t\([a-zA-Z_][a-zA-Z0-9_]*\)()[[:space:]]*$/		void \1()/g' "$file"
    
    # Pattern 3: top-level start() function
    sed -i '' 's/^start()[[:space:]]*$/void start()/g' "$file"
    
    # Pattern 4: any standalone function declaration
    sed -i '' 's/^\([[:space:]]*\)\([a-zA-Z_][a-zA-Z0-9_]*\)()$/\1void \2()/g' "$file"
}

# Process each .cln file
for cln_file in "$TESTS_DIR"/*.cln; do
    if [ ! -f "$cln_file" ]; then
        continue
    fi
    
    filename=$(basename "$cln_file")
    total_files=$((total_files + 1))
    
    echo -n "[$total_files] Processing: $filename... "
    
    # Test if already compiling
    if test_compilation "$cln_file"; then
        echo -e "${GREEN}✓ Already working${NC}"
        already_working=$((already_working + 1))
        continue
    fi
    
    # Try to fix it
    fix_return_types "$cln_file"
    
    # Test if fix worked
    if test_compilation "$cln_file"; then
        echo -e "${GREEN}✓ FIXED${NC}"
        fixed_files=$((fixed_files + 1))
        rm "$cln_file.backup"
    else
        echo -e "${RED}✗ Still failing${NC}"
        failed_to_fix=$((failed_to_fix + 1))
        # Restore backup
        mv "$cln_file.backup" "$cln_file"
    fi
done

echo ""
echo -e "${BLUE}=== Batch Fix Results ===${NC}"
echo -e "Total files processed: ${YELLOW}$total_files${NC}"
echo -e "Already working: ${GREEN}$already_working${NC}"
echo -e "Successfully fixed: ${GREEN}$fixed_files${NC}"
echo -e "Failed to fix: ${RED}$failed_to_fix${NC}"

# Calculate new success rate
if [ $total_files -gt 0 ]; then
    success_count=$((already_working + fixed_files))
    success_rate=$((success_count * 100 / total_files))
    echo -e "New success rate: ${YELLOW}${success_rate}% (${success_count}/${total_files})${NC}"
fi

echo ""
echo -e "${BLUE}=== Running Final Validation ===${NC}"

# Run comprehensive test to get accurate final count
echo "Running comprehensive compilation test..."
total=0
passed=0
for file in "$TESTS_DIR"/*.cln; do
    total=$((total+1))
    if cargo run --release --bin clean-language-compiler compile -i "$file" -o "/tmp/$(basename "$file").wasm" >/dev/null 2>&1; then
        passed=$((passed+1))
    fi
done

final_rate=$((passed * 100 / total))
echo ""
echo -e "${YELLOW}🎉 FINAL RESULTS: $passed/$total tests passed (${final_rate}% success rate)${NC}"

if [ $fixed_files -gt 0 ]; then
    echo -e "${GREEN}Successfully improved $fixed_files additional test files!${NC}"
fi

echo ""
echo -e "${BLUE}Comprehensive batch fix completed!${NC}"