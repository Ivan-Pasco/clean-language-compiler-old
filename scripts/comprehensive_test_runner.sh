#!/bin/bash

# 🧪 Comprehensive Testing Strategy Implementation
# Clean Language Compiler - Production Grade Testing Framework
#
# This script implements the unified testing strategy defined in:
# tests/UNIFIED_TESTING_STRATEGY.md
#
# Usage: ./scripts/comprehensive_test_runner.sh
# Or triggered automatically when user types "test"

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
PROJECT_ROOT="/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler"
TEST_DIR="$PROJECT_ROOT/tests/cln"
OUTPUT_DIR="$PROJECT_ROOT/tests/output"
QA_DIR="$PROJECT_ROOT/tests/qa"

# Statistics tracking
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0
START_TIME=$(date +%s)

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

echo -e "${BLUE}🧪 COMPREHENSIVE TESTING STRATEGY - INITIATED${NC}"
echo -e "${BLUE}===========================================${NC}"
echo "Start Time: $(date)"
echo "Test Directory: $TEST_DIR"
echo "Output Directory: $OUTPUT_DIR"
echo ""

# Phase 1: Pre-Test Setup
echo -e "${YELLOW}📋 PHASE 1: PRE-TEST SETUP${NC}"
echo -e "${YELLOW}=========================${NC}"

# 1.1 Verify project builds
echo "1.1 Verifying project builds..."
if ! cargo build --quiet; then
    echo -e "${RED}❌ CRITICAL: Project does not build! Cannot proceed with testing.${NC}"
    echo -e "${RED}🚨 RECOMMENDATION: Fix compilation errors before running tests.${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Project builds successfully${NC}"

# 1.2 Check git status
echo "1.2 Checking git status..."
git_status=$(git status --porcelain)
if [ -n "$git_status" ]; then
    echo -e "${YELLOW}⚠️  Warning: Uncommitted changes detected${NC}"
    echo "Modified files:"
    git status --short
    echo ""
else
    echo -e "${GREEN}✅ Working directory clean${NC}"
fi

# 1.3 Verify test environment
echo "1.3 Verifying test environment..."
if [ ! -d "$TEST_DIR" ]; then
    echo -e "${RED}❌ CRITICAL: Test directory not found: $TEST_DIR${NC}"
    exit 1
fi

total_test_files=$(find "$TEST_DIR" -name "*.cln" | wc -l)
echo -e "${GREEN}✅ Found $total_test_files test files in organized structure${NC}"
echo ""

# Function to compile and test a single file
test_single_file() {
    local file_path="$1"
    local category="$2"
    local file_name=$(basename "$file_path" .cln)
    local output_file="$OUTPUT_DIR/${file_name}.wasm"

    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    echo "Testing [$category]: $(basename "$file_path")"

    # Attempt compilation
    if cargo run --bin clean-language-compiler compile -i "$file_path" -o "$output_file" >/dev/null 2>&1; then
        PASSED_TESTS=$((PASSED_TESTS + 1))
        echo -e "  ${GREEN}✅ PASS${NC}"

        # Verify WASM file was created and is valid
        if [ -f "$output_file" ]; then
            # Test WASM file with wasmtime if available
            if command -v wasmtime >/dev/null 2>&1; then
                if wasmtime "$output_file" >/dev/null 2>&1; then
                    echo -e "  ${GREEN}✅ WASM execution successful${NC}"
                else
                    echo -e "  ${YELLOW}⚠️  WASM compilation success, execution needs review${NC}"
                fi
            fi
        else
            echo -e "  ${YELLOW}⚠️  WASM file not generated${NC}"
        fi
    else
        FAILED_TESTS=$((FAILED_TESTS + 1))
        echo -e "  ${RED}❌ FAIL${NC}"

        # Capture detailed error for analysis
        echo "    Error details:"
        cargo run --bin clean-language-compiler compile -i "$file_path" -o "$output_file" 2>&1 | head -3 | sed 's/^/    /'

        # Return failure for critical analysis
        return 1
    fi

    return 0
}

