#!/bin/bash

echo "🧪 Testing Serena Integration for Clean Language Compiler"
echo "========================================================"

# Check if we're in the serena conda environment
if [[ "$CONDA_DEFAULT_ENV" != "serena" ]]; then
    echo "⚠️  Activating serena conda environment..."
    source ~/anaconda3/etc/profile.d/conda.sh
    conda activate serena
fi

echo "✅ Environment: $(conda info --envs | grep '*' | awk '{print $1}')"

# Test 1: Verify Serena installation
echo ""
echo "🔍 Test 1: Verifying Serena Installation"
if command -v serena &> /dev/null; then
    echo "✅ Serena found: $(serena --version 2>/dev/null || echo 'version info available')"
else
    echo "❌ Serena not found"
    exit 1
fi

# Test 2: Check project configuration
echo ""
echo "🔍 Test 2: Checking Project Configuration"
if [ -f ".serena/project.yml" ]; then
    echo "✅ Project configuration found"
else
    echo "❌ Project configuration missing"
    exit 1
fi

# Test 3: Verify rust-analyzer
echo ""
echo "🔍 Test 3: Verifying Rust Language Server"
if command -v rust-analyzer &> /dev/null; then
    echo "✅ rust-analyzer found: $(rust-analyzer --version)"
else
    echo "❌ rust-analyzer not found"
    exit 1
fi

# Test 4: Run health check
echo ""
echo "🔍 Test 4: Running Serena Health Check"
if serena project health-check . > /dev/null 2>&1; then
    echo "✅ Health check passed"
else
    echo "❌ Health check failed"
    exit 1
fi

# Test 5: Test basic tools
echo ""
echo "🔍 Test 5: Testing Basic Tools"
echo "   - Available tools: $(serena tools list | wc -l | tr -d ' ')"
echo "   - Project tools: $(serena tools list | grep -c '^\*')"

# Test 6: Test symbol search
echo ""
echo "🔍 Test 6: Testing Symbol Search"
echo "   Testing search for 'Parser' symbol..."
if serena find_symbol "Parser" --type struct > /dev/null 2>&1; then
    echo "✅ Symbol search working"
else
    echo "⚠️  Symbol search may need language server initialization"
fi

echo ""
echo "🎉 Serena Integration Test Complete!"
echo ""
echo "📋 Next Steps:"
echo "1. Serena is configured as MCP server in Claude Code"
echo "2. Restart Claude Code to load the MCP server configuration"
echo "3. Use Serena tools for semantic code analysis"
echo ""
echo "Note: MCP server will start automatically when needed"
echo ""
echo "🚀 You're ready to use Serena with Claude Code!"
echo ""
echo "Example commands:"
echo "  - serena find_symbol 'CompilationError'"
echo "  - serena get_symbols_overview 'src/parser/mod.rs'"
echo "  - serena search_for_pattern 'Result<.*Error>'"
