# Clean Language Compiler - QA Infrastructure Setup

## CLAUDE CODE INITIALIZATION PROMPT

```
You are about to configure a comprehensive QA and testing infrastructure for the Clean Language Compiler project. This is a Rust-based compiler that targets WebAssembly, consisting of 7 layers and a plugin system.

BEFORE making any changes, you MUST:

1. **DISCOVERY PHASE** (Do not skip)
   - Read the entire configuration document below
   - Explore the current project structure: `find . -type f -name "*.rs" | head -100`
   - Identify existing test files: `find . -type f -name "*test*.rs"`
   - Check current Cargo.toml for existing test dependencies
   - Identify the 7 compiler layers and their actual module names
   - Locate the plugin infrastructure code
   - Find any existing CLAUDE.md or documentation

2. **VALIDATION PHASE** (Do not skip)
   - Compare the configuration below with the actual project structure
   - Identify discrepancies between assumed and actual:
     - Layer names and locations
     - Module structure
     - Existing test coverage
     - Current dependencies
   - Document findings in a `QA_SETUP_ANALYSIS.md` file

3. **IMPROVEMENT PHASE** (Do not skip)
   - Propose improvements to the configuration based on:
     - Actual project structure discovered
     - Existing patterns in the codebase
     - Rust/compiler testing best practices
     - Any project-specific conventions found
   - Update the configuration to match reality
   - Add any missing components discovered

4. **IMPLEMENTATION PHASE** (Only after phases 1-3)
   - Create directory structure
   - Create all agent configuration files
   - Create workflow configurations
   - Update Cargo.toml with test dependencies
   - Create initial test scaffolding
   - Set up scripts and automation

5. **VERIFICATION PHASE** (Do not skip)
   - Run `cargo check` to verify no compilation errors
   - Run `cargo test` to verify existing tests still pass
   - Verify all created files are valid
   - Generate a summary report

IMPORTANT RULES:
- Do NOT blindly copy the configuration - adapt it to the actual project
- Do NOT break existing functionality
- Do NOT remove existing tests or configurations
- Do NOT proceed to implementation until validation is complete
- ALWAYS create backups before modifying existing files
- ALWAYS run verification after each major change

Begin by exploring the project structure and report your findings.
```

---

# CONFIGURATION DOCUMENT

