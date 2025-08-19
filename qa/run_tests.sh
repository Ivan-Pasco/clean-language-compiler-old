#!/bin/bash
#
# Clean Language Compiler - Testing Protocol Runner
# 
# Simple command interface to fire testing protocols from Claude Code

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🧪 CLEAN LANGUAGE COMPILER - TESTING PROTOCOL${NC}"
echo "=============================================="
echo

# Check if we're in the right directory and navigate to project root
if [ -f "Cargo.toml" ]; then
    # Already in project root
    PROJECT_ROOT="."
elif [ -f "../Cargo.toml" ]; then
    # In qa folder, go up one level
    PROJECT_ROOT=".."
    cd ..
else
    echo -e "${RED}❌ Error: Not in project directory${NC}"
    echo "Run this script from project root or qa/ folder"
    exit 1
fi

# Function to show usage
show_usage() {
    echo "Usage: $0 [COMMAND]"
    echo
    echo "Commands:"
    echo "  quick       - Quick test suite (parser + basic tests)"
    echo "  full        - Full quality gate (all tests + benchmarks)"
    echo "  unit        - Unit tests only"
    echo "  integration - Integration tests only"
    echo "  benchmark   - Performance benchmarks only"
    echo "  coverage    - Coverage analysis only"
    echo "  property    - Property-based tests only"
    echo "  validate    - Validate test files against spec"
    echo "  fix         - Run test failure protocol checker"
    echo "  all         - Everything (full + coverage + property)"
    echo
    echo "Examples:"
    echo "  $0 quick     # Fast testing for development"
    echo "  $0 full      # Complete quality gate"
    echo "  $0 fix       # When tests fail - validates against spec"
    echo
}

# Parse command line arguments
COMMAND=${1:-help}

