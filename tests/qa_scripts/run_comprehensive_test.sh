#!/bin/bash

# Comprehensive QA Testing Script for Clean Language Compiler
# Executes full test suite with detailed metrics and error categorization

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
RESULTS_DIR="$PROJECT_ROOT/tests/qa_results"
REPORT_FILE="$RESULTS_DIR/qa_report_$TIMESTAMP.md"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🧪 Clean Language Compiler - Comprehensive QA Test Suite${NC}"
echo "=================================================="
echo "Timestamp: $(date)"
echo "Project Root: $PROJECT_ROOT"
echo ""

# Create results directory
mkdir -p "$RESULTS_DIR"

# Change to project directory
cd "$PROJECT_ROOT"

# Initialize report file
cat > "$REPORT_FILE" << EOF
# QA Test Report - $TIMESTAMP

## Test Execution Summary

**Date**: $(date)
**Project**: Clean Language Compiler
**Test Suite**: Comprehensive .cln file compilation

EOF

echo -e "${YELLOW}📊 Running baseline assessment...${NC}"

# Run comprehensive test with timeout and capture output
TEST_OUTPUT=$(timeout 120s cargo run --bin clean-language-compiler comprehensive-test 2>&1 || true)

# Extract success rate from output
SUCCESS_RATE=$(echo "$TEST_OUTPUT" | grep -o "Success Rate: [0-9]*%" | tail -1 || echo "Success Rate: 0%")
TESTS_PASSED=$(echo "$TEST_OUTPUT" | grep -o "([0-9]*/[0-9]*)" | tail -1 || echo "(0/0)")

echo -e "${GREEN}✅ Test execution completed${NC}"
echo "Result: $SUCCESS_RATE $TESTS_PASSED"

# Add results to report
cat >> "$REPORT_FILE" << EOF
## Results

- **Success Rate**: $SUCCESS_RATE
- **Tests Status**: $TESTS_PASSED
- **Execution Time**: $(date)

## Detailed Output

\`\`\`
$TEST_OUTPUT
\`\`\`

EOF

# Error pattern analysis
echo -e "${YELLOW}🔍 Analyzing error patterns...${NC}"

# Extract common error patterns
COMPILATION_ERRORS=$(echo "$TEST_OUTPUT" | grep -c "compilation error" || echo "0")
UNDEFINED_VARS=$(echo "$TEST_OUTPUT" | grep -c "Undefined variable" || echo "0")
UNIMPLEMENTED_FEATURES=$(echo "$TEST_OUTPUT" | grep -c "not yet implemented" || echo "0")
SEMANTIC_ERRORS=$(echo "$TEST_OUTPUT" | grep -c "Semantic error" || echo "0")

# Categorize errors by impact
echo -e "${RED}🔴 CRITICAL Issues:${NC}"
echo "  - Compilation errors: $COMPILATION_ERRORS"

echo -e "${YELLOW}🟡 HIGH Priority Issues:${NC}"
echo "  - Undefined variables: $UNDEFINED_VARS"
echo "  - Unimplemented features: $UNIMPLEMENTED_FEATURES"

echo -e "${GREEN}🟢 MEDIUM Priority Issues:${NC}"
echo "  - Semantic errors: $SEMANTIC_ERRORS"

# Add error analysis to report
cat >> "$REPORT_FILE" << EOF

## Error Pattern Analysis

### Critical Issues (🔴)
- **Compilation errors**: $COMPILATION_ERRORS occurrences

### High Priority Issues (🟡)
- **Undefined variables**: $UNDEFINED_VARS occurrences
- **Unimplemented features**: $UNIMPLEMENTED_FEATURES occurrences

### Medium Priority Issues (🟢)  
- **Semantic errors**: $SEMANTIC_ERRORS occurrences

EOF

# Generate recommendations
echo -e "${BLUE}💡 Generating recommendations...${NC}"

if [ "$COMPILATION_ERRORS" -gt 0 ]; then
    echo -e "${RED}⚠️  CRITICAL: Fix compilation errors first${NC}"
fi

if [ "$UNDEFINED_VARS" -gt 10 ]; then
    echo -e "${YELLOW}⚠️  HIGH: Many undefined variable issues - check namespace support${NC}"
fi

if [ "$UNIMPLEMENTED_FEATURES" -gt 10 ]; then
    echo -e "${YELLOW}⚠️  HIGH: Multiple unimplemented features - prioritize by test count${NC}"
fi

# Add recommendations to report
cat >> "$REPORT_FILE" << EOF

## Recommendations

EOF

if [ "$COMPILATION_ERRORS" -gt 0 ]; then
cat >> "$REPORT_FILE" << EOF
### 🔴 CRITICAL Priority
- Fix compilation errors immediately - these block all testing progress
- Focus on non-exhaustive pattern matches in codegen modules
- Ensure all AST Type variants are handled

EOF
fi

if [ "$UNDEFINED_VARS" -gt 10 ]; then
cat >> "$REPORT_FILE" << EOF
### 🟡 HIGH Priority  
- Address undefined variable issues in semantic analysis
- Implement missing namespace support (math, string, etc.)
- Review symbol resolution logic

EOF
fi

if [ "$UNIMPLEMENTED_FEATURES" -gt 10 ]; then
cat >> "$REPORT_FILE" << EOF
### 🟡 HIGH Priority
- Implement missing language features in order of test impact
- Focus on list literals, method calls, expression statements
- Ensure production-grade implementations (no placeholders)

EOF
fi

# Summary and next steps
echo ""
echo -e "${BLUE}📋 QA Test Summary${NC}"
echo "=================="
echo "Success Rate: $SUCCESS_RATE $TESTS_PASSED"
echo "Report saved to: $REPORT_FILE"
echo ""
echo -e "${GREEN}✅ Next Steps:${NC}"
echo "1. Review detailed report: $REPORT_FILE"
echo "2. Address CRITICAL issues first"
echo "3. Implement HIGH priority fixes by impact"
echo "4. Re-run this script to measure progress"
echo "5. Update TASKS.md with findings"

# Add footer to report
cat >> "$REPORT_FILE" << EOF

## Next Steps

1. **Review Results**: Analyze error patterns and prioritize fixes
2. **Address Critical**: Fix compilation errors that block testing
3. **Implement High Priority**: Focus on undefined variables and unimplemented features
4. **Track Progress**: Update TASKS.md with findings and completion status
5. **Retest**: Run this script again after fixes to measure improvement

---

**Generated by**: Clean Language QA Test Suite  
**Script**: tests/qa_scripts/run_comprehensive_test.sh
**Methodology**: tests/COMPREHENSIVE_QA_PROCEDURE.md
EOF

echo ""
echo -e "${GREEN}🎯 QA testing completed successfully!${NC}"