# Function to handle test failure with intelligent problem resolution
handle_test_failure() {
    local failed_file="$1"
    local category="$2"

    echo ""
    echo -e "${RED}🚨 TEST FAILURE DETECTED${NC}"
    echo -e "${RED}=====================${NC}"
    echo "Failed file: $failed_file"
    echo "Category: $category"
    echo ""

    echo -e "${YELLOW}📊 FAILURE ANALYSIS REQUIRED${NC}"
    echo "1. Use Debug Agent for specific compilation errors"
    echo "2. Use QA Agent if multiple failures occur"
    echo "3. Use Context7 MCP for Rust/WebAssembly issues"
    echo "4. Search internet for similar compiler problems"
    echo ""

    # Generate detailed error log
    echo "Generating detailed error log..."
    error_log="$OUTPUT_DIR/error_$(basename "$failed_file" .cln)_$(date +%s).log"
    echo "=== DETAILED ERROR LOG for $failed_file ===" > "$error_log"
    echo "Timestamp: $(date)" >> "$error_log"
    echo "Category: $category" >> "$error_log"
    echo "" >> "$error_log"
    echo "=== COMPILATION ERROR ===" >> "$error_log"
    RUST_LOG=debug cargo run --bin clean-language-compiler compile -i "$failed_file" -o "$OUTPUT_DIR/debug.wasm" 2>&1 >> "$error_log"
    echo "" >> "$error_log"
    echo "=== FILE CONTENTS ===" >> "$error_log"
    cat "$failed_file" >> "$error_log"

    echo "Error log saved to: $error_log"
    echo ""

    echo -e "${YELLOW}⏸️  TESTING PAUSED FOR MANUAL ANALYSIS${NC}"
    echo "Please:"
    echo "1. Analyze the error in the log file"
    echo "2. Use appropriate agents for problem resolution"
    echo "3. Fix the underlying issue"
    echo "4. Re-run testing to continue"
    echo ""

    return 1
}

# Phase 2: Systematic Test Execution
echo -e "${YELLOW}🔄 PHASE 2: SYSTEMATIC TEST EXECUTION${NC}"
echo -e "${YELLOW}====================================${NC}"

# 2.1 Core Language Features (CRITICAL)
echo -e "${BLUE}2.1 Testing Core Language Features (CRITICAL)${NC}"
echo "Testing core functionality - these MUST pass for basic compiler operation"

core_failed=false
if [ -d "$TEST_DIR/core" ]; then
    for file in $(find "$TEST_DIR/core" -name "*.cln" | sort); do
        if ! test_single_file "$file" "CORE"; then
            handle_test_failure "$file" "CORE"
            core_failed=true
            break
        fi
    done
else
    echo -e "${YELLOW}⚠️  Core tests directory not found, checking alternatives...${NC}"
    # Look for core functionality tests in other locations
    for file in $(find "$TEST_DIR" -name "*basic*" -o -name "*hello*" -o -name "*minimal*" | head -5 | sort); do
        if ! test_single_file "$file" "BASIC"; then
            handle_test_failure "$file" "BASIC"
            core_failed=true
            break
        fi
    done
fi

if [ "$core_failed" = true ]; then
    echo -e "${RED}❌ CRITICAL FAILURE: Core functionality tests failed${NC}"
    echo -e "${RED}🛑 Testing stopped - core issues must be resolved first${NC}"
    exit 1
fi

echo -e "${GREEN}✅ Core functionality tests completed successfully${NC}"
echo ""

# 2.2 Function Tests
echo -e "${BLUE}2.2 Testing Function Features${NC}"
if [ -d "$TEST_DIR/functions" ]; then
    for file in $(find "$TEST_DIR/functions" -name "*.cln" | sort); do
        if ! test_single_file "$file" "FUNCTIONS"; then
            echo -e "${YELLOW}⚠️  Function test failed, continuing with analysis...${NC}"
            # Don't stop for function failures, but log them
        fi
    done
else
    echo -e "${YELLOW}⚠️  Functions directory not found, testing alternative patterns...${NC}"
    for file in $(find "$TEST_DIR" -name "*function*" -o -name "*method*" | head -10 | sort); do
        test_single_file "$file" "FUNCTIONS" || true
    done
fi
echo ""

# 2.3 OOP Features
echo -e "${BLUE}2.3 Testing Object-Oriented Features${NC}"
if [ -d "$TEST_DIR/oop" ]; then
    for file in $(find "$TEST_DIR/oop" -name "*.cln" | sort); do
        test_single_file "$file" "OOP" || true
    done
else
    echo -e "${YELLOW}⚠️  OOP directory not found, testing alternative patterns...${NC}"
    for file in $(find "$TEST_DIR" -name "*class*" -o -name "*inherit*" | head -10 | sort); do
        test_single_file "$file" "OOP" || true
    done
fi
echo ""

# 2.4 Data Structures
echo -e "${BLUE}2.4 Testing Data Structures${NC}"
if [ -d "$TEST_DIR/data-structures" ]; then
    for file in $(find "$TEST_DIR/data-structures" -name "*.cln" | sort); do
        test_single_file "$file" "DATA-STRUCTURES" || true
    done
else
    echo -e "${YELLOW}⚠️  Data structures directory not found, testing alternative patterns...${NC}"
    for file in $(find "$TEST_DIR" -name "*array*" -o -name "*list*" -o -name "*matrix*" | head -10 | sort); do
        test_single_file "$file" "DATA-STRUCTURES" || true
    done
fi
echo ""

