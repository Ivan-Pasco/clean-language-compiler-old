#!/bin/bash

# Batch fix for common missing return type issues
# This script will fix files that have functions without return types

echo "🔧 Batch fixing missing return type issues..."

# Array of files that likely need return type fixes
files_to_fix=(
    "test_inheritance_polymorphism.cln"
    "test_method_override.cln" 
    "test_polymorphism_param.cln"
    "test_simple_polymorphism.cln"
    "debug_simple_static.cln"
    "debug_memory.cln"
    "debug_listlength_conflict.cln"
)

fixed_count=0
for file in "${files_to_fix[@]}"; do
    if [ -f "tests/clean_files/$file" ]; then
        echo "Checking: $file"
        
        # Check if file compiles, if not try to fix it
        if ! cargo run --release --bin clean-language-compiler compile -i "tests/clean_files/$file" -o "/tmp/test_$file.wasm" >/dev/null 2>&1; then
            echo "  ❌ Failed - attempting to fix missing return types"
            
            # Create backup
            cp "tests/clean_files/$file" "tests/clean_files/$file.backup"
            
            # Fix common patterns - functions without return types
            sed -i '' 's/^[[:space:]]*\([a-zA-Z_][a-zA-Z0-9_]*\)()[[:space:]]*$/	void \1()/g' "tests/clean_files/$file"
            sed -i '' 's/^functions:[[:space:]]*$/functions:/g' "tests/clean_files/$file"
            sed -i '' 's/^[[:space:]]*functions:[[:space:]]*$/functions:/g' "tests/clean_files/$file"
            
            # Test if fix worked
            if cargo run --release --bin clean-language-compiler compile -i "tests/clean_files/$file" -o "/tmp/test_$file.wasm" >/dev/null 2>&1; then
                echo "  ✅ Fixed successfully"
                ((fixed_count++))
                rm "tests/clean_files/$file.backup"
            else
                echo "  ⚠️ Fix didn't work, restoring backup"
                mv "tests/clean_files/$file.backup" "tests/clean_files/$file"
            fi
        else
            echo "  ✅ Already compiles"
        fi
    fi
done

echo ""
echo "🎉 Batch fix completed: $fixed_count files fixed"