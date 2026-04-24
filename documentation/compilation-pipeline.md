# Compilation Pipeline

The Clean Language compiler transforms source code to WebAssembly through seven stages. The authoritative entry point is `compile_with_plugins_and_opt_level()` in `src/lib.rs`. This document describes what each stage does, what data structure it produces, where it lives in the source tree, and what crosses the stage boundary.

For language semantics and type rules, see [`spec/semantic-rules.md`](../../spec/semantic-rules.md) and [`spec/type-system.md`](../../spec/type-system.md). For the WASM memory layout, see [`platform-architecture/MEMORY_MODEL.md`](../../platform-architecture/MEMORY_MODEL.md).

---

## Pipeline Overview

```
Source (.cln file)
        |
  Stage 1: Lexer           src/lexer/specification_lexer.rs
        |  Token stream
  Stage 2: Parser          src/parser/token_parser.rs + expression_parser.rs + ...
        |  ast::Program
  Stage 2.5: Plugin processing
        |    2.5a  enforce plugin rules      src/plugins/enforcer.rs
        |    2.5b  expand framework blocks   src/plugins/expander.rs
        |    2.6   register bridge functions src/plugins/bridge.rs
        |  ast::Program (augmented)
  Stage 3: HIR Builder     src/hir/hir_builder.rs
        |  HirProgram
  Stage 4: Resolver        src/resolver/
        |  HirProgram with SymbolId annotations
  Stage 5: Type Checker    src/typechecker/
        |  TAST (Typed AST)
  Stage 6: MIR Lowering    src/mir/mir_builder.rs + optimization.rs
        |  MirProgram (SSA form)
  Stage 7: WASM Codegen    src/codegen/mir_codegen.rs
        |  Vec<u8>  (WASM binary)
```

---

## Stage 1 — Lexical Analysis

**Source:** `src/lexer/specification_lexer.rs`  
**Input:** `&str` (raw source text)  
**Output:** `Vec<Token>` or a token iterator  
**Errors emitted:** `SYN001` (InvalidToken)

The lexer converts the raw source string into a flat sequence of typed tokens. Clean Language uses significant indentation, so the lexer emits explicit `Indent` and `Dedent` tokens rather than relying on the parser to count whitespace. Tab characters (`\t`) are the only valid indentation character; a space where a tab is expected triggers `SYN003` / `SYN006`.

Key token categories:

| Category | Examples |
|----------|---------|
| Keywords | `start`, `state`, `functions`, `class`, `if`, `while`, `for`, `return` |
| Identifiers | `myVar`, `calculateTotal` |
| Literals | `42`, `3.14`, `"hello"`, `true` |
| Operators | `+`, `-`, `*`, `/`, `==`, `!=`, `<`, `>`, `and`, `or`, `not` |
| Structural | `Indent`, `Dedent`, `Newline`, `(`, `)`, `[`, `]`, `:` |

Plugin keywords are registered with the lexer before it runs, so the lexer can emit plugin-defined token types (e.g., `endpoint:`, `model:`, `component:`) as first-class tokens rather than identifiers.

---

## Stage 2 — Parsing

**Source:** `src/parser/token_parser.rs`, `src/parser/expression_parser.rs`, `src/parser/statement_parser.rs`, `src/parser/class_parser.rs`  
**Input:** Token stream + plugin keyword set  
**Output:** `ast::Program`  
**Errors emitted:** `SYN002`–`SYN008`

The parser is a hand-written recursive descent parser operating on a `TokenCursor`. It produces an Abstract Syntax Tree defined in `src/ast/mod.rs`.

### AST Root Structure

```
ast::Program
├── functions: Vec<ast::Function>
├── classes: Vec<ast::Class>
├── start_block: Option<ast::Block>
├── state_block: Option<ast::StateBlock>
├── external_functions: Vec<ast::ExternalFunction>   // from plugin bridge
├── watch_blocks: Vec<ast::WatchBlock>
└── plugin_blocks: Vec<ast::PluginBlock>             // unexpanded framework DSL
```

Each node carries a `SourceLocation { file_id: FileId, line: u32, column: u32 }` used throughout later stages for error reporting.

### Key AST Node Types

```
ast::Function
    name: String
    parameters: Vec<(String, ast::TypeAnnotation)>
    return_type: ast::TypeAnnotation
    body: ast::Block

ast::Class
    name: String
    parent: Option<String>
    fields: Vec<ast::Field>
    methods: Vec<ast::Function>

ast::Statement  (enum)
    VariableDeclaration { name, type_ann, initializer, location }
    Assignment { target, value, location }
    If { condition, then_block, else_block, location }
    While { condition, body, location }
    For { variable, iterable, body, location }
    Return { value: Option<Expression>, location }
    Expression { expr, location }
    Print { args, location }

ast::Expression  (enum)
    Literal { value: ast::LiteralValue, location }
    Variable { name: String, location }
    BinaryOp { op, left, right, location }
    UnaryOp { op, operand, location }
    Call { name, arguments, location }
    MethodCall { receiver, method, arguments, location }
    FieldAccess { receiver, field, location }
    IndexAccess { receiver, index, location }
    StringInterpolation { parts, location }
```

