# CLAUDE.md

Development guidance for the Clean Language compiler.

## Mandatory Requirements

- **NEVER mention Claude Code in any documents, git commits, or any part of the code**

## Project Goal

100% compilation and execution success for all test files in `tests/cln/`. No placeholders, no workarounds, no hidden failures. Fix root causes, not symptoms. Never modify tests to pass — fix the compiler instead.

## MCP Server — Use It First

**CRITICAL: Before writing ANY Clean Language code, call `get_quick_reference` from the clean-language MCP server.** This gives you the correct, up-to-date syntax. Do NOT write Clean code from memory — always verify against the MCP tools.

Available MCP tools: `get_quick_reference`, `check`, `compile`, `parse`, `get_specification`, `list_functions`, `list_types`, `list_builtins`, `list_plugins`, `get_architecture`, `explain_error`

The MCP server is configured in `.mcp.json` at the project root.

## Language Specification

The **single source of truth** for the Clean Language is:
- **[Language Specification](./documentation/Clean_Language_Specification.md)** — prose specification
- **[`foundation/spec/grammar.ebnf`](../foundation/spec/grammar.ebnf)** — formal EBNF grammar (authoritative for syntax)
- **[`foundation/spec/semantic-rules.md`](../foundation/spec/semantic-rules.md)** — numbered semantic rules
- **[`foundation/spec/type-system.md`](../foundation/spec/type-system.md)** — type hierarchy and compatibility
- **[`foundation/spec/stdlib-reference.md`](../foundation/spec/stdlib-reference.md)** — built-in function signatures
- **[`foundation/spec/plugins/`](../foundation/spec/plugins/)** — plugin grammar extensions (EBNF)

When resolving syntax ambiguity, `foundation/spec/grammar.ebnf` takes precedence. If something is not in the spec, propose a spec change before implementing it. When something is added, update both the prose spec and the formal EBNF.

See also: **[KNOWLEDGE.md](./KNOWLEDGE.md)** — known fragile areas in compiler code.

## Platform Architecture

The compiler generates WASM imports — it does NOT implement runtime functions.

- **[Execution Layers](../foundation/platform-architecture/EXECUTION_LAYERS.md)** — read FIRST before implementing any function
- **[Host Bridge](../foundation/platform-architecture/HOST_BRIDGE.md)** — Layer 2 portable functions
- **[Memory Model](../foundation/platform-architecture/MEMORY_MODEL.md)** — WASM memory layout
- **[Server Extensions](../foundation/platform-architecture/SERVER_EXTENSIONS.md)** — Layer 3 HTTP functions
- **[Architecture Boundaries](../foundation/management/ARCHITECTURE_BOUNDARIES.md)** — component responsibilities

**Rule of thumb:** If a function needs I/O, network, or database, it belongs in a plugin declaration, NOT in the compiler registry.

### Compiler Registry (`src/builtins/registry.rs`) — What Belongs Here

- Language built-ins: print, printl, input
- Math operations: abs, sqrt, pow, sin, cos
- Type conversions: toString, toInteger
- Namespaces: Math.*, String.*, Integer.*
- Pure WASM operations: json.*

Everything else comes from plugin `[bridge]` declarations in `plugin.toml`.

### WAT Spec Compliance

The compiler must generate WASM imports matching host bridge signatures exactly:
- String parameters use `(ptr: i32, len: i32)` pairs
- Integer values use `i64` for: `print_integer`, `int_to_string`, `string_to_int`
- Reference: `clean-server/host-bridge/tests/spec_compliance.wat`

## Common Commands

```bash
# Build
cargo build

# Test
cargo test
cargo test --test integration

# Compile a file
cargo run --bin clean-language-compiler compile -i file.cln -o output.wasm

# Debug
cargo run --bin clean-language-compiler debug -i file.cln --show-ast
cargo run --bin debug_wasm -- file.wasm
```

## Automated Workflows

- **"comita"** → CI/CD workflow. See [scripts/CI_CD_WORKFLOW.md](./scripts/CI_CD_WORKFLOW.md)
- **"test"** → Unified testing strategy. See [tests/UNIFIED_TESTING_STRATEGY.md](./tests/UNIFIED_TESTING_STRATEGY.md)

