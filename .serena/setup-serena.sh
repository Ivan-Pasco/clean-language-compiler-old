#!/bin/bash

echo "🚀 Setting up Serena for Clean Language Compiler Development"
echo "=========================================================="

# Check if Python is available
if ! command -v python3 &> /dev/null; then
    echo "❌ Python 3 is required but not installed"
    echo "Please install Python 3 and try again"
    exit 1
fi

# Check if pip is available
if ! command -v pip3 &> /dev/null; then
    echo "❌ pip3 is required but not installed"
    echo "Please install pip3 and try again"
    exit 1
fi

echo "✅ Python 3 and pip3 are available"

# Install Serena
echo "📦 Installing Serena..."
pip3 install serena

if [ $? -eq 0 ]; then
    echo "✅ Serena installed successfully"
else
    echo "❌ Failed to install Serena"
    echo "Trying alternative installation method..."
    pip3 install --user serena
fi

# Check if Serena was installed
if ! command -v serena &> /dev/null; then
    echo "❌ Serena installation failed"
    echo "Please check the error messages above and try manual installation:"
    echo "pip3 install serena"
    exit 1
fi

echo "✅ Serena is now available"

# Create .serena directory if it doesn't exist
if [ ! -d ".serena" ]; then
    echo "📁 Creating .serena directory..."
    mkdir -p .serena
fi

# Copy configuration
echo "⚙️  Setting up Serena configuration..."
cp serena-config.json .serena/config.json

echo ""
echo "🎉 Serena setup complete!"
echo ""
echo "Next steps:"
echo "1. Restart Claude Code to load the new MCP permissions"
echo "2. Use the new commands: serena-onboard, serena-analyze, serena-debug"
echo "3. Serena will now have semantic understanding of your Rust codebase"
echo ""
echo "Example usage:"
echo "- Use 'find_symbol' to locate specific functions or types"
echo "- Use 'get_symbols_overview' to understand file structure"
echo "- Use 'find_referencing_symbols' to trace dependencies"
echo "- Use 'search_for_pattern' to find specific code patterns"
echo ""
echo "For more information, visit: https://github.com/oraios/serena"