### Error Recovery

The parser does not abort on the first error. `ErrorRecovery` (see [Error Handling Guide](./error-handling-guide.md) §Parser Error Recovery) synchronizes to the next statement or declaration boundary, then continues. This allows a single compilation pass to report multiple independent errors.

---

## Stage 2.5 — Plugin Processing

This stage runs only when plugins are declared in `plugins:`. It has three sub-steps that all operate on the `ast::Program` produced by Stage 2.

### Stage 2.5a — Plugin Rule Enforcement

**Source:** `src/plugins/enforcer.rs`

Checks that the program obeys plugin constraints: forbidden functions in certain contexts, required block types, folder naming conventions for framework projects. Emits `PLUGIN001` / `PLUGIN002`.

### Stage 2.5b — Plugin Block Expansion

**Source:** `src/plugins/expander.rs`

Framework DSL blocks (e.g., `endpoint:`, `component:`, `model:`) are not native Clean Language syntax. The expander calls into the plugin's WASM module to transform each `ast::PluginBlock` into a sequence of standard `ast::Statement` and `ast::Function` nodes. The resulting `ast::Program` contains no remaining `PluginBlock` nodes after this step.

The compiler does not contain any plugin-specific logic. If the plugin WASM produces wrong output, the bug is in the codegen that compiled the plugin — not in the compiler's expander. See `management/ARCHITECTURE_BOUNDARIES.md` for the boundary rule.

### Stage 2.6 — Bridge Function Registration

**Source:** `src/plugins/bridge.rs`

Each installed plugin's `plugin.toml` declares `[bridge]` functions — host functions that the WASM module will import. This step converts those declarations into `ast::ExternalFunction` nodes appended to the `ast::Program`. Later stages treat these exactly like user-declared `external` functions.

---

## Stage 3 — HIR Building

**Source:** `src/hir/hir_builder.rs`  
**Input:** `ast::Program`  
**Output:** `HirProgram`  
**Errors emitted:** `FUNC001`–`FUNC007`, `CLASS001`–`CLASS004`, `STATE001`, `STATE003`–`STATE005`, `SYN007`

The HIR (High-level Intermediate Representation) is a desugared, structurally validated form of the AST. The HIR builder:

- Validates file section ordering (`SYN007`).
- Checks function arity at declaration sites (`FUNC006`, `FUNC007`).
- Validates class structures: duplicate fields (`CLASS002`), duplicate methods (`CLASS003`), missing constructors (`CLASS004`).
- Validates state blocks: computed state type consistency (`STATE003`), computed state mutation (`STATE004`), rules expressions (`STATE005`).
- Desugars `for` loops into `while` loops with an explicit index variable.
- Lowers string interpolation into a sequence of concatenation calls.

### HIR Type Definitions (`src/hir/mod.rs`)

```
HirProgram
    functions: Vec<HirFunction>
    classes: Vec<HirClass>
    start_block: Option<HirBlock>
    state_block: Option<HirStateBlock>
    externals: Vec<HirExternalFunction>
    watch_blocks: Vec<HirWatchBlock>

HirFunction
    name: String
    symbol_id: SymbolId          // assigned in Stage 4
    parameters: Vec<HirParam>
    return_type: HirType
    body: HirBlock
    location: SourceLocation

HirBlock
    statements: Vec<HirStatement>

HirStatement  (enum)
    VariableDeclaration { symbol_id, type_ann, initializer, location }
    Assignment { target, value, location }
    If { condition, then_block, else_block, location }
    While { condition, body, location }
    Return { value: Option<HirExpression>, location }
    ExpressionStatement { expr, location }

HirExpression  (enum)
    Literal { value: HirLiteral, location }
    Variable { symbol_id: Option<SymbolId>, name: String, location }
    BinaryOp { op: HirBinOp, left, right, location }
    UnaryOp { op: HirUnOp, operand, location }
    Call { symbol_id: Option<SymbolId>, name: String, arguments, location }
    MethodCall { receiver, method: String, arguments, location }
    FieldAccess { receiver, field: String, location }
    IndexAccess { receiver, index, location }
```

---

## Stage 4 — Name Resolution