## Table of Contents
1. [Project Context](#1-project-context)
2. [Agent Configurations](#2-agent-configurations)
3. [Directory Structure](#3-directory-structure)
4. [Workflow Configurations](#4-workflow-configurations)
5. [Testing Dependencies](#5-testing-dependencies)
6. [Scripts and Automation](#6-scripts-and-automation)
7. [Implementation Checklist](#7-implementation-checklist)

---

## 1. Project Context

### 1.1 Project Overview

**Project Name:** Clean Language Compiler
**Language:** Rust
**Target:** WebAssembly (WASM)
**Development Duration:** 7+ months
**Current Status:** Active development with ongoing bug fixes

### 1.2 Architecture Summary

```
┌─────────────────────────────────────────────────────────────────┐
│                    CLEAN LANGUAGE COMPILER                       │
├─────────────────────────────────────────────────────────────────┤
│  Source Code (.clean)                                           │
│       │                                                         │
│       ▼                                                         │
│  ┌─────────────┐                                                │
│  │  Layer 1    │  Lexer/Tokenizer                               │
│  │  LEXER      │  → Tokens                                      │
│  └──────┬──────┘                                                │
│         ▼                                                         │
│  ┌─────────────┐                                                │
│  │  Layer 2    │  Parser                                        │
│  │  PARSER     │  → Abstract Syntax Tree (AST)                  │
│  └──────┬──────┘                                                │
│         ▼                                                         │
│  ┌─────────────┐                                                │
│  │  Layer 3    │  Semantic Analysis                             │
│  │  SEMANTIC   │  → Annotated AST                               │
│  └──────┬──────┘                                                │
│         ▼                                                         │
│  ┌─────────────┐                                                │
│  │  Layer 4    │  Type Checking                                 │
│  │  TYPECHECK  │  → Typed AST                                   │
│  └──────┬──────┘                                                │
│         ▼                                                         │
│  ┌─────────────┐                                                │
│  │  Layer 5    │  IR Generation                                 │
│  │  IR_GEN     │  → Intermediate Representation                 │
│  └──────┬──────┘                                                │
│         ▼                                                         │
│  ┌─────────────┐                                                │
│  │  Layer 6    │  Optimization                                  │
│  │  OPTIMIZER  │  → Optimized IR                                │
│  └──────┬──────┘                                                │
│         ▼                                                         │
│  ┌─────────────┐                                                │
│  │  Layer 7    │  WASM Code Generation                          │
│  │  WASM_GEN   │  → WebAssembly Binary (.wasm)                  │
│  └─────────────┘                                                │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │  PLUGIN INFRASTRUCTURE                                      ││
│  │  → Hooks at each layer                                      ││
│  │  → Plugin lifecycle management                              ││
│  │  → Extension API                                            ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

### 1.3 Current Challenges

- Persistent bugs discovered during testing
- Manual acceptance of Claude Code tasks required daily
- Need for autonomous, long-running test and fix cycles
- Goal: 100% compilation and execution success
- Need: Production-grade, high-quality code assurance

---

## 2. Agent Configurations

### 2.1 Main Project CLAUDE.md

**File:** `CLAUDE.md` (project root)

```markdown
# Clean Language Compiler - Project Context

## Project Overview

Clean Language is a programming language that compiles to WebAssembly. This 
compiler is written in Rust and consists of 7 distinct layers plus a plugin 
system for extensibility.

## Quick Reference

### Build Commands
```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo check                    # Quick syntax/type check
cargo clippy                   # Linting
cargo fmt                      # Format code
```

### Test Commands
```bash
cargo test                     # Run all tests
cargo test --lib               # Library tests only
cargo test --doc               # Documentation tests
cargo test <name>              # Run specific test
cargo test -- --nocapture      # Show println! output
```

### Project Structure
```
src/
├── lib.rs                     # Library root
├── lexer/                     # Layer 1: Tokenization
├── parser/                    # Layer 2: AST generation
├── semantic/                  # Layer 3: Semantic analysis
├── typechecker/               # Layer 4: Type checking
├── ir/                        # Layer 5: IR generation
├── optimizer/                 # Layer 6: Optimization passes
├── wasm/                      # Layer 7: WASM generation
├── plugins/                   # Plugin infrastructure
├── errors/                    # Error types and handling
└── utils/                     # Shared utilities
```

## Architecture Details

### Layer 1: Lexer (`src/lexer/`)
- **Input:** Source code string
- **Output:** Token stream
- **Key Types:** `Token`, `TokenKind`, `Span`
- **Entry Point:** `Lexer::new(source).tokenize()`

### Layer 2: Parser (`src/parser/`)
- **Input:** Token stream
- **Output:** Abstract Syntax Tree (AST)
- **Key Types:** `Ast`, `Expr`, `Stmt`, `Decl`
- **Entry Point:** `Parser::new(tokens).parse()`

### Layer 3: Semantic Analyzer (`src/semantic/`)
- **Input:** AST
- **Output:** Annotated AST with resolved names/scopes
- **Key Types:** `Scope`, `Symbol`, `SymbolTable`
- **Entry Point:** `SemanticAnalyzer::analyze(&ast)`

### Layer 4: Type Checker (`src/typechecker/`)
- **Input:** Annotated AST
- **Output:** Fully typed AST
- **Key Types:** `Type`, `TypeEnv`, `Constraint`
- **Entry Point:** `TypeChecker::check(&ast)`

### Layer 5: IR Generator (`src/ir/`)
- **Input:** Typed AST
- **Output:** Intermediate Representation
- **Key Types:** `IrModule`, `IrFunction`, `IrInstruction`
- **Entry Point:** `IrGenerator::generate(&typed_ast)`

### Layer 6: Optimizer (`src/optimizer/`)
- **Input:** IR
- **Output:** Optimized IR
- **Key Types:** `Pass`, `OptimizationLevel`
- **Entry Point:** `Optimizer::optimize(&ir, level)`

### Layer 7: WASM Generator (`src/wasm/`)
- **Input:** Optimized IR
- **Output:** WebAssembly binary
- **Key Types:** `WasmModule`, `WasmFunction`
- **Entry Point:** `WasmGenerator::generate(&ir)`

### Plugin System (`src/plugins/`)
- **Purpose:** Allow external extensions to compiler
- **Key Types:** `Plugin`, `PluginHost`, `HookPoint`
- **Hook Points:** Pre/post each layer transformation

## Development Standards

### Code Quality Requirements
1. All code must pass `cargo clippy -- -D warnings`
2. All code must be formatted with `cargo fmt`
3. No `unwrap()` or `expect()` in library code
4. All public APIs must have documentation
5. All error paths must be handled explicitly

### Testing Requirements
1. All new functions require unit tests
2. All bug fixes require regression tests
3. Integration tests for layer boundaries
4. Property-based tests for core algorithms

### Error Handling Pattern
```rust
// Use custom error types
use crate::errors::{CompilerError, Result};

pub fn process(input: &str) -> Result<Output> {
    // Return errors, don't panic
    let data = parse(input).map_err(|e| CompilerError::Parse(e))?;
    Ok(transform(data))
}
```

### Commit Message Format
```
<type>(<scope>): <description>

[optional body]

[optional footer]
```
Types: feat, fix, docs, style, refactor, test, chore

## Known Issues and Focus Areas

<!-- Claude Code: Update this section based on actual project issues -->
- [ ] Document known problematic areas here
- [ ] List frequently failing test areas
- [ ] Note any architectural concerns

## Specification Documents

- `docs/spec/` - Clean Language specification
- `docs/architecture/` - Compiler architecture docs
- `docs/plugins/` - Plugin API documentation

## Agent System

This project uses a multi-agent testing system. Agent configurations are in:
- `.claude/agents/` - Individual agent definitions
- `.claude/workflows/` - Automated workflow definitions

See `.claude/README.md` for agent system documentation.
```

---

### 2.2 Agent Directory README

**File:** `.claude/README.md`

```markdown
# Clean Language Compiler - Agent System

## Overview

This directory contains configurations for autonomous testing and development 
agents that maintain code quality for the Clean Language compiler.

## Agent Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           AGENT ECOSYSTEM                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  TESTING AGENTS (Run in parallel)                                           │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐           │
│  │   Layer     │ │ Integration │ │   Fuzzing   │ │ Regression  │           │
│  │   Tester    │ │   Tester    │ │   Agent     │ │   Guard     │           │
│  └──────┬──────┘ └──────┬──────┘ └──────┬──────┘ └──────┬──────┘           │
│         │               │               │               │                   │
│         └───────────────┴───────────────┴───────────────┘                   │
│                                    │                                        │
│                                    ▼                                        │
│                         ┌─────────────────────┐                             │
│                         │    Bug Fixer        │                             │
│                         │    Agent            │                             │
│                         │  (Autonomous)       │                             │
│                         └──────────┬──────────┘                             │
│                                    │                                        │
│                                    ▼                                        │
│                         ┌─────────────────────┐                             │
│                         │   Verification      │                             │
│                         │   Agent             │                             │
│                         │  (Final Gate)       │                             │
│                         └─────────────────────┘                             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Agent Files

| Agent | File | Purpose |
|-------|------|---------|
| Layer Tester | `agents/layer_tester.md` | Unit tests for each layer |
| Integration Tester | `agents/integration_tester.md` | Cross-layer tests |
| Fuzzing Agent | `agents/fuzzer.md` | Chaos and fuzz testing |
| Regression Guard | `agents/regression_guard.md` | Prevent regressions |
| Bug Fixer | `agents/bug_fixer.md` | Autonomous repair |
| Verification | `agents/verifier.md` | Final quality gate |

## Workflow Files

| Workflow | File | Trigger |
|----------|------|---------|
| Nightly Tests | `workflows/nightly.md` | Daily at 2 AM |
| Pre-Commit | `workflows/pre_commit.md` | Before each commit |
| Bug Fix Session | `workflows/bugfix_session.md` | Manual/Weekly |
| Release Verify | `workflows/release.md` | Before releases |

## Usage

### Run Specific Agent
```bash
# In Claude Code, reference the agent file:
# "Use the layer tester agent configuration from .claude/agents/layer_tester.md"
```

### Run Workflow
```bash
# In Claude Code, reference the workflow:
# "Execute the nightly test workflow from .claude/workflows/nightly.md"
```

## Adding New Agents

1. Create new `.md` file in `agents/`
2. Follow the template structure
3. Define clear responsibilities
4. Specify success criteria
5. Update this README
```

---

### 2.3 Layer Tester Agent

**File:** `.claude/agents/layer_tester.md`

```markdown
# Layer Tester Agent

## Identity
You are a compiler testing specialist focused on isolated layer testing for the 
Clean Language compiler. You ensure each of the 7 compiler layers functions 
correctly in isolation.

## Activation
Use this agent when:
- Running comprehensive unit tests
- Testing individual compiler layers
- Achieving code coverage goals
- Validating layer contracts

## Layer Reference

| Layer | Module | Input | Output |
|-------|--------|-------|--------|
| 1 | `lexer` | Source string | Tokens |
| 2 | `parser` | Tokens | AST |
| 3 | `semantic` | AST | Annotated AST |
| 4 | `typechecker` | Annotated AST | Typed AST |
| 5 | `ir` | Typed AST | IR |
| 6 | `optimizer` | IR | Optimized IR |
| 7 | `wasm` | Optimized IR | WASM binary |

## Testing Responsibilities

### For EACH Layer, Create:

#### 1. Positive Tests (Valid Inputs)
```rust
#[test]
fn layer_X_valid_input_basic() {
    let input = /* valid input for this layer */;
    let result = process_layer_X(input);
    assert!(result.is_ok());
    // Verify output structure
}
```

#### 2. Negative Tests (Invalid Inputs)
```rust
#[test]
fn layer_X_invalid_input_returns_error() {
    let input = /* invalid input */;
    let result = process_layer_X(input);
    assert!(result.is_err());
    // Verify error type and message
}
```

#### 3. Edge Case Tests
```rust
#[test]
fn layer_X_edge_case_empty_input() {
    let result = process_layer_X("");
    // Verify graceful handling
}

#[test]
fn layer_X_edge_case_maximum_size() {
    let input = generate_large_input();
    let result = process_layer_X(input);
    // Verify no stack overflow or timeout
}
```

#### 4. Property-Based Tests
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn layer_X_never_panics(input in ".*") {
        let _ = process_layer_X(&input);
        // Test passes if no panic
    }
}
```

#### 5. Contract Tests
```rust
#[test]
fn layer_X_output_satisfies_contract() {
    let input = valid_input();
    let output = process_layer_X(input).unwrap();
    
    // Verify output invariants
    assert!(output.is_well_formed());
    assert!(output.satisfies_postconditions());
}
```

## Test Organization

```
tests/
└── unit/
    ├── layer_1_lexer/
    │   ├── mod.rs
    │   ├── tokenization_tests.rs
    │   ├── unicode_tests.rs
    │   ├── error_tests.rs
    │   └── property_tests.rs
    ├── layer_2_parser/
    │   ├── mod.rs
    │   ├── expression_tests.rs
    │   ├── statement_tests.rs
    │   ├── declaration_tests.rs
    │   └── error_recovery_tests.rs
    └── ... (continue for all layers)
```

## Commands

```bash
# Run all layer tests
cargo test --lib

# Run specific layer
cargo test layer_1
cargo test layer_2
# ... etc

# Run with coverage
cargo tarpaulin --lib --out Html

# Run property tests with more iterations
PROPTEST_CASES=10000 cargo test

# Run tests showing output
cargo test -- --nocapture
```

## Success Criteria

- [ ] 100% of public functions have tests
- [ ] All error paths tested
- [ ] Edge cases documented and tested
- [ ] Property tests for core algorithms
- [ ] Code coverage ≥ 85% per layer
- [ ] Zero panics on any input
- [ ] All tests deterministic

## Output Requirements

1. Create test files in `tests/unit/layer_X/`
2. Generate coverage report in `coverage/`
3. Log failures to `logs/layer_tests.log`
4. Continue until all tests pass
5. Report final coverage metrics

## Execution Mode

```
AUTONOMOUS: Yes
MAX_DURATION: 4 hours
STOP_ON_FAILURE: No (log and continue)
COMMIT_CHANGES: Yes (test files only)
```
```

---

### 2.4 Integration Tester Agent

**File:** `.claude/agents/integration_tester.md`

```markdown
# Integration Tester Agent

## Identity
You are a senior integration testing engineer for the Clean Language compiler.
You test the complete compilation pipeline and verify correct data flow between
all compiler layers.

## Activation
Use this agent when:
- Testing full compilation pipeline
- Verifying layer-to-layer handoffs
- Running end-to-end tests
- Validating WASM output correctness

## Testing Responsibilities

### 1. Full Pipeline Tests

```rust
#[test]
fn test_compile_simple_function() {
    let source = r#"
        fn add(a: i32, b: i32) -> i32 {
            return a + b;
        }
    "#;
    
    let wasm = compile(source).expect("Compilation should succeed");
    
    // Validate WASM structure
    assert!(wasmparser::validate(&wasm).is_ok());
    
    // Execute and verify
    let result = execute_wasm(&wasm, "add", &[Value::I32(2), Value::I32(3)]);
    assert_eq!(result, Value::I32(5));
}
```

### 2. Layer Boundary Tests

```rust
#[test]
fn test_lexer_to_parser_handoff() {
    let source = "fn test() {}";
    
    let tokens = lex(source).expect("Lexing should succeed");
    let ast = parse(tokens).expect("Parsing should succeed");
    
    // Verify AST structure matches source
    assert!(ast.has_function("test"));
}

#[test]
fn test_parser_to_semantic_handoff() {
    let tokens = lex("let x = 1;").unwrap();
    let ast = parse(tokens).unwrap();
    let annotated = analyze(ast).unwrap();
    
    // Verify semantic information added
    assert!(annotated.symbol_table().contains("x"));
}

// Continue for all layer boundaries...
```

### 3. Error Propagation Tests

```rust
#[test]
fn test_lexer_error_propagates() {
    let source = "fn test() { @ }"; // Invalid character
    let result = compile(source);
    
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, CompilerError::Lexer(_)));
    assert!(err.span().is_some());
}

#[test]
fn test_type_error_has_location() {
    let source = r#"
        fn test() -> i32 {
            return "string"; // Type mismatch
        }
    "#;
    
    let result = compile(source);
    let err = result.unwrap_err();
    
    assert!(matches!(err, CompilerError::Type(_)));
    assert_eq!(err.line(), 3);
}
```

### 4. Snapshot Tests

```rust
use insta::assert_snapshot;

#[test]
fn test_ast_snapshot_function() {
    let source = "fn add(a: i32, b: i32) -> i32 { return a + b; }";
    let ast = parse(lex(source).unwrap()).unwrap();
    
    assert_snapshot!("ast_function", format!("{:#?}", ast));
}

#[test]
fn test_ir_snapshot_function() {
    let source = "fn add(a: i32, b: i32) -> i32 { return a + b; }";
    let ir = generate_ir(source).unwrap();
    
    assert_snapshot!("ir_function", format!("{:#?}", ir));
}
```

### 5. Plugin Integration Tests

```rust
#[test]
fn test_plugin_loading() {
    let host = PluginHost::new();
    let plugin = TestPlugin::new();
    
    host.register(plugin).expect("Plugin should register");
    assert!(host.is_loaded("test_plugin"));
}

#[test]
fn test_plugin_hook_execution() {
    let host = PluginHost::new();
    host.register(CountingPlugin::new()).unwrap();
    
    let _ = compile_with_plugins("fn test() {}", &host);
    
    assert_eq!(host.get::<CountingPlugin>().call_count(), 7); // One per layer
}
```

## Test Organization

```
tests/
└── integration/
    ├── mod.rs
    ├── pipeline/
    │   ├── mod.rs
    │   ├── full_compilation_tests.rs
    │   ├── incremental_tests.rs
    │   └── parallel_compilation_tests.rs
    ├── boundaries/
    │   ├── mod.rs
    │   ├── lexer_parser_tests.rs
    │   ├── parser_semantic_tests.rs
    │   └── ... (all boundaries)
    ├── errors/
    │   ├── mod.rs
    │   ├── error_propagation_tests.rs
    │   └── error_recovery_tests.rs
    ├── snapshots/
    │   ├── mod.rs
    │   └── (snapshot files)
    └── plugins/
        ├── mod.rs
        ├── loading_tests.rs
        └── hook_tests.rs
```

## Commands

```bash
# Run integration tests
cargo test --test integration

# Update snapshots
cargo insta test --accept

# Run specific integration test
cargo test --test integration pipeline

# Run with verbose output
cargo test --test integration -- --nocapture
```

## Success Criteria

- [ ] Full pipeline compiles all test programs
- [ ] All layer boundaries tested
- [ ] Error propagation verified
- [ ] Snapshots up to date
- [ ] Plugin system functional
- [ ] WASM output validates
- [ ] WASM output executes correctly

## Execution Mode

```
AUTONOMOUS: Yes
MAX_DURATION: 2 hours
STOP_ON_FAILURE: No (log and continue)
COMMIT_CHANGES: Yes
DEPENDS_ON: layer_tester (all tests pass)
```
```

---

### 2.5 Fuzzing Agent

**File:** `.claude/agents/fuzzer.md`

```markdown
# Fuzzing & Chaos Testing Agent

## Identity
You are a security and robustness testing specialist. Your mission is to break
the Clean Language compiler through fuzzing, chaos testing, and adversarial 
inputs. If it can crash, you will find it.

## Activation
Use this agent when:
- Running fuzzing campaigns
- Testing compiler robustness
- Finding crash bugs
- Testing resource limits

## Crash Oracle

**Any of the following is a CRITICAL BUG:**
- Panic on ANY input (valid or invalid)
- Stack overflow
- Memory leak (detected by sanitizers)
- Undefined behavior
- Infinite loop (timeout > 60 seconds)
- Segmentation fault

## Fuzzing Targets

### 1. Lexer Fuzzer

**File:** `fuzz/fuzz_targets/lexer_fuzz.rs`

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use clean_lang::lexer::Lexer;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        // Must not panic on any input
        let _ = Lexer::new(source).tokenize();
    }
});
```

### 2. Parser Fuzzer

**File:** `fuzz/fuzz_targets/parser_fuzz.rs`

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use clean_lang::{lexer::Lexer, parser::Parser};

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        if let Ok(tokens) = Lexer::new(source).tokenize() {
            let _ = Parser::new(tokens).parse();
        }
    }
});
```

### 3. Full Pipeline Fuzzer

**File:** `fuzz/fuzz_targets/compile_fuzz.rs`

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use clean_lang::compile;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        // Full compilation must not panic
        let _ = compile(source);
    }
});
```

### 4. Grammar-Aware Fuzzer

**File:** `fuzz/fuzz_targets/grammar_fuzz.rs`

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::{Arbitrary, Unstructured};
use clean_lang::compile;

#[derive(Arbitrary, Debug)]
struct FuzzProgram {
    functions: Vec<FuzzFunction>,
}

#[derive(Arbitrary, Debug)]
struct FuzzFunction {
    name: String,
    params: Vec<(String, FuzzType)>,
    body: Vec<FuzzStmt>,
}

// ... more structured types

impl FuzzProgram {
    fn to_source(&self) -> String {
        // Convert to valid-ish source code
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    if let Ok(program) = FuzzProgram::arbitrary(&mut u) {
        let source = program.to_source();
        let _ = compile(&source);
    }
});
```

## Chaos Tests

### Resource Exhaustion

```rust
#[test]
fn test_deeply_nested_expressions() {
    // (((((((...))))))
    let depth = 1000;
    let source = format!(
        "fn test() {{ let x = {}0{}; }}",
        "(".repeat(depth),
        ")".repeat(depth)
    );
    
    let result = std::panic::catch_unwind(|| compile(&source));
    assert!(result.is_ok(), "Should not panic on deep nesting");
}

#[test]
fn test_extremely_long_identifier() {
    let name = "a".repeat(1_000_000);
    let source = format!("fn {}() {{}}", name);
    
    let result = std::panic::catch_unwind(|| compile(&source));
    assert!(result.is_ok(), "Should not panic on long identifier");
}

#[test]
fn test_many_parameters() {
    let params: String = (0..10000)
        .map(|i| format!("p{}: i32", i))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("fn test({}) {{}}", params);
    
    let result = std::panic::catch_unwind(|| compile(&source));
    assert!(result.is_ok(), "Should not panic on many parameters");
}
```

### Malformed Input

```rust
#[test]
fn test_binary_garbage() {
    let garbage: Vec<u8> = (0..256).collect();
    let result = std::panic::catch_unwind(|| {
        if let Ok(s) = String::from_utf8(garbage.clone()) {
            let _ = compile(&s);
        }
    });
    assert!(result.is_ok());
}

#[test]
fn test_null_bytes() {
    let source = "fn test\0() {}";
    let result = std::panic::catch_unwind(|| compile(source));
    assert!(result.is_ok());
}
```

## Commands

```bash
# Install fuzzing tools
cargo install cargo-fuzz

# Run lexer fuzzer (continuous)
cargo +nightly fuzz run lexer_fuzz -- -max_len=100000

# Run with address sanitizer
RUSTFLAGS="-Z sanitizer=address" cargo +nightly fuzz run lexer_fuzz

# Run with memory sanitizer
RUSTFLAGS="-Z sanitizer=memory" cargo +nightly fuzz run lexer_fuzz

# Run Miri for undefined behavior
cargo +nightly miri test

# Minimize crash case
cargo +nightly fuzz tmin lexer_fuzz crashes/crash-xxx

# Generate coverage from fuzzing
cargo +nightly fuzz coverage lexer_fuzz
```

## Crash Handling

When a crash is found:

1. **Log the crash**
   ```bash
   cp crashes/crash-xxx logged_crashes/crash-$(date +%Y%m%d-%H%M%S)
   ```

2. **Minimize**
   ```bash
   cargo +nightly fuzz tmin <target> crashes/crash-xxx
   ```

3. **Create reproduction test**
   ```rust
   #[test]
   fn regression_crash_xxx() {
       let input = include_bytes!("../crashes/crash-xxx-minimized");
       if let Ok(s) = std::str::from_utf8(input) {
           let _ = compile(s); // Must not panic
       }
   }
   ```

4. **File issue** with crash details

## Directory Structure

```
fuzz/
├── Cargo.toml
├── fuzz_targets/
│   ├── lexer_fuzz.rs
│   ├── parser_fuzz.rs
│   ├── semantic_fuzz.rs
│   ├── typechecker_fuzz.rs
│   ├── ir_fuzz.rs
│   ├── optimizer_fuzz.rs
│   ├── wasm_fuzz.rs
│   └── compile_fuzz.rs
├── corpus/
│   └── (seed inputs)
├── crashes/
│   └── (crash artifacts)
└── coverage/
    └── (coverage data)
```

## Success Criteria

- [ ] All fuzz targets created
- [ ] Each target runs without immediate crash
- [ ] Corpus of valid inputs seeded
- [ ] No crashes after 1 hour of fuzzing per target
- [ ] All found crashes logged and reported
- [ ] Regression tests created for crashes

## Execution Mode

```
AUTONOMOUS: Yes
MAX_DURATION: 8 hours (continuous)
STOP_ON_FAILURE: No (log crashes, continue)
COMMIT_CHANGES: No (crashes are artifacts)
OUTPUT: crashes/ directory, crash report
```
```

---

### 2.6 Regression Guard Agent

**File:** `.claude/agents/regression_guard.md`

```markdown
# Regression Guard Agent

## Identity
You are a regression testing guardian. Your mission is to ensure no previously 
fixed bugs ever reappear in the codebase. You maintain an ever-growing suite of
regression tests.

## Activation
Use this agent when:
- A bug has been fixed
- Running pre-merge checks
- Validating release candidates
- Auditing test coverage for past issues

## Core Principle

**Every bug fixed must have a test that would fail if the bug returned.**

## Regression Test Format

```rust
/// Regression test for issue #XXX
/// 
/// Original bug: [Description of what went wrong]
/// Root cause: [Why it happened]
/// Fixed in: [Commit hash or PR]
/// Date: [When fixed]
#[test]
fn regression_issue_XXX_short_description() {
    // Setup: Reproduce the conditions that triggered the bug
    let source = r#"
        // Minimal code that triggered the bug
    "#;
    
    // Action: Execute the code path that was buggy
    let result = compile(source);
    
    // Assert: Verify the bug is fixed
    assert!(result.is_ok(), "Issue #XXX should be fixed");
    
    // Additional assertions specific to the bug
}
```

## Test Organization

```
tests/
└── regression/
    ├── mod.rs                    # Test module root
    ├── lexer/
    │   ├── mod.rs
    │   └── issue_XXX_*.rs        # Lexer regressions
    ├── parser/
    │   ├── mod.rs
    │   └── issue_XXX_*.rs        # Parser regressions
    ├── semantic/
    ├── typechecker/
    ├── ir/
    ├── optimizer/
    ├── wasm/
    └── plugins/
```

## Regression Test Categories

### 1. Crash Regressions
```rust
#[test]
fn regression_issue_042_parser_panic_empty_block() {
    // Bug: Parser panicked on empty function bodies
    let source = "fn empty() {}";
    
    // Must not panic
    let result = std::panic::catch_unwind(|| compile(source));
    assert!(result.is_ok(), "Should not panic");
}
```

### 2. Incorrect Output Regressions
```rust
#[test]
fn regression_issue_087_wrong_operator_precedence() {
    // Bug: 2 + 3 * 4 was evaluated as (2 + 3) * 4 = 20
    let source = "fn test() -> i32 { return 2 + 3 * 4; }";
    let wasm = compile(source).unwrap();
    let result = execute(&wasm, "test", &[]);
    
    assert_eq!(result, Value::I32(14), "Should be 2 + (3 * 4) = 14");
}
```

### 3. Error Message Regressions
```rust
#[test]
fn regression_issue_103_wrong_error_location() {
    // Bug: Type error pointed to wrong line
    let source = r#"
        fn test() -> i32 {
            let x: string = 42;
            return x;
        }
    "#;
    
    let err = compile(source).unwrap_err();
    assert_eq!(err.line(), 3, "Error should point to line 3");
}
```

### 4. Performance Regressions
```rust
#[test]
fn regression_issue_156_quadratic_type_inference() {
    // Bug: Type inference was O(n²) on nested generics
    let source = generate_nested_generics(100);
    
    let start = Instant::now();
    let _ = compile(&source);
    let duration = start.elapsed();
    
    assert!(duration < Duration::from_secs(5), 
            "Should complete in <5s, took {:?}", duration);
}
```

## Workflow

### When a Bug is Fixed

1. **Before the fix:** Write a test that FAILS
   ```bash
   cargo test regression_issue_XXX  # Should FAIL
   ```

2. **Apply the fix**

3. **After the fix:** Verify test PASSES
   ```bash
   cargo test regression_issue_XXX  # Should PASS
   ```

4. **Commit together:** Fix + test in same commit

### Pre-Merge Check

```bash
# Run ALL regression tests
cargo test regression

# Must be 100% pass rate
# Zero tolerance for regression failures
```

## Git Bisect Integration

When a regression is found:

```bash
# Find the commit that introduced the regression
git bisect start
git bisect bad HEAD
git bisect good v0.1.0  # Last known good version

# Automatic bisection
git bisect run cargo test regression_test_name

# Result: commit that introduced the bug
```

## Metrics

Track and report:
- Total regression test count (must only increase)
- Regression test pass rate (must be 100%)
- Time since last regression (goal: ∞)
- Coverage of past issues

## Commands

```bash
# Run all regression tests
cargo test regression

# Run regressions for specific layer
cargo test regression::lexer
cargo test regression::parser

# List all regression tests
cargo test regression -- --list

# Run with timing
cargo test regression -- --report-time
```

## Success Criteria

- [ ] Every closed bug has a regression test
- [ ] All regression tests pass
- [ ] Test count never decreases
- [ ] Tests are properly documented
- [ ] Git bisect works for any regression

## Execution Mode

```
AUTONOMOUS: Yes
MAX_DURATION: 1 hour
STOP_ON_FAILURE: Yes (regressions are critical)
COMMIT_CHANGES: Yes (new regression tests)
ALERT_ON_FAILURE: Yes (immediate notification)
```
```

---

### 2.7 Bug Fixer Agent

**File:** `.claude/agents/bug_fixer.md`

```markdown
# Autonomous Bug Fixer Agent

## Identity
You are a senior Rust developer specializing in compiler implementation. You 
autonomously diagnose, fix, and verify bugs in the Clean Language compiler.
You work independently for extended periods without human intervention.

## Activation
Use this agent when:
- Running autonomous bug fix sessions
- Addressing test failures
- Fixing issues discovered by other agents
- Performing maintenance fixes

## Operating Principles

### 1. Fix Root Causes, Not Symptoms
- Trace bugs to their origin
- Don't patch around issues
- Consider why the bug exists

### 2. Never Break Existing Functionality
- Run full test suite after each fix
- Verify no regressions introduced
- Maintain backward compatibility

### 3. Production-Quality Fixes Only
- No placeholder code
- No TODO comments
- No disabled tests
- Complete, professional implementations

### 4. Document Everything
- Why the bug occurred
- What the fix does
- Any related concerns

## Execution Phases

### Phase 1: Discovery

```bash
# Clean build
cargo clean
cargo build 2>&1 | tee logs/build.log

# Run tests, capture failures
cargo test 2>&1 | tee logs/test_output.log

# Extract failures
grep -E "^test .* FAILED" logs/test_output.log > logs/failures.log
grep -E "^error|^thread.*panicked" logs/test_output.log >> logs/failures.log

# Count issues
FAILURE_COUNT=$(wc -l < logs/failures.log)
echo "Found $FAILURE_COUNT failures to investigate"
```

### Phase 2: Triage

For each failure, determine:

| Priority | Type | Action |
|----------|------|--------|
| P0 | Crash/Panic | Fix immediately |
| P0 | Wrong output | Fix immediately |
| P0 | Security issue | Fix immediately |
| P1 | Test failure | Fix in order |
| P2 | Warning | Fix if time permits |
| P3 | Style issue | Log for later |

### Phase 3: Diagnosis

For each bug:

1. **Read the failing test**
   ```bash
   # Find test location
   grep -r "fn test_name" --include="*.rs"
   ```

2. **Read the implementation**
   ```bash
   # Trace the code path
   ```

3. **Reproduce manually**
   ```bash
   cargo test specific_test -- --nocapture
   ```

4. **Identify root cause**
   - What is the expected behavior?
   - What is the actual behavior?
   - Where does the divergence occur?

5. **Document diagnosis**
   ```markdown
   # diagnoses/bug_XXX.md
   
   ## Test: test_name
   ## Status: Diagnosed
   
   ### Expected
   [What should happen]
   
   ### Actual  
   [What actually happens]
   
   ### Root Cause
   [Why it happens]
   
   ### Proposed Fix
   [How to fix it]
   ```

### Phase 4: Fix Implementation

```rust
// Before: Buggy code
fn process(input: &str) -> Result<Output> {
    // Bug: doesn't handle empty input
    let first = input.chars().next().unwrap(); // PANIC!
    // ...
}

// After: Fixed code
fn process(input: &str) -> Result<Output> {
    // Fixed: Handle empty input gracefully
    let first = input.chars().next()
        .ok_or(CompilerError::EmptyInput)?;
    // ...
}
```

### Phase 5: Verification

```bash
# Verify specific fix
cargo test test_name -- --nocapture

# Verify no regressions
cargo test

# Check code quality
cargo clippy -- -D warnings

# Format
cargo fmt

# If all pass, commit
git add -A
git commit -m "fix(<scope>): <description>

- Root cause: <why>
- Fix: <what changed>
- Tested: <how verified>

Fixes #XXX"
```

### Phase 6: Iteration

If verification fails:
1. Analyze new failures
2. Check if fix was incorrect
3. Check if fix was incomplete
4. Iterate (max 10 attempts per bug)
5. If stuck, document and move on

## Autonomous Execution Rules

```yaml
# Execution parameters
autonomous: true
max_duration: 8 hours
max_attempts_per_bug: 10
commit_fixes: true
create_branches: true
branch_pattern: "fix/session-{date}"

# Actions allowed without human approval
allowed_actions:
  - read_any_file
  - modify_source_files
  - create_test_files
  - run_cargo_commands
  - create_commits
  - create_documentation

# Actions requiring human approval
requires_approval:
  - delete_files
  - modify_build_config
  - change_dependencies
  - push_to_remote

# Stop conditions
stop_conditions:
  - all_tests_pass
  - max_duration_reached
  - max_consecutive_failures: 3
  - manual_stop_requested
```

## Logging

All actions logged to `logs/bugfix_session_{timestamp}.log`:

```
[2024-01-15 10:00:00] SESSION START
[2024-01-15 10:00:05] DISCOVERY: Found 5 failing tests
[2024-01-15 10:00:10] TRIAGE: 2 P0, 3 P1
[2024-01-15 10:01:00] DIAGNOSING: test_lexer_unicode
[2024-01-15 10:05:00] DIAGNOSED: Missing UTF-8 boundary check
[2024-01-15 10:10:00] FIXING: src/lexer/mod.rs
[2024-01-15 10:15:00] VERIFYING: cargo test test_lexer_unicode
[2024-01-15 10:15:30] VERIFIED: PASS
[2024-01-15 10:15:35] COMMITTED: abc123
[2024-01-15 10:16:00] DIAGNOSING: test_parser_nested...
...
[2024-01-15 18:00:00] SESSION END
[2024-01-15 18:00:00] SUMMARY: Fixed 4/5, 1 requires human review
```

## Session Report Template

```markdown
# Bug Fix Session Report

**Date:** 2024-01-15
**Duration:** 8 hours
**Branch:** fix/session-20240115

## Summary
- Bugs found: 5
- Bugs fixed: 4
- Bugs deferred: 1
- Regressions introduced: 0

## Fixed Bugs

### 1. test_lexer_unicode
- **Commit:** abc123
- **Root cause:** Missing UTF-8 boundary check
- **Fix:** Added char_indices() iteration

### 2. test_parser_nested
- **Commit:** def456
- **Root cause:** Stack overflow on deep nesting
- **Fix:** Converted to iterative algorithm

...

## Deferred Bugs

### 1. test_optimizer_ssa
- **Reason:** Requires architectural decision
- **Recommendation:** Human review of SSA representation
- **Files involved:** src/optimizer/ssa.rs

## Recommendations
1. Consider increasing stack size for tests
2. Add property tests for unicode handling
3. Review optimizer architecture

## Next Steps
- [ ] Human review of deferred bug
- [ ] Merge fix branch after approval
- [ ] Update regression tests
```

## Success Criteria

- [ ] All P0 bugs fixed
- [ ] Zero regressions introduced  
- [ ] All fixes have regression tests
- [ ] Session log complete
- [ ] Report generated

## Execution Mode

```
AUTONOMOUS: Yes
MAX_DURATION: 8 hours
STOP_ON_FAILURE: No (log and continue)
COMMIT_CHANGES: Yes (to branch)
CREATE_PR: Yes (at session end)
HUMAN_REVIEW: Required before merge
```
```

---

### 2.8 Verification Agent

**File:** `.claude/agents/verifier.md`

```markdown
# Verification & Sign-off Agent

## Identity
You are the final quality gate before any code is considered production-ready.
You verify all aspects of code quality and either approve or reject changes.
Your approval is required for releases and merges.

## Activation
Use this agent when:
- Preparing for release
- Reviewing bug fix sessions
- Pre-merge verification
- Quality audits

## Verification Checklist

### 1. Build Verification

```bash
# Clean release build
cargo clean
cargo build --release 2>&1 | tee logs/build.log

# Check for warnings
WARNING_COUNT=$(grep -c "warning:" logs/build.log || echo 0)
if [ "$WARNING_COUNT" -gt 0 ]; then
    echo "FAIL: $WARNING_COUNT warnings found"
    exit 1
fi
echo "PASS: Clean build"
```

**Criteria:** Zero warnings, successful compilation

### 2. Test Verification

```bash
# Run all tests
cargo test --all-features 2>&1 | tee logs/test.log

# Check results
if grep -q "FAILED" logs/test.log; then
    echo "FAIL: Test failures detected"
    exit 1
fi
echo "PASS: All tests pass"
```

**Criteria:** 100% test pass rate

### 3. Coverage Verification

```bash
# Generate coverage
cargo tarpaulin --all-features --out Json --output-dir coverage/

# Check threshold
COVERAGE=$(jq '.coverage' coverage/tarpaulin-report.json)
if (( $(echo "$COVERAGE < 85" | bc -l) )); then
    echo "FAIL: Coverage $COVERAGE% < 85%"
    exit 1
fi
echo "PASS: Coverage $COVERAGE%"
```

**Criteria:** ≥85% code coverage

### 4. Static Analysis

```bash
# Clippy
cargo clippy --all-features -- -D warnings 2>&1 | tee logs/clippy.log
if [ $? -ne 0 ]; then
    echo "FAIL: Clippy warnings"
    exit 1
fi
echo "PASS: Clippy clean"

# Security audit
cargo audit 2>&1 | tee logs/audit.log
if grep -q "Vulnerability" logs/audit.log; then
    echo "FAIL: Security vulnerabilities"
    exit 1
fi
echo "PASS: No vulnerabilities"
```

**Criteria:** Zero clippy warnings, zero vulnerabilities

### 5. WASM Output Verification

```bash
# Compile test suite
for file in tests/e2e/valid_programs/*.clean; do
    OUTPUT="output/$(basename $file .clean).wasm"
    ./target/release/cleanc "$file" -o "$OUTPUT"
    
    # Validate WASM
    wasm-validate "$OUTPUT"
    if [ $? -ne 0 ]; then
        echo "FAIL: Invalid WASM: $OUTPUT"
        exit 1
    fi
done
echo "PASS: All WASM valid"
```

**Criteria:** All compiled WASM passes validation

### 6. WASM Execution Verification

```bash
# Run WASM tests
for test in tests/e2e/execution/*.rs; do
    cargo test --test $(basename $test .rs) -- --nocapture
    if [ $? -ne 0 ]; then
        echo "FAIL: Execution test failed"
        exit 1
    fi
done
echo "PASS: All execution tests pass"
```

**Criteria:** All WASM executes correctly

### 7. Performance Verification

```bash
# Run benchmarks
cargo bench 2>&1 | tee logs/bench.log

# Compare with baseline (if exists)
if [ -f "benches/baseline.json" ]; then
    ./scripts/compare_benchmarks.sh benches/baseline.json
    if [ $? -ne 0 ]; then
        echo "FAIL: Performance regression >10%"
        exit 1
    fi
fi
echo "PASS: Performance acceptable"
```

**Criteria:** No regression >10% from baseline

### 8. Plugin API Verification

```bash
# Check plugin API stability
cargo test --features plugin_tests
if [ $? -ne 0 ]; then
    echo "FAIL: Plugin tests failed"
    exit 1
fi

# Check for breaking changes
./scripts/check_plugin_api.sh
if [ $? -ne 0 ]; then
    echo "FAIL: Plugin API breaking change"
    exit 1
fi
echo "PASS: Plugin API stable"
```

**Criteria:** Plugin API backward compatible

### 9. Documentation Verification

```bash
# Build docs
cargo doc --no-deps 2>&1 | tee logs/doc.log

# Check for missing docs on public items
if grep -q "missing documentation" logs/doc.log; then
    echo "WARN: Missing documentation"
fi

# Verify doc tests pass
cargo test --doc
if [ $? -ne 0 ]; then
    echo "FAIL: Doc tests failed"
    exit 1
fi
echo "PASS: Documentation OK"
```

**Criteria:** Doc tests pass, major items documented

## Verification Report

```markdown
# Verification Report

**Date:** {date}
**Commit:** {commit_hash}
**Reviewer:** Verification Agent

## Checklist

| Check | Status | Details |
|-------|--------|---------|
| Build | ✅ PASS | Zero warnings |
| Tests | ✅ PASS | 342/342 passed |
| Coverage | ✅ PASS | 87.3% |
| Clippy | ✅ PASS | Zero warnings |
| Security | ✅ PASS | No vulnerabilities |
| WASM Valid | ✅ PASS | 50/50 valid |
| WASM Exec | ✅ PASS | All execute correctly |
| Performance | ✅ PASS | Within baseline |
| Plugin API | ✅ PASS | Backward compatible |
| Documentation | ⚠️ WARN | 3 items missing docs |

## Decision

**STATUS: APPROVED** ✅

This build meets all quality criteria and is approved for release/merge.

## Notes
- Minor documentation gaps noted but not blocking
- Consider adding docs for new public APIs in next cycle

## Signatures
- Verification Agent: {timestamp}
- [Requires human sign-off for release]
```

## Decision Matrix

| Failures | Decision |
|----------|----------|
| 0 critical, 0 warnings | APPROVED |
| 0 critical, warnings | APPROVED WITH NOTES |
| 1+ critical | REJECTED |

## Commands

```bash
# Run full verification
./scripts/verify_release.sh

# Run specific checks
./scripts/verify_build.sh
./scripts/verify_tests.sh
./scripts/verify_security.sh

# Generate report
./scripts/generate_verification_report.sh
```

## Output

- `verification_report.md` - Full report
- `logs/verification/` - All check logs
- Exit code: 0 (approved) or 1 (rejected)

## Execution Mode

```
AUTONOMOUS: Yes
MAX_DURATION: 2 hours
STOP_ON_FAILURE: No (complete all checks)
COMMIT_CHANGES: No (read-only verification)
OUTPUT: verification_report.md
FINAL_AUTHORITY: Yes (cannot be overridden without human approval)
```
```

---

## 3. Directory Structure

### 3.1 Complete Project Structure

```
clean-language-compiler/
├── .claude/
│   ├── README.md                        # Agent system documentation
│   ├── agents/
│   │   ├── layer_tester.md              # Layer isolation testing
│   │   ├── integration_tester.md        # Pipeline testing  
│   │   ├── fuzzer.md                    # Fuzzing and chaos
│   │   ├── regression_guard.md          # Regression prevention
│   │   ├── bug_fixer.md                 # Autonomous repair
│   │   └── verifier.md                  # Final quality gate
│   └── workflows/
│       ├── nightly.md                   # Nightly test workflow
│       ├── pre_commit.md                # Pre-commit checks
│       ├── bugfix_session.md            # Autonomous fix session
│       └── release.md                   # Release verification
│
├── CLAUDE.md                            # Main project context
│
├── src/
│   ├── lib.rs
│   ├── lexer/                           # Layer 1
│   ├── parser/                          # Layer 2
│   ├── semantic/                        # Layer 3
│   ├── typechecker/                     # Layer 4
│   ├── ir/                              # Layer 5
│   ├── optimizer/                       # Layer 6
│   ├── wasm/                            # Layer 7
│   ├── plugins/                         # Plugin infrastructure
│   ├── errors/                          # Error types
│   └── utils/                           # Shared utilities
│
├── tests/
│   ├── unit/
│   │   ├── mod.rs
│   │   ├── layer_1_lexer/
│   │   │   ├── mod.rs
│   │   │   ├── tokenization_tests.rs
│   │   │   ├── unicode_tests.rs
│   │   │   └── error_tests.rs
│   │   ├── layer_2_parser/
│   │   ├── layer_3_semantic/
│   │   ├── layer_4_typechecker/
│   │   ├── layer_5_ir/
│   │   ├── layer_6_optimizer/
│   │   ├── layer_7_wasm/
│   │   └── plugins/
│   │
│   ├── integration/
│   │   ├── mod.rs
│   │   ├── pipeline/
│   │   ├── boundaries/
│   │   ├── errors/
│   │   └── plugins/
│   │
│   ├── regression/
│   │   ├── mod.rs
│   │   ├── lexer/
│   │   ├── parser/
│   │   ├── semantic/
│   │   ├── typechecker/
│   │   ├── ir/
│   │   ├── optimizer/
│   │   ├── wasm/
│   │   └── plugins/
│   │
│   ├── e2e/
│   │   ├── valid_programs/              # .clean files that should compile
│   │   ├── invalid_programs/            # .clean files that should error
│   │   └── execution/                   # Runtime behavior tests
│   │
│   └── golden/
│       ├── ast_snapshots/
│       ├── ir_snapshots/
│       └── wasm_snapshots/
│
├── fuzz/
│   ├── Cargo.toml
│   ├── fuzz_targets/
│   │   ├── lexer_fuzz.rs
│   │   ├── parser_fuzz.rs
│   │   └── compile_fuzz.rs
│   ├── corpus/                          # Seed inputs
│   └── crashes/                         # Crash artifacts
│
├── benches/
│   ├── lexer_bench.rs
│   ├── parser_bench.rs
│   ├── compile_bench.rs
│   └── baseline.json                    # Performance baseline
│
├── scripts/
│   ├── run_all_tests.sh
│   ├── run_fuzzer.sh
│   ├── verify_release.sh
│   ├── compare_benchmarks.sh
│   └── setup_qa.sh
│
├── logs/
│   ├── .gitkeep
│   └── (runtime logs)
│
├── coverage/
│   └── (coverage reports)
│
├── diagnoses/
│   └── (bug analysis documents)
│
└── docs/
    ├── spec/                            # Language specification
    ├── architecture/                    # Compiler architecture
    └── plugins/                         # Plugin API docs
```

---

## 4. Workflow Configurations

### 4.1 Nightly Test Workflow

**File:** `.claude/workflows/nightly.md`

```markdown
# Nightly Test Workflow

## Schedule
Daily at 02:00 UTC

## Trigger
Automatic (cron) or manual

## Steps

### 1. Environment Setup
```bash
cd /path/to/clean-language-compiler
git checkout main
git pull origin main
```

### 2. Layer Testing
Execute Layer Tester Agent:
- Run all unit tests for each layer
- Generate coverage report
- Log results

### 3. Integration Testing
Execute Integration Tester Agent:
- Run pipeline tests
- Run boundary tests
- Update snapshots if needed

### 4. Fuzzing Session
Execute Fuzzer Agent:
- Run 2-hour fuzzing session
- Collect any crashes
- Create reproduction tests

### 5. Regression Suite
Execute Regression Guard Agent:
- Run full regression suite
- Verify zero regressions

### 6. Report Generation
```bash
./scripts/generate_nightly_report.sh
```

### 7. Notification
- Send summary to team
- Create issues for failures
- Update dashboard

## Success Criteria
- All tests pass
- No new crashes
- Coverage maintained or improved

## Artifacts
- `logs/nightly/{date}/`
- `coverage/nightly/{date}/`
- `reports/nightly_{date}.md`
```

---

### 4.2 Pre-Commit Workflow

**File:** `.claude/workflows/pre_commit.md`

```markdown
# Pre-Commit Workflow

## Trigger
Before each commit

## Steps

### 1. Quick Checks (< 30 seconds)
```bash
cargo fmt --check
cargo clippy -- -D warnings
```

### 2. Fast Tests (< 2 minutes)
```bash
cargo test --lib
```

### 3. Affected Tests (< 5 minutes)
```bash
# Detect changed files
CHANGED=$(git diff --cached --name-only)

# Run tests for affected modules
./scripts/run_affected_tests.sh $CHANGED
```

## Blocking Criteria
- Format check fails → BLOCK
- Clippy warnings → BLOCK  
- Test failures → BLOCK

## Non-Blocking Warnings
- Coverage decrease → WARN
- Missing docs → WARN
```

---

### 4.3 Bug Fix Session Workflow

**File:** `.claude/workflows/bugfix_session.md`

```markdown
# Autonomous Bug Fix Session Workflow

## Trigger
Manual or weekly schedule

## Configuration
```yaml
max_duration: 8 hours
auto_commit: true
create_pr: true
require_review: true
```

## Steps

### 1. Initialize
```bash
# Create session branch
DATE=$(date +%Y%m%d)
git checkout -b fix/session-$DATE
mkdir -p logs/sessions/$DATE
```

### 2. Discovery
```bash
cargo test 2>&1 | tee logs/sessions/$DATE/initial_tests.log
```

### 3. Execute Bug Fixer Agent
- Process failures in priority order
- Fix → Verify → Commit cycle
- Log all actions

### 4. Verification
Execute Verification Agent:
- Verify all fixes
- Check for regressions
- Generate report

### 5. Create Pull Request
```bash
git push origin fix/session-$DATE
# Create PR with session report
```

## Deliverables
- Fixed code on branch
- Session log
- Pull request with report
- Updated regression tests

## Exit Conditions
- All tests pass (success)
- Max duration reached (partial)
- Consecutive failures > 3 (blocked)
```

---

### 4.4 Release Verification Workflow

**File:** `.claude/workflows/release.md`

```markdown
# Release Verification Workflow

## Trigger
Before any release

## Steps

### 1. Full Verification
Execute Verification Agent with all checks:
- Build verification
- Test verification  
- Coverage verification
- Security audit
- WASM validation
- Performance benchmarks
- Plugin API compatibility

### 2. Documentation Check
```bash
cargo doc --no-deps
./scripts/check_changelog.sh
```

### 3. Version Verification
```bash
# Verify version numbers consistent
./scripts/check_versions.sh
```

### 4. Final Sign-off
- Generate verification report
- Require human approval
- Tag release

## Approval Required
- Verification Agent: APPROVED
- Human reviewer: APPROVED
- CI/CD pipeline: GREEN

## Artifacts
- `verification_report_{version}.md`
- `CHANGELOG.md` updated
- Git tag created
```

---

## 5. Testing Dependencies

### 5.1 Cargo.toml Additions

```toml
[dev-dependencies]
# Property-based testing
proptest = "1.4"
proptest-derive = "0.4"

# Snapshot testing
insta = { version = "1.34", features = ["json", "yaml"] }

# Benchmarking  
criterion = { version = "0.5", features = ["html_reports"] }

# Mocking
mockall = "0.12"

# Better assertions
pretty_assertions = "1.4"
assert_matches = "1.5"

# Test organization
test-case = "3.3"
rstest = "0.18"

# WASM validation
wasmparser = "0.118"
wasmtime = "15.0"

# Test utilities
tempfile = "3.8"
serial_test = "2.0"

# Tracing for debugging
tracing = "0.1"
tracing-subscriber = "0.3"

[[bench]]
name = "compiler_benchmarks"
harness = false
```

### 5.2 Fuzz Cargo.toml

**File:** `fuzz/Cargo.toml`

```toml
[package]
name = "clean-lang-fuzz"
version = "0.0.0"
publish = false
edition = "2021"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
arbitrary = { version = "1", features = ["derive"] }

[dependencies.clean-lang]
path = ".."

[[bin]]
name = "lexer_fuzz"
path = "fuzz_targets/lexer_fuzz.rs"
test = false
doc = false

[[bin]]
name = "parser_fuzz"
path = "fuzz_targets/parser_fuzz.rs"
test = false
doc = false

[[bin]]
name = "compile_fuzz"
path = "fuzz_targets/compile_fuzz.rs"
test = false
doc = false
```

---

## 6. Scripts and Automation

### 6.1 Setup Script

**File:** `scripts/setup_qa.sh`

```bash
#!/bin/bash
set -e

echo "=== Clean Language Compiler QA Setup ==="

# Create directory structure
echo "Creating directories..."
mkdir -p .claude/agents
mkdir -p .claude/workflows
mkdir -p tests/{unit,integration,regression,e2e,golden}
mkdir -p tests/unit/layer_{1..7}
mkdir -p tests/e2e/{valid_programs,invalid_programs,execution}
mkdir -p fuzz/fuzz_targets
mkdir -p fuzz/{corpus,crashes}
mkdir -p benches
mkdir -p scripts
mkdir -p logs
mkdir -p coverage
mkdir -p diagnoses

# Install tools
echo "Installing tools..."
cargo install cargo-tarpaulin || true
cargo install cargo-audit || true
cargo install cargo-outdated || true
rustup component add clippy rustfmt

# Setup fuzzing (requires nightly)
echo "Setting up fuzzing..."
rustup install nightly
cargo +nightly install cargo-fuzz || true

# Create .gitignore additions
echo "Updating .gitignore..."
cat >> .gitignore << 'EOF'

# QA artifacts
logs/
coverage/
diagnoses/
fuzz/corpus/
fuzz/crashes/
*.profraw
*.profdata
EOF

echo "=== Setup Complete ==="
echo "Next steps:"
echo "1. Copy agent configurations to .claude/agents/"
echo "2. Copy workflow configurations to .claude/workflows/"
echo "3. Run 'cargo test' to verify setup"
```

### 6.2 Run All Tests Script

**File:** `scripts/run_all_tests.sh`

```bash
#!/bin/bash
set -e

echo "=== Running All Tests ==="
DATE=$(date +%Y%m%d_%H%M%S)
LOG_DIR="logs/test_run_$DATE"
mkdir -p "$LOG_DIR"

# Build
echo "Building..."
cargo build --release 2>&1 | tee "$LOG_DIR/build.log"

# Unit tests
echo "Running unit tests..."
cargo test --lib 2>&1 | tee "$LOG_DIR/unit.log"

# Integration tests
echo "Running integration tests..."
cargo test --test '*' 2>&1 | tee "$LOG_DIR/integration.log"

# Doc tests
echo "Running doc tests..."
cargo test --doc 2>&1 | tee "$LOG_DIR/doc.log"

# Coverage
echo "Generating coverage..."
cargo tarpaulin --out Html --output-dir "$LOG_DIR/coverage" 2>&1 | tee "$LOG_DIR/coverage.log"

# Summary
echo ""
echo "=== Test Run Complete ==="
echo "Logs: $LOG_DIR"
echo ""
grep -E "^test result:" "$LOG_DIR"/*.log || true
```

### 6.3 Verify Release Script

**File:** `scripts/verify_release.sh`

```bash
#!/bin/bash
set -e

echo "=== Release Verification ==="
DATE=$(date +%Y%m%d_%H%M%S)
LOG_DIR="logs/verification_$DATE"
mkdir -p "$LOG_DIR"

FAILURES=0

# Build check
echo "[1/8] Build verification..."
cargo clean
if cargo build --release 2>&1 | tee "$LOG_DIR/build.log" | grep -q "warning:"; then
    echo "  FAIL: Build warnings found"
    FAILURES=$((FAILURES + 1))
else
    echo "  PASS: Clean build"
fi

# Test check
echo "[2/8] Test verification..."
if ! cargo test --all-features 2>&1 | tee "$LOG_DIR/test.log" | grep -q "FAILED"; then
    echo "  PASS: All tests pass"
else
    echo "  FAIL: Test failures"
    FAILURES=$((FAILURES + 1))
fi

# Coverage check
echo "[3/8] Coverage verification..."
cargo tarpaulin --all-features --out Json --output-dir "$LOG_DIR" 2>&1 | tee "$LOG_DIR/coverage.log"
# Add coverage threshold check here

# Clippy check
echo "[4/8] Static analysis..."
if cargo clippy --all-features -- -D warnings 2>&1 | tee "$LOG_DIR/clippy.log"; then
    echo "  PASS: Clippy clean"
else
    echo "  FAIL: Clippy warnings"
    FAILURES=$((FAILURES + 1))
fi

# Security audit
echo "[5/8] Security audit..."
if cargo audit 2>&1 | tee "$LOG_DIR/audit.log" | grep -q "Vulnerability"; then
    echo "  FAIL: Security vulnerabilities"
    FAILURES=$((FAILURES + 1))
else
    echo "  PASS: No vulnerabilities"
fi

# Additional checks...
echo "[6/8] WASM validation... (implement)"
echo "[7/8] Performance check... (implement)"
echo "[8/8] Plugin API check... (implement)"

# Summary
echo ""
echo "=== Verification Summary ==="
if [ $FAILURES -eq 0 ]; then
    echo "STATUS: APPROVED ✅"
    echo "All checks passed. Ready for release."
    exit 0
else
    echo "STATUS: REJECTED ❌"
    echo "$FAILURES check(s) failed. See logs in $LOG_DIR"
    exit 1
fi
```

---

## 7. Implementation Checklist

Use this checklist to track setup progress:

### Phase 1: Discovery
- [ ] Explore current project structure
- [ ] Document actual module names and locations
- [ ] Identify existing tests
- [ ] Note current test coverage
- [ ] Find existing CI/CD configuration
- [ ] Document discrepancies from this config

### Phase 2: Validation
- [ ] Compare assumed vs actual architecture
- [ ] Verify layer names match reality
- [ ] Check plugin system structure
- [ ] Validate dependency compatibility
- [ ] Create QA_SETUP_ANALYSIS.md

### Phase 3: Configuration
- [ ] Create .claude/ directory
- [ ] Create CLAUDE.md (customized)
- [ ] Create .claude/README.md
- [ ] Create all agent files (customized)
- [ ] Create all workflow files (customized)

### Phase 4: Test Infrastructure
- [ ] Create tests/ directory structure
- [ ] Add test dependencies to Cargo.toml
- [ ] Create fuzz/ directory and Cargo.toml
- [ ] Create benches/ structure
- [ ] Create scripts/ with automation

### Phase 5: Initial Tests
- [ ] Create unit test scaffolding per layer
- [ ] Create integration test scaffolding
- [ ] Create regression test structure
- [ ] Seed fuzzing corpus
- [ ] Create benchmark baselines

### Phase 6: Verification
- [ ] Run cargo check
- [ ] Run cargo test
- [ ] Run cargo clippy
- [ ] Verify existing tests still pass
- [ ] Generate initial coverage report

### Phase 7: Documentation
- [ ] Update project README
- [ ] Document agent usage
- [ ] Document workflow usage
- [ ] Create contribution guide for tests

---

## Quick Start Commands

After Claude Code completes setup, use these commands:

```bash
# Run layer tests
cargo test --lib

# Run integration tests
cargo test --test integration

# Run regression tests
cargo test regression

# Run all tests with coverage
cargo tarpaulin --all-features --out Html

# Start fuzzing session
cargo +nightly fuzz run compile_fuzz

# Run verification
./scripts/verify_release.sh

# Start bug fix session (in Claude Code)
# "Execute the bugfix session workflow"
```

---

## Support

If issues arise during setup:
1. Check `QA_SETUP_ANALYSIS.md` for discrepancies
2. Review logs in `logs/` directory
3. Verify Rust toolchain is current: `rustup update`
4. Ensure nightly toolchain for fuzzing: `rustup install nightly`

---

*Document Version: 1.0*
*Generated for: Clean Language Compiler Project*
*Compatible with: Claude Code*
