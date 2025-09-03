#!/bin/bash

# Progress Report Generator for Clean Language Compiler QA
# Compares current test results with historical data to show improvement trends

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RESULTS_DIR="$PROJECT_ROOT/tests/qa_results"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
PROGRESS_REPORT="$RESULTS_DIR/progress_report_$TIMESTAMP.md"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${CYAN}📈 Clean Language Compiler - Progress Report Generator${NC}"
echo "========================================================"
echo "Timestamp: $(date)"
echo ""

# Ensure results directory exists
mkdir -p "$RESULTS_DIR"

# Change to project directory  
cd "$PROJECT_ROOT"

echo -e "${YELLOW}📊 Collecting current test results...${NC}"

# Run current comprehensive test
CURRENT_OUTPUT=$(timeout 120s cargo run --bin clean-language-compiler comprehensive-test 2>&1 || true)
CURRENT_RATE=$(echo "$CURRENT_OUTPUT" | grep -o "Success Rate: [0-9]*%" | tail -1 || echo "Success Rate: 0%")
CURRENT_TESTS=$(echo "$CURRENT_OUTPUT" | grep -o "([0-9]*/[0-9]*)" | tail -1 || echo "(0/0)")

# Extract numeric values for calculations
CURRENT_PERCENT=$(echo "$CURRENT_RATE" | grep -o "[0-9]*" || echo "0")
CURRENT_PASSED=$(echo "$CURRENT_TESTS" | sed 's/[^0-9]*\([0-9]*\).*/\1/' || echo "0")
CURRENT_TOTAL=$(echo "$CURRENT_TESTS" | sed 's/.*\/\([0-9]*\).*/\1/' || echo "0")

echo "Current Status: $CURRENT_RATE $CURRENT_TESTS"

echo -e "${YELLOW}📚 Analyzing historical data...${NC}"

# Find all previous QA reports
HISTORICAL_REPORTS=$(find "$RESULTS_DIR" -name "qa_report_*.md" | sort | tail -5 || echo "")

# Initialize report
cat > "$PROGRESS_REPORT" << EOF
# QA Progress Report - $TIMESTAMP

## Executive Summary

**Current Status**: $CURRENT_RATE ($CURRENT_TESTS tests)
**Report Generated**: $(date)

EOF

# Historical comparison if reports exist
if [ -n "$HISTORICAL_REPORTS" ]; then
    echo -e "${BLUE}🔍 Comparing with historical data...${NC}"
    
    # Get data from most recent previous report
    LATEST_HISTORICAL=$(echo "$HISTORICAL_REPORTS" | tail -1)
    
    if [ -f "$LATEST_HISTORICAL" ] && [ "$LATEST_HISTORICAL" != "$PROGRESS_REPORT" ]; then
        PREV_RATE=$(grep -o "Success Rate: [0-9]*%" "$LATEST_HISTORICAL" | head -1 || echo "Success Rate: 0%")
        PREV_TESTS=$(grep -o "([0-9]*/[0-9]*)" "$LATEST_HISTORICAL" | head -1 || echo "(0/0)")
        PREV_PERCENT=$(echo "$PREV_RATE" | grep -o "[0-9]*" || echo "0")
        PREV_PASSED=$(echo "$PREV_TESTS" | sed 's/[^0-9]*\([0-9]*\).*/\1/' || echo "0")
        
        # Calculate improvement
        PERCENT_CHANGE=$((CURRENT_PERCENT - PREV_PERCENT))
        TESTS_CHANGE=$((CURRENT_PASSED - PREV_PASSED))
        
        echo "Previous Status: $PREV_RATE $PREV_TESTS"
        
        # Add comparison to report
        cat >> "$PROGRESS_REPORT" << EOF
## Progress Comparison

### Latest Results
- **Current**: $CURRENT_RATE ($CURRENT_TESTS tests)
- **Previous**: $PREV_RATE ($PREV_TESTS tests)