**Source:** `src/resolver/`  
**Input:** `HirProgram` (with `symbol_id: None` on all name references)  
**Output:** `HirProgram` (with `symbol_id: Some(SymbolId)` on all resolved names)  
**Errors emitted:** `SCOPE001`–`SCOPE005`, `SEM002`, `SEM003`, `IMPORT001`–`IMPORT004`, `FUNC001`

The resolver performs a two-pass walk:

1. **Declaration pass**: registers every function, class, state variable, and external function into the `SymbolTable`, assigning each a unique `SymbolId`.
2. **Reference pass**: resolves every `Variable`, `Call`, and `MethodCall` node by looking up the name in the current scope chain, then writing the resolved `SymbolId` back into the HIR node.

Bridge functions registered in Stage 2.6 are added to the symbol table in the declaration pass, making them available as first-class call targets.

### Scope Rules

Scopes nest: function body → block (if/while body) → inner block. The resolver maintains a `ScopeStack` — a `Vec<HashMap<String, SymbolId>>`. Pushing a new scope on block entry and popping it on exit ensures that inner declarations shadow outer ones within their extent, and that out-of-scope accesses are caught (`SCOPE001`).

---

## Stage 5 — Type Checking

**Source:** `src/typechecker/`  
**Input:** Resolved `HirProgram`  
**Output:** TAST (Typed AST) — the HIR annotated with fully resolved `HirType` at every expression node  
**Errors emitted:** `SEM001`, `SEM004`–`SEM009`, `FUNC002`–`FUNC005`, `IDX001`–`IDX004`

The type checker uses constraint-based inference. For each expression it either:

- Assigns a concrete type directly from a literal or declared type annotation, or
- Generates a type constraint (e.g., "left operand of `+` must equal right operand's type") and solves the constraint set.

Type compatibility rules are defined in [`spec/type-system.md`](../../spec/type-system.md). The type checker consults those rules directly — it does not duplicate them.

When a type error is encountered, `SemanticErrorRecovery::recover_from_type_error` assigns `Type::Error` or `Type::Unknown` to the failing node so that downstream checking can continue. See [Error Handling Guide](./error-handling-guide.md) §Semantic Error Recovery.

### Type Representation in the HIR

```
HirType  (enum)
    Integer          // 64-bit signed
    Number           // 64-bit float
    Boolean
    String
    Void
    List(Box<HirType>)
    Matrix(Box<HirType>)
    Pairs
    Any
    Class(String)    // nominal class type by name
    Error            // error recovery sentinel
    Unknown          // unresolved during inference
```

---

## Stage 6 — MIR Lowering

**Source:** `src/mir/mir_builder.rs`, `src/mir/optimization.rs`  
**Input:** TAST + `opt_level: u8` (0–3)  
**Output:** `MirProgram`  
**Errors emitted:** `COM001`–`COM003`

The MIR (Mid-level Intermediate Representation) is in SSA (Static Single Assignment) form. Each value is assigned exactly once. Control flow is represented as a graph of `BasicBlock`s connected by `Terminator` instructions.

### MIR Type Definitions (`src/mir/mir_types.rs`)

```
MirProgram
    functions: HashMap<SymbolId, MirFunction>
    globals: Vec<MirGlobal>
    string_pool: StringPool           // deduplicated string literals
    externals: Vec<MirExternalFunction>

MirFunction
    symbol_id: SymbolId
    name: String
    parameters: Vec<(String, MirType)>
    return_type: MirType
    basic_blocks: Vec<BasicBlock>
    attributes: FunctionAttributes    // e.g., export, inline

BasicBlock
    id: BasicBlockId
    instructions: Vec<MirInstruction>
    terminator: Terminator

Terminator  (enum)
    Return(Option<ValueId>)
    Branch { condition: ValueId, then_block: BasicBlockId, else_block: BasicBlockId }
    Jump(BasicBlockId)
    Unreachable

MirType  (enum)
    I32
    I64
    F32
    F64
    Ptr(Box<MirType>)   // Ptr(I8) for string literals, Ptr(U8) for runtime strings
    Bool
    Void
    Struct(Vec<MirType>)
```

**Important:** `Ptr(I8)` and `Ptr(U8)` are both lowered to `i32` in WASM. The distinction is internal MIR bookkeeping for string operations. `is_string_operand()` must check for both variants. See `KNOWLEDGE.md` for the history of this distinction.

### Optimization Passes

| Level | Passes applied |
|-------|---------------|
| 0 | None (fastest compilation; best for debug) |
| 1 | Constant folding |
| 2 | Constant folding, dead code elimination, copy propagation (default) |
| 3 | All level-2 passes + inlining, loop invariant code motion |

