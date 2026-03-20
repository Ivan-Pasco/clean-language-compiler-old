# Clean Language Compiler Documentation

## Authoritative References

| Document | Purpose |
|----------|---------|
| [Clean Language Specification](./Clean_Language_Specification.md) | **Single source of truth** for the language: syntax, types, semantics |
| [Execution Layers](../../platform-architecture/EXECUTION_LAYERS.md) | What each layer does — read before implementing any function |
| [Architecture Boundaries](../../system-documents/ARCHITECTURE_BOUNDARIES.md) | What each component is and is not responsible for |

## Compiler Documentation

### Getting Started

| Document | When to Read |
|----------|-------------|
| [Project Overview](./project-overview.md) | First time working with the project |
| [Development Guide](./development-guide.md) | Before implementing features or fixing bugs |

### Compiler Pipeline

| Document | Covers |
|----------|--------|
| [Parser](./parser.md) | Pest grammar, AST generation, error recovery |
| [AST Reference](./ast-reference.md) | AST node types and structure |
| [Semantic Analysis](./semantic-analysis.md) | Type checking, scope management, validation |
| [Compilation Pipeline](./compilation-pipeline.md) | HIR → MIR → WASM transformation pipeline |
| [WebAssembly](./webassembly.md) | WASM code generation, memory management |

### Language Features

| Document | Covers |
|----------|--------|
| [Standard Library](./standard-library.md) | Built-in functions, namespaces, host imports |
| [Error Handling Guide](./error-handling-guide.md) | Error recovery system, onError syntax |
| [Memory Management](./memory-management.md) | WASM memory layout, bump allocator, GC |
| [Pattern System](./clean_pattern_system_specification.md) | Pattern matching specification |
| [Plugin Architecture](./Plugin-Architecture.md) | Plugin system, plugin.toml, bridge functions |

### Tooling

| Document | Covers |
|----------|--------|
| [Language Server](./language-server.md) | LSP implementation, IDE integration |
| [Multi-Runtime Targets](./multi-runtime-targets.md) | Wasmtime/Wasmer selection, target optimization |
| [Testing Strategy](./testing-strategy.md) | Test organization, testing approaches |

## Platform Architecture (Shared)

These documents live in `platform-architecture/` and are shared across all components:

| Document | Covers |
|----------|--------|
| [Host Bridge](../../platform-architecture/HOST_BRIDGE.md) | Layer 2: portable host functions |
| [Memory Model](../../platform-architecture/MEMORY_MODEL.md) | WASM memory layout, string format |
| [Server Extensions](../../platform-architecture/SERVER_EXTENSIONS.md) | Layer 3: HTTP server functions |
| [Implementing a New Host](../../platform-architecture/IMPLEMENTING_HOST.md) | Guide for new runtime implementations |

## Quick Reference

```bash
# Build
cargo build

# Test
cargo test

# Compile a file
cargo run --bin clean-language-compiler compile -i file.cln -o output.wasm

# Debug AST
cargo run --bin clean-language-compiler debug -i file.cln --show-ast

# Debug WASM
cargo run --bin debug_wasm -- file.wasm
```

## Key Source Files

| File | Purpose |
|------|---------|
| `src/parser/grammar.pest` | Clean Language grammar definition |
| `src/ast/mod.rs` | AST node definitions |
| `src/hir/mod.rs` | High-level IR types |
| `src/mir/mir_types.rs` | Mid-level IR types |
| `src/codegen/mod.rs` | WASM code generator |
| `src/builtins/registry.rs` | Built-in function registry |
| `TASKS.md` | Current issues and priorities |