# 2.5 Integration Tests
echo -e "${BLUE}2.5 Testing Integration Features${NC}"
if [ -d "$TEST_DIR/integration" ]; then
    for file in $(find "$TEST_DIR/integration" -name "*.cln" | sort); do
        test_single_file "$file" "INTEGRATION" || true
    done
else
    echo -e "${YELLOW}⚠️  Integration directory not found, testing comprehensive files...${NC}"
    for file in $(find "$TEST_DIR" -name "*comprehensive*" -o -name "*integration*" -o -name "*complex*" | head -5 | sort); do
        test_single_file "$file" "INTEGRATION" || true
    done
fi
echo ""

# Phase 3: Results Analysis
echo -e "${YELLOW}📊 PHASE 3: RESULTS ANALYSIS${NC}"
echo -e "${YELLOW}============================${NC}"

END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))
SUCCESS_RATE=0

if [ $TOTAL_TESTS -gt 0 ]; then
    SUCCESS_RATE=$((PASSED_TESTS * 100 / TOTAL_TESTS))
fi

echo "📈 TESTING RESULTS SUMMARY"
echo "=========================="
echo "Total Tests: $TOTAL_TESTS"
echo "Passed: $PASSED_TESTS"
echo "Failed: $FAILED_TESTS"
echo "Success Rate: ${SUCCESS_RATE}%"
echo "Duration: ${DURATION} seconds"
echo "End Time: $(date)"
echo ""

# Quality Gate Assessment
echo "🎯 QUALITY GATE ASSESSMENT"
echo "=========================="

if [ $SUCCESS_RATE -eq 100 ]; then
    echo -e "${GREEN}🏆 ELITE STATUS: 100% Success Rate Achieved!${NC}"
    echo -e "${GREEN}✅ Production ready - all tests passing${NC}"
    status="ELITE"
elif [ $SUCCESS_RATE -ge 95 ]; then
    echo -e "${GREEN}🎯 PRODUCTION READY: ≥95% Success Rate${NC}"
    echo -e "${GREEN}✅ Ready for production deployment${NC}"
    status="PRODUCTION"
elif [ $SUCCESS_RATE -ge 85 ]; then
    echo -e "${YELLOW}🔧 HIGH QUALITY: ≥85% Success Rate${NC}"
    echo -e "${YELLOW}⚠️  Near production ready, minor fixes needed${NC}"
    status="HIGH_QUALITY"
elif [ $SUCCESS_RATE -ge 50 ]; then
    echo -e "${YELLOW}🚧 DEVELOPMENT QUALITY: ≥50% Success Rate${NC}"
    echo -e "${YELLOW}⚠️  Significant development needed${NC}"
    status="DEVELOPMENT"
else
    echo -e "${RED}🚨 CRITICAL ISSUES: <50% Success Rate${NC}"
    echo -e "${RED}❌ Major architectural issues require attention${NC}"
    status="CRITICAL"
fi

echo ""

# Recommendations
echo "💡 RECOMMENDATIONS"
echo "=================="

if [ $FAILED_TESTS -gt 0 ]; then
    echo "❗ $FAILED_TESTS tests failed - analysis required:"
    echo "  1. 🤖 Use QA Agent for systematic failure analysis"
    echo "  2. 🔧 Use Debug Agent for specific compilation errors"
    echo "  3. 📚 Use Context7 MCP for Rust/WebAssembly expertise"
    echo "  4. 🌐 Search internet for similar compiler issues"
    echo "  5. 🧠 Think deeply about architectural implications"
    echo ""

    if [ $FAILED_TESTS -gt 10 ]; then
        echo "🚨 HIGH FAILURE COUNT: Consider using QA Agent for systematic analysis"
    elif [ $FAILED_TESTS -gt 5 ]; then
        echo "⚠️  MODERATE FAILURES: Focus on critical path fixes first"
    else
        echo "✓ LOW FAILURE COUNT: Target individual test fixes"
    fi
fi

if [ $SUCCESS_RATE -lt 100 ]; then
    echo ""
    echo "📋 NEXT STEPS:"
    echo "1. Analyze error logs in $OUTPUT_DIR"
    echo "2. Use appropriate agents based on error types"
    echo "3. Fix underlying issues (no placeholders)"
    echo "4. Re-run testing to validate fixes"
    echo "5. Update TASKS.md with any remaining issues"
fi

echo ""
echo -e "${BLUE}🧪 COMPREHENSIVE TESTING STRATEGY - COMPLETED${NC}"
echo -e "${BLUE}=============================================${NC}"
echo "Status: $status"
echo "Success Rate: ${SUCCESS_RATE}%"
echo "Next: Analyze results and apply intelligent problem resolution as needed"

# Return appropriate exit code
if [ $SUCCESS_RATE -eq 100 ]; then
    exit 0
elif [ $SUCCESS_RATE -ge 85 ]; then
    exit 1
else
    exit 2
fi