Optimization runs in `src/mir/optimization.rs` as a sequence of passes over the `MirProgram`. Each pass is idempotent: running it twice produces the same result.

---

## Stage 7 — WASM Code Generation

**Source:** `src/codegen/mir_codegen.rs`  
**Input:** `MirProgram` + bridge function list  
**Output:** `Vec<u8>` (WASM binary)  
**Errors emitted:** `COM001`, `COM004`–`COM006`

`MirCodeGenerator` walks the `MirProgram` and emits WASM binary using the `wasm-encoder` crate. One WASM function is emitted per `MirFunction`. Bridge functions become WASM `import` entries; no implementation is emitted for them.

### Code Generation Flow

For each `MirFunction`:

1. Allocate a `wasm_encoder::Function` and declare its local variable types.
2. Walk `basic_blocks` in order. Each `BasicBlock` maps to a WASM block or loop instruction depending on the control flow structure.
3. For each `MirInstruction`, emit the corresponding WASM opcode sequence. Type-specific paths exist for integer vs. float arithmetic.
4. Emit the `Terminator` as a WASM `return`, `br_if`, `br`, or `unreachable`.

### Host Bridge Contract

External functions declared by plugins must generate WASM imports matching the host bridge signatures exactly:

- String parameters use `(ptr: i32, len: i32)` pairs (length-prefixed).
- `integer` values in host-boundary functions use `i64` (not `i32`).
- The authoritative signatures are in [`platform-architecture/HOST_BRIDGE.md`](../../platform-architecture/HOST_BRIDGE.md).

Any mismatch between the compiler-generated import and the host's export is a contract violation that produces a WASM link error at runtime.

### String Pool

The `StringPool` in `MirProgram` holds all string literals used in the program. During codegen, each unique string is written to the WASM data section at a fixed offset. The pool tracks `(content, wasm_offset)` pairs. When a `MirInstruction` references a string literal, codegen emits an `i32.const <wasm_offset>` instruction. See [`platform-architecture/MEMORY_MODEL.md`](../../platform-architecture/MEMORY_MODEL.md) for the memory layout.

### Function Table and Indirect Calls

When a function is used as a value (passed as an argument or stored in a variable), it is placed in the WASM function table and referenced by its table index. `MirCodeGenerator::add_function_alias` registers alternate names so that both the canonical name and any aliases resolve to the same WASM function index.

---

## Data Flow Summary

The table below shows what leaves each stage boundary and where it is defined:

| Stage boundary | Type | Defined in |
|----------------|------|-----------|
| After Stage 1 | `Vec<Token>` | `src/lexer/token.rs` |
| After Stage 2 | `ast::Program` | `src/ast/mod.rs` |
| After Stage 2.5 | `ast::Program` (augmented) | `src/ast/mod.rs` + `src/plugins/` |
| After Stage 3 | `HirProgram` | `src/hir/mod.rs` |
| After Stage 4 | `HirProgram` (all SymbolIds resolved) | `src/hir/mod.rs` |
| After Stage 5 | TAST (HirProgram with type annotations) | `src/hir/mod.rs` + `src/typechecker/` |
| After Stage 6 | `MirProgram` | `src/mir/mir_types.rs` |
| After Stage 7 | `Vec<u8>` (WASM binary) | — |

---

## Adding a New Compilation Stage

If a new transformation pass is needed (e.g., a new optimization or a new IR lowering):

1. Define the new output type in `src/<ir>/mod.rs`.
2. Implement the transformation as a `fn transform(input: &InputType, ...) -> Result<OutputType, CompilerError>`.
3. Insert the call in `compile_with_plugins_and_opt_level()` in `src/lib.rs` at the correct position in the pipeline.
4. Update this document with the new stage entry and add it to the data flow summary table.
5. Error codes for the new stage must be defined in `spec/error-codes.md` before use (requires developer approval per Principle 25).

---

## Debugging the Pipeline

To observe the state at any stage boundary, use the debug CLI commands:

```bash
# Show the AST produced by Stage 2
cln debug file.cln --show-ast

# Show the MIR produced by Stage 6
cln debug file.cln --show-mir

# Show the parsed token stream from Stage 1
cargo run --bin debug_parser -- file.cln

# Disassemble the WASM binary from Stage 7
cargo run --bin debug_wasm -- output.wasm

# Run with wasmtime (provides all host bridge functions)
./target/release/wasmtime_runner output.wasm
```

Internal compiler tracing at any stage can be enabled per-module via `RUST_LOG`:

```bash
RUST_LOG=clean_language_compiler::hir=debug cln compile file.cln -o output.wasm
RUST_LOG=clean_language_compiler::codegen=trace cln compile file.cln -o output.wasm
```
