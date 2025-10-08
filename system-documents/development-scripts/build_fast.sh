#!/bin/bash

echo "🚀 Clean Language Compiler - Fast Development Build"
echo "=================================================="
echo

# Build with minimal dependencies for fastest compilation
echo "Building core compiler (fastest build)..."
time cargo build --no-default-features --features core

echo
echo "✅ Fast build complete!"
echo
echo "📊 Available features:"
echo "  --no-default-features --features core      # Fastest (parsing + basic codegen only)"
echo "  --features runtime                         # Add WASM execution support"  
echo "  --features optimization                    # Add WASM optimization"
echo "  --features development                     # Add debug logging"
echo "  --features networking                      # Add HTTP standard library"
echo
echo "💡 For development, use: cargo build --no-default-features --features core"
echo "💡 For testing, add: --features runtime"