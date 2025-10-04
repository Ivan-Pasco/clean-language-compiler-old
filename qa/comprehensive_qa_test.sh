#!/bin/bash

# Master Comprehensive Clean Language QA Test
# Runs complete compilation and runtime testing pipeline

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m' # No Color

echo -e "${BOLD}${BLUE}=========================================${NC}"
echo -e "${BOLD}${BLUE}  Clean Language Comprehensive QA Test  ${NC}"
echo -e "${BOLD}${BLUE}=========================================${NC}"
echo ""
echo -e "${YELLOW}This comprehensive test will:${NC}"
echo "  1. Compile all 319 .cln files individually"
echo "  2. Categorize any compilation failures"
echo "  3. Execute all compiled WASM files"
echo "  4. Categorize any runtime failures"
echo "  5. Generate detailed reports"
echo ""

# Create timestamped QA session directory
QA_SESSION="qa_session_$(date +%Y%m%d_%H%M%S)"
mkdir -p "tests/results/$QA_SESSION"

echo -e "${BLUE}=== Phase 1: Compilation Testing ===${NC}"
echo ""

# Run compilation test
if ./comprehensive_compile_test.sh; then
    echo -e "${GREEN}✓ Compilation test completed successfully${NC}"
    COMPILE_SUCCESS=true
else
    echo -e "${RED}✗ Compilation test found issues${NC}"
    COMPILE_SUCCESS=false
fi

echo ""
echo -e "${BLUE}=== Phase 2: Runtime Testing ===${NC}"
echo ""

# Check if we have WASM files to test
WASM_COUNT=$(find tests/output -name "*.wasm" 2>/dev/null | wc -l)
if [ "$WASM_COUNT" -eq 0 ]; then
    echo -e "${YELLOW}No WASM files found. Skipping runtime tests.${NC}"
    RUNTIME_SUCCESS=false
else
    echo "Found $WASM_COUNT WASM files to test."
    echo ""
    
    # Run runtime test
    if ./comprehensive_runtime_test.sh; then
        echo -e "${GREEN}✓ Runtime test completed successfully${NC}"
        RUNTIME_SUCCESS=true
    else
        echo -e "${RED}✗ Runtime test found issues${NC}"
        RUNTIME_SUCCESS=false
    fi
fi

echo ""
echo -e "${BLUE}=== Phase 3: Results Summary ===${NC}"
echo ""

# Copy latest results to QA session
cp tests/results/compilation_summary_*.txt "tests/results/$QA_SESSION/" 2>/dev/null || true
cp tests/results/runtime_summary_*.txt "tests/results/$QA_SESSION/" 2>/dev/null || true

# Create master summary
MASTER_SUMMARY="tests/results/$QA_SESSION/master_qa_summary.txt"
echo "Clean Language Comprehensive QA Test Results" > "$MASTER_SUMMARY"
echo "=============================================" >> "$MASTER_SUMMARY"
echo "Date: $(date)" >> "$MASTER_SUMMARY"
echo "QA Session: $QA_SESSION" >> "$MASTER_SUMMARY"
echo "" >> "$MASTER_SUMMARY"

# Get compilation stats
LATEST_COMPILE_SUMMARY=$(ls -t tests/results/compilation_summary_*.txt 2>/dev/null | head -1)
if [ -n "$LATEST_COMPILE_SUMMARY" ]; then
    echo "COMPILATION RESULTS:" >> "$MASTER_SUMMARY"
    echo "===================" >> "$MASTER_SUMMARY"
    grep -E "(Total files|Successful|Failed|Success rate|Parser errors|Semantic errors|Codegen errors)" "$LATEST_COMPILE_SUMMARY" >> "$MASTER_SUMMARY"
    echo "" >> "$MASTER_SUMMARY"
fi

# Get runtime stats
LATEST_RUNTIME_SUMMARY=$(ls -t tests/results/runtime_summary_*.txt 2>/dev/null | head -1)
if [ -n "$LATEST_RUNTIME_SUMMARY" ]; then
    echo "RUNTIME RESULTS:" >> "$MASTER_SUMMARY"
    echo "================" >> "$MASTER_SUMMARY"
    grep -E "(Total WASM|Successful runs|Failed runs|Runtime success rate|Memory errors|Execution errors|Timeout errors)" "$LATEST_RUNTIME_SUMMARY" >> "$MASTER_SUMMARY"
    echo "" >> "$MASTER_SUMMARY"
fi

# Overall assessment
echo "OVERALL ASSESSMENT:" >> "$MASTER_SUMMARY"
echo "===================" >> "$MASTER_SUMMARY"
if [ "$COMPILE_SUCCESS" = true ] && [ "$RUNTIME_SUCCESS" = true ]; then
    echo "Status: ALL TESTS PASSED ✓" >> "$MASTER_SUMMARY"
    OVERALL_SUCCESS=true
elif [ "$COMPILE_SUCCESS" = true ]; then
    echo "Status: COMPILATION PASSED, RUNTIME ISSUES FOUND" >> "$MASTER_SUMMARY"
    OVERALL_SUCCESS=false
else
    echo "Status: COMPILATION ISSUES FOUND" >> "$MASTER_SUMMARY"
    OVERALL_SUCCESS=false
fi

echo "" >> "$MASTER_SUMMARY"
echo "QA Session Files:" >> "$MASTER_SUMMARY"
echo "- Master Summary: tests/results/$QA_SESSION/master_qa_summary.txt" >> "$MASTER_SUMMARY"
echo "- Compilation Logs: tests/logs/" >> "$MASTER_SUMMARY"
echo "- WASM Output: tests/output/" >> "$MASTER_SUMMARY"

# Display final results
echo -e "${BOLD}=== FINAL QA RESULTS ===${NC}"
echo ""

if [ "$OVERALL_SUCCESS" = true ]; then
    echo -e "${BOLD}${GREEN}🎉 ALL TESTS PASSED! 🎉${NC}"
    echo -e "${GREEN}The Clean Language compiler is ready for production use.${NC}"
else
    echo -e "${BOLD}${YELLOW}⚠️  ISSUES FOUND - FIXES NEEDED ⚠️${NC}"
    
    if [ "$COMPILE_SUCCESS" = false ]; then
        echo -e "${RED}• Compilation issues detected${NC}"
    fi
    
    if [ "$RUNTIME_SUCCESS" = false ] && [ "$WASM_COUNT" -gt 0 ]; then
        echo -e "${RED}• Runtime issues detected${NC}"
    fi
fi

echo ""
echo -e "${BLUE}QA Session Results:${NC}"
echo -e "  Master Summary: ${YELLOW}tests/results/$QA_SESSION/master_qa_summary.txt${NC}"
echo -e "  Detailed Logs: ${YELLOW}tests/logs/${NC}"
echo -e "  WASM Output: ${YELLOW}tests/output/${NC}"
echo ""

# Display master summary content
echo -e "${BLUE}=== Quick Summary ===${NC}"
if [ -f "$MASTER_SUMMARY" ]; then
    grep -A 20 "COMPILATION RESULTS:" "$MASTER_SUMMARY" || true
    echo ""
    grep -A 20 "RUNTIME RESULTS:" "$MASTER_SUMMARY" || true
    echo ""
    grep -A 5 "OVERALL ASSESSMENT:" "$MASTER_SUMMARY" || true
fi

echo ""
echo -e "${BLUE}Comprehensive QA test completed!${NC}"

# Set exit code
if [ "$OVERALL_SUCCESS" = true ]; then
    exit 0
else
    exit 1
fi