### Improvement Metrics
EOF
        
        if [ $PERCENT_CHANGE -gt 0 ]; then
            echo -e "${GREEN}✅ Improvement: +${PERCENT_CHANGE}% success rate${NC}"
            echo "- **Success Rate**: +${PERCENT_CHANGE}% improvement" >> "$PROGRESS_REPORT"
        elif [ $PERCENT_CHANGE -lt 0 ]; then
            echo -e "${RED}⚠️  Regression: ${PERCENT_CHANGE}% success rate${NC}"
            echo "- **Success Rate**: ${PERCENT_CHANGE}% regression" >> "$PROGRESS_REPORT"
        else
            echo -e "${YELLOW}➡️  No change in success rate${NC}"
            echo "- **Success Rate**: No change (${CURRENT_PERCENT}%)" >> "$PROGRESS_REPORT"
        fi
        
        if [ $TESTS_CHANGE -gt 0 ]; then
            echo -e "${GREEN}✅ Tests: +${TESTS_CHANGE} additional tests passing${NC}"
            echo "- **Tests Passing**: +${TESTS_CHANGE} additional tests" >> "$PROGRESS_REPORT"
        elif [ $TESTS_CHANGE -lt 0 ]; then
            echo -e "${RED}⚠️  Tests: ${TESTS_CHANGE} fewer tests passing${NC}"
            echo "- **Tests Passing**: ${TESTS_CHANGE} fewer tests" >> "$PROGRESS_REPORT"
        else
            echo -e "${YELLOW}➡️  No change in passing tests${NC}"
            echo "- **Tests Passing**: No change (${CURRENT_PASSED} tests)" >> "$PROGRESS_REPORT"
        fi
        
        echo "" >> "$PROGRESS_REPORT"
    fi
else
    echo -e "${YELLOW}ℹ️  No historical data found - this is the baseline${NC}"
    cat >> "$PROGRESS_REPORT" << EOF
## Progress Comparison

*This is the first progress report - establishing baseline metrics*

### Baseline Metrics
- **Success Rate**: $CURRENT_RATE
- **Tests Status**: $CURRENT_TESTS
- **Total Tests**: $CURRENT_TOTAL

EOF
fi

# Trend analysis if multiple reports exist
REPORT_COUNT=$(echo "$HISTORICAL_REPORTS" | wc -w)
if [ "$REPORT_COUNT" -ge 3 ]; then
    echo -e "${BLUE}📈 Generating trend analysis...${NC}"
    
    cat >> "$PROGRESS_REPORT" << EOF
## Trend Analysis

### Historical Success Rates
EOF
    
    # Extract success rates from multiple reports
    for report in $HISTORICAL_REPORTS; do
        if [ -f "$report" ]; then
            report_date=$(basename "$report" | sed 's/qa_report_\([0-9]*_[0-9]*\)\.md/\1/' | sed 's/_/ /')
            success_rate=$(grep -o "Success Rate: [0-9]*%" "$report" | head -1 || echo "Success Rate: 0%")
            tests_status=$(grep -o "([0-9]*/[0-9]*)" "$report" | head -1 || echo "(0/0)")
            echo "- **$report_date**: $success_rate $tests_status" >> "$PROGRESS_REPORT"
        fi
    done
    
    echo "- **Current ($TIMESTAMP)**: $CURRENT_RATE $CURRENT_TESTS" >> "$PROGRESS_REPORT"
    echo "" >> "$PROGRESS_REPORT"
fi

# Quality gates assessment
echo -e "${BLUE}🎯 Assessing quality gates...${NC}"

cat >> "$PROGRESS_REPORT" << EOF
## Quality Gates Assessment

EOF

# Define quality thresholds
PRODUCTION_READY_THRESHOLD=95
HIGH_QUALITY_THRESHOLD=85
BASELINE_THRESHOLD=50

if [ "$CURRENT_PERCENT" -ge "$PRODUCTION_READY_THRESHOLD" ]; then
    echo -e "${GREEN}🎉 PRODUCTION READY: Success rate ≥ ${PRODUCTION_READY_THRESHOLD}%${NC}"
    echo "### 🟢 PRODUCTION READY ✅" >> "$PROGRESS_REPORT"
    echo "Success rate of ${CURRENT_PERCENT}% meets production quality standards." >> "$PROGRESS_REPORT"
