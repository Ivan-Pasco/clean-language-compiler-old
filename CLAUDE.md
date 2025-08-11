# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

This is a Rust-based compiler for Clean Language, a type-safe programming language that compiles to WebAssembly. Clean Language combines JavaScript-like readability with Rust-like safety features.

**MAIN LANGUAGE GUIDE**: The definitive Clean Language specification is located at:
```
/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/documentation/Clean_Language_Specification.md
```

Language characteristics are described in the [Clean Language Specification](./documentation/Clean_Language_Specification.md). If you find something that is not described in the specification, propose a change to the specification before implementing it. When something is added you need to update the specification.

this is a full implementation, all the functions should do the complete expected work, no placeholders or fallback implementations.

## Common Commands

### Building and Testing
```bash
# Build the compiler
cargo build
make build

# Run all tests
cargo test
make test

# Run integration tests specifically
cargo test --test integration

# Run simple compilation test
cargo run --bin clean-language-compiler simple-test

# Run comprehensive tests
cargo run --bin clean-language-compiler comprehensive-test
```

### Compilation
```bash
# Compile a Clean Language file
cargo run --bin clean-language-compiler compile -i examples/hello.cln -o output.wasm

# Alternative using cleanc binary
cargo run --bin cleanc -- input.clean output.wasm

# Using Makefile
make run INPUT=examples/hello.cln OUTPUT=examples/hello.wasm
```

### Debugging Tools
```bash
# Debug parsing with AST display
cargo run --bin clean-language-compiler debug -i file.clean --show-ast

# Parse with error recovery
cargo run --bin clean-language-compiler parse -i file.clean --recover-errors

# Lint code
cargo run --bin clean-language-compiler lint -i file.clean

# Debug WebAssembly generation
cargo run --bin debug_wasm -- file.wasm

# Debug parser specifically
cargo run --bin debug_parser -- file.clean
```

### Language Server
```bash
# Build the language server
cd language-server && cargo build --release

# Run language server for VS Code integration
cd language-server && cargo run --release

# Test language server with debug output
RUST_LOG=debug cargo run --bin clean-language-server
```

### Package Management
```bash
# Initialize a new Clean Language package
cargo run --bin clean-language-compiler package init

# Add a dependency
cargo run --bin clean-language-compiler package add package-name

# Install dependencies
cargo run --bin clean-language-compiler package install
```

## Architecture

### Core Components

**Parser** (`src/parser/`): Pest-based parser with error recovery
- Uses `grammar.pest` for grammar rules
- Supports classes, functions, async operations, and inheritance
- Error recovery mode for better debugging

**Semantic Analysis** (`src/semantic/`): Type checking and validation
- Type inference and constraint solving
- Scope management for variables and functions
- Class inheritance validation

**Code Generation** (`src/codegen/`): WebAssembly output
- Generates WASM using `wasm-encoder`
- Memory management and string pooling
- Type-specific instruction generation

**Runtime** (`src/runtime/`): Async and I/O operations
- Async task scheduling
- File I/O operations
- HTTP client functionality

**Standard Library** (`src/stdlib/`): Built-in functions
- List operations, math functions, string manipulation
- Memory management utilities
- Type conversion functions

**Language Server** (`language-server/`): IDE support and LSP implementation
- Real-time syntax analysis and error detection
- Intelligent autocompletion and hover information
- Integration with VS Code, Neovim, and other LSP clients
- Comprehensive IDE features for Clean Language development

### Key Features

**Type System**: Strong static typing with inference
- Primitive types: `integer`, `number`, `string`, `boolean`
- Complex types: `list<T>`, `Matrix<T>`
- Class inheritance with `base()` constructor calls

**WebAssembly Target**: All code compiles to WASM
- Memory-safe execution
- Portable across platforms
- Integration with existing WASM toolchain

**Error Handling**: Comprehensive error recovery
- Parse errors with recovery suggestions
- Semantic analysis errors with context
- Runtime error propagation with `onError` syntax

## Testing Strategy

The project uses multiple testing approaches:

