# Clean Language Compiler Documentation

## Authoritative References

| Document | Purpose |
|----------|---------|
| [Clean Language Specification](../../Clean_Language_Specification.md) | **Human-friendly** language spec (syntax, types, semantics, examples) |
| [Formal Grammar](../../foundation/spec/grammar.ebnf) | **Authoritative** EBNF grammar — takes precedence over prose |
| [Semantic Rules](../../foundation/spec/semantic-rules.md) | Numbered semantic rules (SYN, SEM, SCOPE, FUNC, CLASS) |
| [Type System](../../foundation/spec/type-system.md) | Type hierarchy, compatibility matrix, conversions |
| [Execution Layers](../../foundation/platform-architecture/EXECUTION_LAYERS.md) | What each layer does — read before implementing any function |
| [Architecture Boundaries](../../foundation/management/ARCHITECTURE_BOUNDARIES.md) | What each component is and is not responsible for |

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
| [AST Reference](../../foundation/spec/ast.md) | AST node types and structure (formal spec) |
| [AST Implementation Notes](./ast-reference.md) | Compiler-internal AST implementation details |
| [Compilation Pipeline](./compilation-pipeline.md) | HIR → MIR → WASM transformation pipeline |
| [WebAssembly](./webassembly.md) | WASM code generation, memory management |

### Language Features

| Document | Covers |
|----------|--------|
| [Error Handling Guide](./error-handling-guide.md) | Error recovery system, onError syntax |
| [Memory Management](./memory-management.md) | WASM memory layout, bump allocator, GC |
| [Plugin Architecture](./Plugin-Architecture.md) | Plugin system, plugin.toml, bridge functions |

### Tooling

| Document | Covers |
|----------|--------|
| [Language Server](./language-server.md) | LSP implementation, IDE integration |
| [Multi-Runtime Targets](./multi-runtime-targets.md) | Wasmtime/Wasmer selection, target optimization |
| [Testing Strategy](./testing-strategy.md) | Test organization, testing approaches |

## Platform Architecture (Shared)

These documents live in `foundation/platform-architecture/` and are shared across all components:

| Document | Covers |
|----------|--------|
| [Host Bridge](../../foundation/platform-architecture/HOST_BRIDGE.md) | Layer 2: portable host functions |
| [Memory Model](../../foundation/platform-architecture/MEMORY_MODEL.md) | WASM memory layout, string format |
| [Server Extensions](../../foundation/platform-architecture/SERVER_EXTENSIONS.md) | Layer 3: HTTP server functions |
| [Implementing a New Host](../../foundation/platform-architecture/IMPLEMENTING_HOST.md) | Guide for new runtime implementations |

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
