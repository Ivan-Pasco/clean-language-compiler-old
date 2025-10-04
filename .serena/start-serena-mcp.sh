#!/bin/bash

echo "🚀 Starting Serena MCP Server for Clean Language Compiler"
echo "========================================================="

# Check if we're in the serena conda environment
if [[ "$CONDA_DEFAULT_ENV" != "serena" ]]; then
    echo "⚠️  Activating serena conda environment..."
    source ~/anaconda3/etc/profile.d/conda.sh
    conda activate serena
fi

# Check if Serena is available
if ! command -v serena &> /dev/null; then
    echo "❌ Serena not found. Please run setup-serena.sh first."
    exit 1
fi

echo "✅ Serena found: $(serena --version 2>/dev/null || echo 'version info not available')"

# Check if project is configured
if [ ! -f ".serena/project.yml" ]; then
    echo "❌ Project not configured. Please run setup-serena.sh first."
    exit 1
fi

echo "✅ Project configuration found"

# Start MCP server
echo "🌐 Starting MCP server..."
echo "   - Project: clean-language-compiler"
echo "   - Context: compiler-development"
echo "   - Modes: desktop-app, code-analysis"
echo ""
echo "📋 MCP Server will be available via stdio transport"
echo "   Use this with Claude Code MCP integration"
echo ""
echo "🔄 Server starting... (Press Ctrl+C to stop)"

# Start the MCP server
serena start-mcp-server \
    --project . \
    --context compiler-development \
    --mode desktop-app,code-analysis \
    --transport stdio