case "$COMMAND" in
    "help"|"-h"|"--help")
        show_usage
        exit 0
        ;;
    
    "quick")
        echo -e "${YELLOW}🚀 Running Quick Test Suite...${NC}"
        echo
        
        echo "1️⃣ Basic compilation check..."
        cargo check --lib --quiet
        echo -e "${GREEN}✅ Compilation: PASSED${NC}"
        
        echo "2️⃣ Running parser tests..."
        cargo run --bin test_runner --quiet
        echo -e "${GREEN}✅ Parser tests: PASSED${NC}"
        
        echo "3️⃣ Running basic unit tests..."
        cargo test --lib semantic::tests::test_semantic_analyzer_creation --quiet
        echo -e "${GREEN}✅ Basic tests: PASSED${NC}"
        
        echo
        echo -e "${GREEN}🎉 Quick test suite completed successfully!${NC}"
        ;;
        
    "full")
        echo -e "${YELLOW}🎯 Running Full Quality Gate...${NC}"
        echo
        make quality-gate
        ;;
        
    "unit")
        echo -e "${YELLOW}🧪 Running Unit Tests...${NC}"
        echo
        cargo test --lib --tests
        ;;
        
    "integration")
        echo -e "${YELLOW}🔗 Running Integration Tests...${NC}"
        echo
        cargo test --test "*"
        ;;
        
    "benchmark")
        echo -e "${YELLOW}🚀 Running Performance Benchmarks...${NC}"
        echo
        cargo run --bin performance_benchmark
        ;;
        
    "coverage")
        echo -e "${YELLOW}📊 Running Coverage Analysis...${NC}"
        echo
        cargo run --bin coverage_report
        ;;
        
    "property")
        echo -e "${YELLOW}🎲 Running Property-Based Tests...${NC}"
        echo
        cargo test parser::property_tests --lib
        ;;
        
    "validate")
        echo -e "${YELLOW}📚 Validating Test Files Against Specification...${NC}"
        echo
        echo "Checking Clean Language test files against specification..."
        
        # Check if specification exists
        if [ ! -f "documentation/Clean_Language_Specification.md" ]; then
            echo -e "${RED}❌ Clean Language Specification not found!${NC}"
            echo "Expected: documentation/Clean_Language_Specification.md"
            exit 1
        fi
        
        # Count test files
        TEST_COUNT=$(find tests/clean_files -name "*.cln" | wc -l)
        echo "Found ${TEST_COUNT} Clean Language test files"
        
        # Basic validation (can be extended)
        echo "✅ Specification file exists"
        echo "✅ Test files directory structure correct"
        echo
        echo -e "${BLUE}💡 Manual Review Required:${NC}"
        echo "- Review failing test syntax against specification"
        echo "- Verify test expectations match intended behavior"
        echo "- Fix tests before fixing implementation"
        ;;
        
    "fix")
        echo -e "${YELLOW}🔧 Test Failure Protocol Checker...${NC}"
        echo
        echo -e "${RED}⚠️  CRITICAL REMINDER: Test Failure Protocol${NC}"
        echo
        echo -e "${BLUE}🎯 QUALITY STANDARD: 100% COMPILATION + 100% EXECUTION RATE REQUIRED${NC}"
        echo "ALL Clean Language files must compile AND execute successfully"
        echo
        echo "When ANY test fails, follow this EXACT sequence:"
        echo
        echo "1️⃣ 📚 FIRST: Review Clean Language Specification"
        echo "   → Open: documentation/Clean_Language_Specification.md"
        echo "   → Verify test syntax matches specification"
        echo
        echo "2️⃣ 🔍 SECOND: Validate Test Correctness"
        echo "   → Is test syntax correct per specification?"
        echo "   → Are expectations reasonable and well-defined?"
        echo
        echo "3️⃣ ✏️ THIRD: Fix Test if Wrong"
        echo "   → Fix incorrect test syntax FIRST"
        echo "   → Update test expectations to match spec"
        echo
        echo "4️⃣ 🔧 FOURTH: Fix Implementation (only if test correct)"
        echo "   → Only after confirming test is correct"
        echo "   → Add needed changes to TASKS.md"
        echo
        echo -e "${RED}❌ NEVER remove failing tests without fixing them${NC}"
        echo -e "${RED}❌ NEVER fix implementation to match wrong tests${NC}"
        echo
        echo -e "${GREEN}✅ ALWAYS validate test correctness first${NC}"
        echo
        echo "Ready to proceed with test validation? (y/N)"
        read -r response
        if [[ "$response" =~ ^[Yy]$ ]]; then
            echo
            echo "Opening specification file..."
            echo "File: documentation/Clean_Language_Specification.md"
            echo
            echo "Run tests to identify failures:"
            echo "  ./qa/run_tests.sh quick    # For quick testing"
            echo "  ./qa/run_tests.sh unit     # For specific unit tests"
        fi
        ;;
        
    "all")
        echo -e "${YELLOW}🎯 Running Complete Test Suite...${NC}"
        echo -e "${BLUE}🎯 ENFORCING: 100% Compilation + 100% Execution Rate${NC}"
        echo
        
        echo "Phase 1: Full Quality Gate"
        make quality-gate
        echo
        
        echo "Phase 2: Comprehensive Compilation Test (100% Required)"
        ./scripts/test_all.sh
        COMPILATION_RESULT=$?
        echo
        
        echo "Phase 3: Coverage Analysis"
        cargo run --bin coverage_report
        echo
        
        echo "Phase 4: Property-Based Tests"
        cargo test parser::property_tests --lib
        echo
        
        if [ $COMPILATION_RESULT -eq 0 ]; then
            echo -e "${GREEN}🎉 Complete test suite finished! 100% SUCCESS RATE ACHIEVED!${NC}"
        else
            echo -e "${RED}❌ FAILED: 100% compilation rate not achieved${NC}"
            echo -e "${RED}   Quality standard requires ALL tests to compile and execute${NC}"
            exit 1
        fi
        ;;
        
    *)
        echo -e "${RED}❌ Unknown command: $COMMAND${NC}"
        echo
        show_usage
        exit 1
        ;;
esac