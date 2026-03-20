# Compilation Pipeline

The Clean Language compiler transforms source code to WebAssembly through 7 stages. The main entry point is `compile_with_plugins_and_opt_level()` in `src/lib.rs`.

## Pipeline Overview

```
Source Code (.cln)
       |
  Stage 1: Lexer (SpecificationLexer)
       |
  Stage 2: Parser (SpecificationParser) → AST (ast::Program)
       |
  Stage 2.5a: Plugin Enforcement (optional)
  Stage 2.5b: Plugin Expansion (PluginExpander)
  Stage 2.6: Bridge Function Registration
       |
  Stage 3: HIR Builder → HirProgram
       |
  Stage 4: Resolver → Resolved HIR (name/symbol resolution)
       |
  Stage 5: Type Checker → TAST (typed AST)
       |
  Stage 6: MIR Lowering → MirProgram (SSA form, optimizations)
       |
  Stage 7: WASM Code Generation (MirCodeGenerator) → .wasm bytes
```

## Stage Details

### Stage 1: Lexical Analysis
- **Source**: `src/lexer/specification_lexer.rs`
- **Input**: Raw source string
- **Output**: Token stream
- Tokenizes Clean Language source into typed tokens

### Stage 2: Parsing
- **Source**: `src/parser/` (token_parser.rs, expression_parser.rs, etc.)
- **Input**: Token stream + plugin keywords
- **Output**: `ast::Program`
- Builds the Abstract Syntax Tree using a recursive descent parser
- Plugin keywords are passed so the parser recognizes plugin-defined blocks

### Stage 2.5: Plugin Processing
- **2.5a**: Enforces plugin rules (restricted functions, required blocks, folder conventions)
- **2.5b**: Expands framework blocks into standard Clean AST (`src/plugins/expander.rs`)
- **2.6**: Converts plugin bridge functions from `plugin.toml` into `ast::ExternalFunction` declarations

### Stage 3: HIR (High-level IR)
- **Source**: `src/hir/hir_builder.rs`
- **Input**: `ast::Program`
- **Output**: `HirProgram`
- Desugars syntax and validates semantic structure
- Key types: `HirProgram`, `HirFunction`, `HirStatement`, `HirExpression` (in `src/hir/mod.rs`)

### Stage 4: Name Resolution
- **Source**: `src/resolver/`
- **Input**: `HirProgram` + bridge functions
- **Output**: Resolved HIR with symbol IDs
- Resolves all names to unique `SymbolId` values
- Registers bridge functions from plugin registry

### Stage 5: Type Checking
- **Source**: `src/typechecker/`
- **Input**: Resolved HIR
- **Output**: TAST (Typed AST)
- Constraint-based type inference
- Produces fully typed representation

### Stage 6: MIR Lowering
- **Source**: `src/mir/mir_builder.rs`, `src/mir/optimization.rs`
- **Input**: TAST + optimization level (0-3)
- **Output**: `MirProgram`
- Lowers to SSA form with basic blocks and control flow graphs
- Key types: `MirProgram`, `MirFunction`, `BasicBlockId`, `ValueId`, `MirType` (in `src/mir/mir_types.rs`)
- Applies optimizations based on opt_level

### Stage 7: WASM Generation
- **Source**: `src/codegen/mir_codegen.rs`
- **Input**: `MirProgram` + bridge functions
- **Output**: WebAssembly binary bytes
- Generates WASM using `wasm-encoder`
- Bridge functions become WASM import entries

## Key Data Types

### AST (`src/ast/mod.rs`)
```
Program → functions, classes, statements, externals, start_block, state_block...
```

### HIR (`src/hir/mod.rs`)
```
HirProgram → HirFunction, HirClass, HirStateBlock, HirWatchBlock, HirExternalFunction
HirFunction → name, parameters, return_type, body (HirBlock)
HirBlock → Vec<HirStatement>
HirStatement → VariableDeclaration, Assignment, If, While, For, Return, Expression...
HirExpression → Literal, Variable, BinaryOp, Call, MethodCall, FieldAccess...
```

### MIR (`src/mir/mir_types.rs`)
```
MirProgram → functions (HashMap<SymbolId, MirFunction>), globals, string_pool, externals
MirFunction → basic_blocks (Vec<BasicBlock>), parameters, return_type, attributes
BasicBlock → instructions (Vec<MirInstruction>), terminator
MirType → I32, I64, F32, F64, Ptr(Box<MirType>), Void, Bool, Struct(...)
```

## Optimization Levels

| Level | Description |
|-------|-------------|
| 0 | No optimization (fastest compilation, for debugging) |
| 1 | Light optimization |
| 2 | Standard optimization (default) |
| 3 | Aggressive optimization (speed + size) |