1. **Unit Tests**: Individual component testing (`cargo test`)
2. **Integration Tests**: End-to-end compilation testing (`tests/integration_tests.rs`)
3. **Parser Tests**: Grammar and parsing validation (`tests/parser_tests/`)
4. **Standard Library Tests**: Built-in function verification (`tests/stdlib_tests.rs`)

### **MANDATORY Test File Locations** ⚠️

**CRITICAL REQUIREMENT**: ALL Clean Language test files MUST follow this exact directory structure:

```
/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/
├── tests/
│   ├── clean_files/          # ✅ ALL .cln test files go here ONLY
│   │   ├── 00_minimal.cln
│   │   ├── 01_hello_world.cln
│   │   └── ... (73+ existing test files)
│   └── wasm/                 # ✅ ALL compiled .wasm files go here ONLY
│       ├── 00_minimal.wasm
│       ├── 01_hello_world.wasm
│       └── ... (compiled outputs)
```

**ABSOLUTE RULES - NO EXCEPTIONS:**

1. **Source Files**: ALL `.cln` test files MUST be created in: `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/clean_files/`
2. **Compiled Files**: ALL `.wasm` output files MUST be compiled to: `/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/tests/wasm/`
3. **Naming Convention**: Use `XX_descriptive_name.cln/.wasm` format (XX = two-digit number)
4. **Consistency**: This maintains consistency with existing 73+ test files and build system integration

**Standard Compilation Command:**
```bash
cargo run --bin clean-language-compiler compile -i "tests/clean_files/filename.cln" -o "tests/wasm/filename.wasm"
```

### Test Development Guidelines

- You don't need permission to add tests - just add them, compile them, run them
- When tests fail, add failed results to the `TASKS.md` file
- Check test syntax against the Clean Language Specification (`/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/documentation/Clean_Language_Specification.md`) before fixing implementation
- Always verify tests compile to `tests/wasm/` directory before considering them complete

## Development Workflow

When implementing new features:

1. Update grammar in `src/parser/grammar.pest` if needed
2. Extend AST definitions in `src/ast/mod.rs`
3. Add parsing logic in appropriate `src/parser/` files
4. Implement semantic analysis in `src/semantic/`
5. Add code generation in `src/codegen/`
6. Write comprehensive tests
7. Update examples and documentation
8. Update the language specification in `documentation/Clean_Language_Specification.md`
9. when you find an error while compiling, add it to the `TASKS.md` file.
10. when a test fails, check if the test is correct and has the right syntax according to the Clean Language Specification (`documentation/Clean_Language_Specification.md`), if it is wrong, fix the test. once the test is fixed if it still fails add the needed changes to the TASKS.md file.

## File Extensions

- `.cln`: Clean Language source files
- `.wasm`: Compiled WebAssembly output
- `package.clean.toml`: Package manifest files

## Development Rules and Guidelines

### Code Quality Standards

1. **NO PLACEHOLDER IMPLEMENTATIONS**: Never create placeholder functions that return dummy values (like `return 0`, `return false`, etc.). All functions must be fully implemented and functional.

2. **NO FALLBACK IMPLEMENTATIONS**: Avoid temporary "simplified" implementations that don't provide the actual functionality. Each function should work as intended.

3. **WORKING CODE ONLY**: All code must be production-ready and functional. If a feature cannot be fully implemented immediately, document it as a task rather than implementing a stub.

### Task Management

1. **TASKS.md TRACKING**: When discovering any error, bug, or incomplete implementation:
   - Add it as a new task in TASKS.md with appropriate priority level
   - Include specific file paths and line numbers
   - Describe the exact issue and expected behavior

2. **TASK COMPLETION UPDATES**: When completing any task:
   - Mark the task as completed in TASKS.md
   - Update the status and add completion notes
   - Include any relevant technical details about the solution

3. **TASK PRIORITIZATION**: Follow the existing priority system in TASKS.md:
   - 🔴 CRITICAL: Core functionality issues that break the compiler
   - 🟡 MEDIUM-HIGH: Important features with significant impact
   - 🟢 LOW: Nice-to-have improvements and optimizations

### Error Handling Philosophy

- Fix the root cause, not the symptoms
- Maintain functional implementations while fixing underlying issues
- Document complex issues that require multi-step solutions
- Always prefer proper fixes over workarounds