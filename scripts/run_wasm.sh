#!/bin/bash

# Clean Language WASM Runner Script
# Usage: ./scripts/run_wasm.sh <clean_file>

set -e

if [ $# -eq 0 ]; then
    echo "Usage: $0 <clean_file>"
    echo "Example: $0 examples/hello.cln"
    exit 1
fi

CLEAN_FILE="$1"
WASM_FILE="output.wasm"

echo "🔧 Compiling $CLEAN_FILE to WebAssembly..."

# Run in Docker container
docker-compose run --rm compiler cargo run --bin cleanc -- "$CLEAN_FILE"

if [ $? -eq 0 ]; then
    echo "✅ Compilation successful!"
    echo "🚀 Running WebAssembly module..."
    
    # Execute the WASM file
    docker-compose run --rm wasm-runner cargo run --bin wasmtime_runner -- "/app/$WASM_FILE"
else
    echo "❌ Compilation failed!"
    exit 1
fi 