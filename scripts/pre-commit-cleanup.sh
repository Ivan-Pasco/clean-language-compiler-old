#!/bin/bash

# Clean Language Compiler - Pre-Commit Cleanup Script
# This script ensures proper repository hygiene before git commits
# Run this before every git commit to maintain a clean repository

set -e

echo "🧹 CLEAN LANGUAGE COMPILER - PRE-COMMIT CLEANUP"
echo "==============================================="
echo ""

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || ! grep -q "clean-language-compiler" Cargo.toml; then
    echo "❌ Error: Must be run from the clean-language-compiler root directory"
    exit 1
fi

echo "📊 Repository state before cleanup:"
echo "   Size: $(du -sh . | cut -f1)"
echo ""

# Step 1: Cargo clean
echo "🧹 STEP 1: CARGO CLEAN"
echo "======================"
if [ -d "target" ]; then
    echo "   Cleaning build artifacts..."
    cargo clean
    echo "   ✅ Build artifacts cleaned"
else
    echo "   ✅ No target directory found (already clean)"
fi
echo ""

# Step 2: Delete .cln and .wasm files from project root
echo "🧹 STEP 2: DELETE .CLN AND .WASM FILES FROM ROOT"
echo "================================================"

cln_count=$(find . -maxdepth 1 -name "*.cln" -type f | wc -l | tr -d ' ')
wasm_count=$(find . -maxdepth 1 -name "*.wasm" -type f | wc -l | tr -d ' ')

if [ "$cln_count" -gt 0 ]; then
    echo "   Removing $cln_count .cln files from root..."
    find . -maxdepth 1 -name "*.cln" -type f -delete
    echo "   ✅ Root .cln files removed"
else
    echo "   ✅ No .cln files in root"
fi

if [ "$wasm_count" -gt 0 ]; then
    echo "   Removing $wasm_count .wasm files from root..."
    find . -maxdepth 1 -name "*.wasm" -type f -delete
    echo "   ✅ Root .wasm files removed"
else
    echo "   ✅ No .wasm files in root"
fi
echo ""

# Step 3: Delete test and debug files
echo "🧹 STEP 3: DELETE TEMPORARY AND DEBUG FILES"
echo "==========================================="

# Count temporary files
temp_count=$(find . -maxdepth 1 \( -name "test_*" -o -name "debug_*" -o -name "qa_*" -o -name "simple_*" -o -name "*.tmp" \) | wc -l | tr -d ' ')

if [ "$temp_count" -gt 0 ]; then
    echo "   Removing $temp_count temporary/debug files..."
    find . -maxdepth 1 \( -name "test_*" -o -name "debug_*" -o -name "qa_*" -o -name "simple_*" -o -name "*.tmp" \) -delete
    echo "   ✅ Temporary files removed"
else
    echo "   ✅ No temporary files found"
fi

# Remove tmp directory if it exists
if [ -d "tmp" ]; then
    echo "   Removing tmp/ directory..."
    rm -rf tmp/
    echo "   ✅ tmp/ directory removed"
else
    echo "   ✅ No tmp/ directory found"
fi

# Remove other temporary files
temp_txt_count=$(find . -maxdepth 1 -name "*.log" -o -name "compilation_*.txt" -o -name "qa_*.txt" | wc -l | tr -d ' ')
if [ "$temp_txt_count" -gt 0 ]; then
    echo "   Removing $temp_txt_count temporary text files..."
    find . -maxdepth 1 \( -name "*.log" -o -name "compilation_*.txt" -o -name "qa_*.txt" \) -delete
    echo "   ✅ Temporary text files removed"
fi
echo ""

# Step 4: Verification
echo "🧹 CLEANUP VERIFICATION"
echo "======================="
final_size=$(du -sh . | cut -f1)
echo "   Repository size after cleanup: $final_size"
echo ""

# Final checks
remaining_cln=$(find . -maxdepth 1 -name "*.cln" -type f | wc -l | tr -d ' ')
remaining_wasm=$(find . -maxdepth 1 -name "*.wasm" -type f | wc -l | tr -d ' ')
remaining_temp=$(find . -maxdepth 1 \( -name "test_*" -o -name "debug_*" -o -name "qa_*" -o -name "simple_*" \) | wc -l | tr -d ' ')

echo "   Final state verification:"
echo "   • .cln files in root: $remaining_cln ✅"
echo "   • .wasm files in root: $remaining_wasm ✅" 
echo "   • Test/debug files: $remaining_temp ✅"
echo "   • tmp/ directory: $([ -d "tmp" ] && echo "EXISTS ❌" || echo "REMOVED ✅")"
echo "   • target/ directory: $([ -d "target" ] && echo "EXISTS ❌" || echo "REMOVED ✅")"
echo ""

if [ "$remaining_cln" -eq 0 ] && [ "$remaining_wasm" -eq 0 ] && [ "$remaining_temp" -eq 0 ] && [ ! -d "tmp" ] && [ ! -d "target" ]; then
    echo "🎉 CLEANUP COMPLETE - REPOSITORY IS COMMIT-READY!"
    echo "   Repository is now clean and ready for git commit."
    echo ""
    echo "💡 NEXT STEPS:"
    echo "   1. git add <files>"
    echo "   2. git commit -m \"your message\""
    echo "   3. git tag (if releasing)"
    echo "   4. git push origin main"
else
    echo "⚠️  WARNING: Some files may still need manual cleanup"
    exit 1
fi