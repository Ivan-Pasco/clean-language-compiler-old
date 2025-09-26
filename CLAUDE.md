# CLAUDE.md

This file provides guidance when working with code in this repository.

## Mandatory Requirements

- **NEVER mention Claude Code in any documents, git commits, or any part of the code**
- **NEVER reference Claude Code in documentation or git commits**  
- **NEVER include Claude Code attribution in any files, git commits, or documentation**
- **NEVER mention Claude Code in any context within this codebase**

## Overview

This is a Rust-based compiler for Clean Language, a type-safe programming language that compiles to WebAssembly. Clean Language combines JavaScript-like readability with Rust-like safety features.

Language characteristics are described in the [Language Specification](./Language-Specification.md). If you find something that is not described in the specification, propose a change to the specification before implementing it. When something is added you need to update the specification.

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

## 🧪 **UNIFIED TESTING STRATEGY**

### **Automatic Testing Command**
When the user types **"test"**, automatically execute the unified testing strategy defined in `tests/UNIFIED_TESTING_STRATEGY.md`. This proven methodology combines:

1. **Existing Test Priority**: Always search existing tests in `tests/cln/` FIRST before creating new ones (353+ organized files)
2. **Proven QA Methodology**: Phase-based approach with **proven effectiveness in achieving 100% success rates**
3. **Intelligent Problem Resolution**: Automatic agent selection based on proven error classification:
   - **QA Agent**: For systematic failures (5+ similar errors), regression analysis, comprehensive quality assurance
   - **Debug Agent**: For specific compilation errors, parser issues, semantic analysis problems
   - **Context7 MCP**: For Rust compiler internals, WebAssembly generation, best practices
   - **Internet Search**: For complex problems requiring external research and similar compiler solutions
   - **Deep Thinking**: For particularly challenging architectural issues with systematic analysis

### **Proven Error Classification System**
- **🔴 CRITICAL**: Blocks all tests (compiler compilation failures, core AST issues) → Debug Agent + Context7 MCP
- **🟡 HIGH**: Affects 20-50 tests (missing language features, undefined variables) → Debug Agent + QA Agent
- **🟢 MEDIUM**: Affects 1-10 tests (syntax edge cases, advanced features) → Debug Agent

### **Testing Quality Standards**
- **100% Compilation Rate REQUIRED**: All .cln files must compile successfully
- **100% Execution Rate REQUIRED**: All compiled programs must execute without errors
- **NO Placeholders**: Production-grade code only, no `todo!()` or dummy implementations
- **Specification Compliance**: All tests must align with Clean Language Specification
- **Proven Fix Patterns**: Use established templates for AST, semantic, and codegen fixes

### **Problem Resolution Escalation (Proven Framework)**
1. **Level 1**: Use specialized agents based on error classification and impact analysis
2. **Level 2**: Add Context7 MCP for Rust/WebAssembly technical expertise
3. **Level 3**: Internet search for similar compiler problems and implementation patterns
4. **Level 4**: Deep thinking with architectural consideration and proven debugging procedures

### **Testing Infrastructure**
- **Test Files**: All located in `tests/cln/` organized by category (353+ files)
- **Test Outputs**: All compiled .wasm files go to `tests/output/` directory
- **Quality Assurance**: Proven QA procedures in `tests/qa/` with automation scripts
- **Documentation**: Complete unified methodology in `tests/UNIFIED_TESTING_STRATEGY.md`

**📋 TRIGGER**: When user writes "test", automatically execute the unified testing strategy combining proven QA methodology with intelligent agent integration for systematic problem resolution.**

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
- Array operations, math functions, string manipulation
- Memory management utilities
- Type conversion functions

### Key Features

**Type System**: Strong static typing with inference
- Primitive types: `integer`, `number`, `string`, `boolean`
- Complex types: `Array<T>`, `Matrix<T>`
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

## Development Workflow

When implementing new features:

1. Update grammar in `src/parser/grammar.pest` if needed
2. Extend AST definitions in `src/ast/mod.rs`
3. Add parsing logic in appropriate `src/parser/` files
4. Implement semantic analysis in `src/semantic/`
5. Add code generation in `src/codegen/`
6. Write comprehensive tests
7. Update examples and documentation
8. Update the language specification in `Language-Specification.md`
9. When you find an error while compiling, add it to the `TASKS.md` file.
10. When a test fails, check if the test is correct and has the right syntax according to the Language-Specification.md, if it is wrong, fix the test. Once the test is fixed if it still fails add the needed changes to the TASKS.md file.

## File Extensions

- `.cln`: Clean Language source files
- `.wasm`: Compiled WebAssembly output
- `package.clean.toml`: Package manifest files

## Development Rules and Guidelines

### Code Quality Standards

1. **NO PLACEHOLDER IMPLEMENTATIONS**: Never create placeholder functions that return dummy values (like `return 0`, `return false`, etc.). All functions must be fully implemented and functional.

2. **NO FALLBACK IMPLEMENTATIONS**: Avoid temporary "simplified" implementations that don't provide the actual functionality. Each function should work as intended.

3. **WORKING CODE ONLY**: All code must be production-ready and functional. If a feature cannot be fully implemented immediately, document it as a task rather than implementing a stub.

### Version Management

**MANDATORY SEMANTIC VERSIONING RULE**: All Clean Language compiler versions MUST follow strict semantic versioning (semver) format: `MAJOR.MINOR.PATCH`

1. **Version Format**: `X.Y.Z` (e.g., `0.7.0`, `1.2.3`)
   - **MAJOR**: Breaking changes, incompatible API changes
   - **MINOR**: New features, backward compatible
   - **PATCH**: Bug fixes, backward compatible

2. **Forbidden Version Formats**:
   - ❌ `0.7.0-production-wasm` (descriptive suffixes)
   - ❌ `v0.7.0` (version prefixes)
   - ❌ `0.7` (incomplete versioning)
   - ❌ Any non-numeric suffixes or prefixes

3. **Version Update Process**:
   - Update `Cargo.toml` version field
   - Ensure all references use semantic versioning
   - Tag releases with exact version number (e.g., `0.7.0`)
   - Update version in help text and CLI output

4. **Release Naming**: Use only the semantic version number for:
   - Git tags: `0.7.0`
   - Binary releases: `cln-0.7.0`
   - Documentation references: `Clean Language 0.7.0`

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

## Important Instructions

Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
- we dont need backwards compatibility, the Clean Language Specification and the intermediate representations are the main and only source of truth