elif [ "$CURRENT_PERCENT" -ge "$HIGH_QUALITY_THRESHOLD" ]; then
    echo -e "${YELLOW}🚀 HIGH QUALITY: Success rate ≥ ${HIGH_QUALITY_THRESHOLD}%${NC}"
    echo "### 🟡 HIGH QUALITY - Near Production Ready" >> "$PROGRESS_REPORT"
    echo "Success rate of ${CURRENT_PERCENT}% indicates high quality. $(($PRODUCTION_READY_THRESHOLD - $CURRENT_PERCENT))% improvement needed for production readiness." >> "$PROGRESS_REPORT"
elif [ "$CURRENT_PERCENT" -ge "$BASELINE_THRESHOLD" ]; then
    echo -e "${YELLOW}⚠️  DEVELOPMENT: Success rate ≥ ${BASELINE_THRESHOLD}%${NC}"
    echo "### 🟡 DEVELOPMENT QUALITY" >> "$PROGRESS_REPORT"
    echo "Success rate of ${CURRENT_PERCENT}% shows progress. Continue systematic improvements." >> "$PROGRESS_REPORT"
else
    echo -e "${RED}🔴 CRITICAL: Success rate < ${BASELINE_THRESHOLD}%${NC}"
    echo "### 🔴 CRITICAL - Immediate Action Required" >> "$PROGRESS_REPORT"
    echo "Success rate of ${CURRENT_PERCENT}% requires immediate attention to critical issues." >> "$PROGRESS_REPORT"
fi

echo "" >> "$PROGRESS_REPORT"

# Recommendations based on current status
echo -e "${BLUE}💡 Generating recommendations...${NC}"

cat >> "$PROGRESS_REPORT" << EOF
## Recommendations

### Immediate Actions
EOF

if [ "$CURRENT_PERCENT" -lt "$BASELINE_THRESHOLD" ]; then
    cat >> "$PROGRESS_REPORT" << EOF
1. **🔴 CRITICAL**: Focus on compilation-blocking errors
2. **🔴 CRITICAL**: Fix non-exhaustive pattern matches
3. **🔴 CRITICAL**: Implement core missing language features
4. **Target**: Achieve >50% success rate before feature development

EOF
elif [ "$CURRENT_PERCENT" -lt "$HIGH_QUALITY_THRESHOLD" ]; then
    cat >> "$PROGRESS_REPORT" << EOF
1. **🟡 HIGH**: Implement remaining unimplemented features
2. **🟡 HIGH**: Fix undefined variable/namespace issues  
3. **🟡 HIGH**: Address semantic analysis gaps
4. **Target**: Achieve >85% success rate for pre-production quality

EOF
else
    cat >> "$PROGRESS_REPORT" << EOF
1. **🟢 MEDIUM**: Polish edge cases and error handling
2. **🟢 MEDIUM**: Optimize performance and memory usage
3. **🟢 MEDIUM**: Complete comprehensive language specification coverage
4. **Target**: Achieve >95% success rate for production readiness

EOF
fi

# Next steps
cat >> "$PROGRESS_REPORT" << EOF
### Next Steps
1. **Run Error Analysis**: \`python3 tests/qa_scripts/categorize_errors.py\`
2. **Update TASKS.md**: Document priority issues and fixes
3. **Implement Fixes**: Follow systematic QA procedure
4. **Retest Progress**: Re-run this script to measure improvement
5. **Iterate**: Repeat until quality gates are met

---

**Generated by**: QA Progress Report Generator  
**Script**: tests/qa_scripts/generate_progress_report.sh  
**QA Methodology**: tests/COMPREHENSIVE_QA_PROCEDURE.md
EOF

echo ""
echo -e "${GREEN}📋 Progress Report Summary${NC}"
echo "=========================="
echo "Current Status: $CURRENT_RATE $CURRENT_TESTS"

if [ -n "$PREV_RATE" ]; then
    if [ $PERCENT_CHANGE -gt 0 ]; then
        echo -e "Improvement: ${GREEN}+${PERCENT_CHANGE}%${NC} success rate"
    elif [ $PERCENT_CHANGE -lt 0 ]; then
        echo -e "Change: ${RED}${PERCENT_CHANGE}%${NC} success rate"
    else
        echo -e "Change: ${YELLOW}No change${NC} in success rate"
    fi
fi

echo "Report saved to: $PROGRESS_REPORT"
echo ""
echo -e "${GREEN}✅ Progress analysis completed!${NC}"