## Architecture Overview

| Component | Location | Purpose |
|-----------|----------|---------|
| Parser | `src/parser/` | Pest-based parser with error recovery |
| HIR | `src/hir/` | High-level intermediate representation |
| Resolver | `src/resolver/` | Name resolution and symbol management |
| Type Checker | `src/typechecker/` | Type inference and checking |
| MIR | `src/mir/` | Mid-level IR, optimization target |
| Code Generation | `src/codegen/` | WASM output via `wasm-encoder` |
| Builtins | `src/builtins/` | Built-in function registry |
| Plugins | `src/plugins/` | Plugin system (plugin.toml parsing) |

Full documentation: [documentation/](./documentation/)

## Development Workflow

1. Update grammar in `src/parser/grammar.pest` if needed
2. Extend AST in `src/ast/mod.rs`
3. Add parsing logic in `src/parser/`
4. Implement semantic analysis in `src/semantic/`
5. Add code generation in `src/codegen/`
6. Write tests, update documentation and spec
7. Errors found → add to `TASKS.md`

## Code Quality Rules

- No placeholder implementations — all code must be functional
- No `todo!()` macros in production code
- Strict semantic versioning: `MAJOR.MINOR.PATCH` only
- Track bugs and tasks in `TASKS.md`
- Fix root causes, not symptoms
- No backwards compatibility hacks needed — the spec is the source of truth

## Testing

- All test files: `tests/cln/` (organized by category)
- All compiled output: `tests/output/`
- Future/unimplemented features: `tests/cln/future/`
- Full methodology: [tests/UNIFIED_TESTING_STRATEGY.md](./tests/UNIFIED_TESTING_STRATEGY.md)

## MCP Ecosystem Catalog

When plugins change in the ecosystem, update `src/mcp/server.rs` → `get_ecosystem_catalog()`.

## Cross-Component Work Policy

Do NOT directly edit code in other components. You CAN read other components to understand interfaces. You MUST NOT modify their code.

**When you discover something in another component, choose ONE channel:**

| What you found | Channel | Why |
|---|---|---|
| A **bug** (crash, wrong output, spec violation, regression) | **`report_error` MCP tool** — MANDATORY | Fingerprint dedup, occurrence tracking, automatic user notification on fix, visible on the ecosystem dashboard at errors.cleanlanguage.dev. This is the canonical bug-tracking path per `../CLAUDE.md` § "Cross-Component Bug Reporting". |
| A **design proposal, directive change, schema/API request, architectural ask** | Markdown file in `../foundation/management/cross-component-prompts/` | Requires discussion, not auto-fix. Not a bug. |

**Never** write a markdown prompt for something that is a bug. Bug reports that sit in a markdown file are invisible to the dashboard dedup, don't notify users when fixed, and can't be pulled via `list_component_bugs`.

If in doubt: it's a bug → `report_error`.

## Documentation Sync Protocol

Facts about the language live in `foundation/spec/` (at the project root). Facts about the platform live in `foundation/platform-architecture/`. Do not duplicate them here — link to them instead.

**When you make a change in this component, update the corresponding spec file in the same commit:**

| Change type | Update required |
|-------------|-----------------|
| New language syntax | `foundation/spec/grammar.ebnf` |
| New semantic rule or error code | `foundation/spec/semantic-rules.md` + `foundation/spec/error-codes.md` |
| New or changed type rule | `foundation/spec/type-system.md` |
| New or changed built-in function | `foundation/spec/stdlib-reference.md` |
| New or changed AST node | `foundation/spec/ast.md` |
| New or changed plugin contract | `foundation/spec/plugins/plugin-contract.md` |
| New or changed host bridge function | `foundation/platform-architecture/HOST_BRIDGE.md` |
| New or changed execution layer | `foundation/platform-architecture/EXECUTION_LAYERS.md` |

The spec files are the single source of truth. Component documentation explains implementation — it does not redefine language rules.
