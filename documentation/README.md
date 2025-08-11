# Clean Language Compiler Documentation

This directory contains comprehensive documentation for the Clean Language compiler project.

> 📖 **Cross-Reference**: See [Clean Language Specification](../docs/language/Clean_Language_Specification.md) for the complete language definition and [CLAUDE.md](../CLAUDE.md) for development commands.

## Quick Start

If you're new to this project, start here:

1. **[Project Overview](./project-overview.md)** - High-level architecture and design principles
2. **[Development Guide](./development-guide.md)** - Best practices and workflows for development
3. **[WebAssembly Documentation](./webassembly.md)** - Code generation and WASM integration
4. **[Parser Documentation](./parser.md)** - Parsing system and grammar details
5. **[Semantic Analysis](./semantic-analysis.md)** - Type checking and semantic validation
6. **[Standard Library](./standard-library.md)** - Built-in functions and runtime system

## Documentation Structure

### 📋 **[Project Overview](./project-overview.md)**
- System architecture and compilation pipeline
- Key design principles and language features
- Performance characteristics and integration points
- Extension mechanisms and development workflows

**When to read**: First time working with the project, need high-level understanding

### 🛠️ **[Development Guide](./development-guide.md)**
- Step-by-step workflows for common development tasks
- Best practices and coding patterns
- Debugging techniques and testing strategies
- Security considerations and performance optimization

**When to read**: Before implementing new features, fixing bugs, or making changes

### ⚙️ **[WebAssembly Documentation](./webassembly.md)**
- Code generation architecture and instruction generation
- Memory management system and type mapping
- Integration with runtime and host environment
- Optimization strategies and debugging techniques

**When to read**: Working on code generation, memory management, or WASM output

### 📝 **[Parser Documentation](./parser.md)**
- Grammar definition and parsing architecture
- AST generation and error recovery systems
- Expression and statement parsing details
- Integration with semantic analysis

**When to read**: Modifying syntax, fixing parse errors, or extending grammar

### 🔍 **[Semantic Analysis](./semantic-analysis.md)**
- Type system implementation and checking
- Scope management and symbol resolution
- Function overloading and class inheritance
- Error handling and diagnostic systems

**When to read**: Working on type checking, semantic validation, or error reporting

### 📚 **[Standard Library](./standard-library.md)**
- Built-in function implementation and organization
- Memory management and WebAssembly integration
- Host import patterns and runtime considerations
- Extension patterns for new functionality

**When to read**: Adding standard library functions, debugging runtime issues

## Key Resources

### Language Specification
The authoritative language specification is located at:
- **Primary**: `/docs/language/Clean_Language_Specification.md`
- **CLAUDE.md**: Root directory - Contains compiler instructions and development rules

### Critical Project Files
- **Grammar**: `src/parser/grammar.pest` - Defines Clean Language syntax
- **Main Library**: `src/lib.rs` - Programmatic compiler API
- **AST Definitions**: `src/ast/mod.rs` - Abstract syntax tree structures
- **Tasks**: `TASKS.md` - Current issues and development priorities

### Testing and Examples
- **Test Suite**: `tests/` - Comprehensive test coverage
- **Examples**: `examples/` - Sample Clean Language programs
- **Demo Programs**: `demo_examples/` - Curated example programs

## Quick Reference

### Common Development Tasks

| Task | Primary Documentation | Key Files |
|------|----------------------|-----------|
| Add new syntax | [Parser](./parser.md) + [Development Guide](./development-guide.md) | `grammar.pest`, parser modules |
| Fix type errors | [Semantic Analysis](./semantic-analysis.md) | `src/semantic/` modules |
| Add stdlib function | [Standard Library](./standard-library.md) | `src/stdlib/` modules |
| Debug WASM output | [WebAssembly](./webassembly.md) | `src/codegen/` modules |
| Optimize performance | [Development Guide](./development-guide.md) | All modules |

### Debugging Quick Start

```bash
# Parse and show AST
cargo run --bin clean-language-compiler debug -i file.cln --show-ast

# Compile with detailed output
RUST_LOG=debug cargo run --bin clean-language-compiler compile -i file.cln -o output.wasm

# Examine generated WebAssembly
cargo run --bin wasm2wat output.wasm > output.wat
```

### Testing Quick Start

```bash
# Run all tests
cargo test

# Run specific test category
cargo test --test integration_tests
cargo test --test parser_tests

# Run with detailed output
cargo test -- --nocapture
```

## Architecture Summary

```
Clean Language Source (.cln)
           ↓
    Parser (Pest-based)
           ↓
    Abstract Syntax Tree
           ↓
   Semantic Analysis
           ↓
    Code Generation
           ↓
   WebAssembly (.wasm)
```

**Key Components:**
- **Parser**: Converts source code to AST using Pest grammar
- **Semantic Analyzer**: Validates types, scopes, and language semantics
- **Code Generator**: Produces WebAssembly bytecode with memory management
- **Standard Library**: Provides built-in functions and runtime support
- **Runtime**: Handles async operations, I/O, and host integration

## Development Principles

When working on this project, always keep these principles in mind:

1. **Type Safety First** - Clean Language prioritizes compile-time safety
2. **WebAssembly Native** - All features must compile efficiently to WASM
3. **Developer Experience** - Provide excellent error messages and debugging
4. **Memory Safety** - Use reference counting and bounds checking
5. **Specification Compliance** - Follow the language specification exactly

## Getting Help

If you encounter issues or need clarification:

1. **Check the relevant documentation** in this directory
2. **Review the Language Specification** for authoritative language rules
3. **Examine existing code patterns** in the codebase
4. **Look at test cases** for usage examples
5. **Check TASKS.md** for known issues and priorities

## Contributing

When making changes:

1. **Read the Development Guide** for workflows and best practices
2. **Follow existing code patterns** and architectural decisions
3. **Add comprehensive tests** for all new functionality
4. **Update documentation** when changing behavior
5. **Maintain WebAssembly compatibility** for all language features

## Documentation Maintenance

This documentation is designed to be:
- **Accurate**: Reflects the current state of the codebase
- **Complete**: Covers all major systems and components
- **Practical**: Focuses on actionable information for development
- **Up-to-date**: Maintained alongside code changes

If you find inaccuracies or need additional information, the documentation should be updated to reflect the current project state.

---

**Happy coding with Clean Language!** 🚀

This documentation provides everything needed to understand and work effectively with the Clean Language compiler. Start with the Project Overview for a high-level understanding, then dive into specific areas as needed for your development tasks.