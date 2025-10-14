#!/bin/bash

# 🧪 Quick Test Command
# Triggers the comprehensive testing strategy
# Usage: ./scripts/test.sh
# Or when user types "test" - this script is automatically executed

echo "🧪 Executing Comprehensive Testing Strategy..."
echo "==============================================="

# Change to project root directory
cd "$(dirname "$0")/.."

# Execute the comprehensive test runner
exec ./scripts/comprehensive_test_runner.sh "$@"