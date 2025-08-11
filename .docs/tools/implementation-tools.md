# Clean Language Compiler Implementation Tools

We've developed several utility tools to help with implementing and validating the critical fixes for the Clean Language compiler. This document describes these tools and how to use them.

## Standalone Tests

### 1. Memory Management Test
- **File**: `src/bin/memory_test.rs`
- **Purpose**: Validates the memory management system fixes, including allocation, type IDs, and memory release.
- **How to run**: `cargo run --bin memory_test` or `rustc src/bin/memory_test.rs -o memory_test && ./memory_test`
- **Status**: ✅ Tests pass successfully

### 2. Parser Test
- **File**: `parser_fix_test/src/main.rs`
- **Purpose**: Validates the parser Rule enum fix and error handling improvements.
- **How to run**: `cd parser_fix_test && cargo run`
- **Status**: ✅ Tests pass successfully

### 3. Type Conversion Test
- **File**: `type_test.rs`
- **Purpose**: Validates the type conversion helpers between WasmType, ValType, and tuple representations.
- **How to run**: `rustc type_test.rs && ./type_test`
- **Status**: ✅ Tests pass successfully

## Fix Automation Tools

We've created several tools to automate applying the necessary fixes across the codebase:

### 1. Error Method Call Fixer
- **File**: `src/bin/error_fix.rs`
- **Purpose**: Updates error method calls to use the new three-parameter signatures and option-based helper methods.
- **How to run**: `cargo run --bin error_fix -- <filepath>`
- **What it does**:
  - Fixes `runtime_error`, `validation_error`, `parse_error`, and `codegen_error` calls to include all three parameters
  - Updates `with_help` and `with_location` calls to use the option-based methods

### 2. CallIndirect Fixer
- **File**: `src/bin/call_indirect_fix.rs`
- **Purpose**: Updates CallIndirect instructions to use the correct field names (`ty` and `table` instead of `ty_idx` and `table_idx`).
- **How to run**: `cargo run --bin call_indirect_fix`
- **What it does**:
  - Automatically processes list_ops.rs to fix the CallIndirect field names
  - Uses regex pattern matching to identify and replace incorrect field names

### 3. WasmType Usage Fixer
- **File**: `src/bin/wasm_type_fix.rs`
- **Purpose**: Updates direct tuple usage to use the type conversion helpers.
- **How to run**: `cargo run --bin wasm_type_fix -- <filepath>`
- **What it does**:
  - Adds the necessary import for type conversion helpers if needed
  - Replaces direct tuple patterns like `(0, ValType::I32)` with `to_tuple(WasmType::I32)`
  - Makes type usage consistent across the codebase

## Usage Instructions

To complete the remaining implementation fixes, follow these steps:

1. **Fix Error Method Calls**:
   ```sh
   cargo run --bin error_fix -- src/codegen/mod.rs
   cargo run --bin error_fix -- src/validation/mod.rs
   cargo run --bin error_fix -- src/stdlib/mod.rs
   ```

2. **Fix CallIndirect Instructions**:
   ```sh
   cargo run --bin call_indirect_fix
   ```

3. **Fix WasmType Usage**:
   ```sh
   cargo run --bin wasm_type_fix -- src/stdlib/string_ops.rs
   cargo run --bin wasm_type_fix -- src/stdlib/list_ops.rs
   ```

4. **Run the Tests**:
   ```sh
   rustc type_test.rs && ./type_test
   cargo run --bin memory_test
   cd parser_fix_test && cargo run
   ```

## Next Steps

After applying these fixes, the next step is to run a full compilation test to verify that all components work together correctly. This can be done by creating a simple Clean Language program and running it through the complete compilation pipeline.

## Troubleshooting

If you encounter any issues with the fix tools:

1. Ensure the regex patterns in the fix tools match the actual code patterns in your files.
2. For complex files, it may be necessary to manually apply some fixes.
3. If a fix tool makes incorrect changes, you can restore the file from version control and apply the fixes manually.

The automated tools are designed to handle the common cases but may not cover all edge cases, especially in complex code structures. 