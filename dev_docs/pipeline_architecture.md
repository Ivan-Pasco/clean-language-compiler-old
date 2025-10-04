# Clean Language Compiler Pipeline Architecture

Status: Normative for implementation
Authority: Based on dev_docs/AST_Specification.md and industry best practices
Version: 1.0 - Complete pipeline redesign for 100% specification alignment
Updated: December 2024

## 1. Overview

This document defines the complete 7-stage pipeline architecture for the Clean Language compiler, designed to achieve 100% alignment with the AST Specification while incorporating best practices from modern compiler design.

### Design Philosophy

1. **Exact Specification Adherence**: Every stage must represent exactly what the Clean Language Specification defines
2. **Clear Stage Boundaries**: Each stage has well-defined inputs, outputs, and responsibilities  
3. **Immutable Data Structures**: Use IDs and arenas, avoid cross-node pointers
4. **Source Preservation**: Maintain source locations throughout for error reporting
5. **Future-Proof Architecture**: Ready for incremental compilation and parallelization
6. **Clean Code**: We want to write clean code that is easy to understand and maintain, you can replace the legacy code with the new code. You can refactor the code to make it more readable and maintainable but do not add any additional features that are not in the specification.

### Pipeline Flow

```
Source Code (.cln)
    ↓
Stage 1: Lexical Analysis → Token Stream
    ↓
Stage 2: Parsing → AST (Untyped, specification-compliant)
    ↓
Stage 3: Validation & Desugaring → HIR (High-level IR)
    ↓  
Stage 4: Name/Module Resolution → HIR (with symbols)
    ↓
Stage 5: Type Inference & Checking → TAST (Typed AST)
    ↓
Stage 6: Lower to MIR → MIR (Mid-level IR) [Optional]
    ↓
Stage 7: Code Generation → WebAssembly Binary
```

## 2. Stage Definitions

### Stage 1: Lexical Analysis & Tokenization

**Purpose**: Convert source code into a stream of tokens with exact specification compliance.

**Input**: 
- Raw source code (UTF-8 .cln files)
- File path information

**Output**:
- Token stream with SourceLocation for each token
- Preserved whitespace information for error reporting

**Components**:
- `src/lexer/mod.rs` - Main lexer interface
- `src/lexer/token.rs` - Token definitions matching specification
- `src/lexer/keywords.rs` - Keyword recognition
- `src/lexer/literals.rs` - Literal parsing (including precision modifiers)

**Key Requirements**:
- Support all keywords from Clean Language Specification
- Parse all literal types: integers, numbers, strings, booleans, lists, matrices
- Handle precision modifiers: `integer:64`, `integer:32u`, `number:32`
- Support string interpolation tokenization: `"Hello {name}"`
- Preserve exact source positions for error reporting
- Handle tab-based indentation as per specification

**Token Types (from specification)**:
```rust
pub enum TokenType {
    // Literals
    IntegerLiteral(i64),
    NumberLiteral(f64),
    StringLiteral(String),
    BooleanLiteral(bool),
    
    // Precision Literals
    Integer8Literal(i8),
    Integer8uLiteral(u8),
    Integer16Literal(i16),
    Integer16uLiteral(u16),
    Integer32Literal(i32),
    Integer64Literal(i64),
    Number32Literal(f32),
    Number64Literal(f64),
    
    // Keywords (all from specification)
    And, Class, Constructor, Else, Error, False, For, From,
    Function, If, Import, In, Iterate, Not, OnError, Or, 
    Print, Println, Return, Start, Step, Test, Tests, This,
    To, True, While, Is, Returns, Description, Input, Unit,
    Private, Constant, Functions,
    
    // Operators
    Plus, Minus, Multiply, Divide, Modulo, Power,
    Equal, NotEqual, Less, Greater, LessEqual, GreaterEqual,
    Assign, LeftParen, RightParen, LeftBracket, RightBracket,
    LeftBrace, RightBrace, Comma, Dot, Colon, Semicolon,
    
    // Identifiers and Special
    Identifier(String),
    Newline,
    Indent,
    Dedent,
    Eof,
}
```

**Error Handling**:
- Invalid character sequences
- Unterminated string literals
- Invalid number formats
- Unrecognized precision modifiers

### Stage 2: Parsing → AST (Untyped)

**Purpose**: Transform token stream into Abstract Syntax Tree exactly matching the AST Specification.

**Input**:
- Token stream from Stage 1
- Source location information

**Output**:
- Untyped AST with all nodes from specification
- SourceLocation preserved on all expression nodes
- Exact precedence and associativity as specified

**Components**:
- `src/parser/mod.rs` - Main parser interface
- `src/parser/expression_parser.rs` - Expression parsing with precedence
- `src/parser/statement_parser.rs` - Statement parsing
- `src/parser/class_parser.rs` - Class and function parsing
- `src/parser/apply_blocks.rs` - Apply block parsing
- `src/ast/mod.rs` - AST node definitions (already compliant)

**Key Requirements**:
- Parse all expressions from specification §5
- Implement exact operator precedence: Primary → Unary → Power → Multiplicative → Additive → Comparison → Equality → Logical AND → Logical OR → Assignment
- Support all statement types from specification §6
- Handle all apply blocks: TypeApplyBlock, FunctionApplyBlock, MethodApplyBlock, ConstantApplyBlock
- Parse functions blocks (required for all functions)
- Support class definitions with inheritance (`is` keyword)
- Handle pattern matching constructs
- Parse console input expressions: `input()`, `input.integer()`, `input.number()`, `input.yesNo()`
- Multi-line expression support with parentheses

**Parsing Strategy**:
- Recursive descent parser
- Precedence climbing for expressions
- Error recovery at statement boundaries
- Preserve all source information

**Critical Distinctions**:
```rust
// Must distinguish these at parse time based on case and context:
math.sqrt(9)      → NamespaceCall (lowercase namespace)
obj.method(9)     → MethodCall (object method)  
Class.method(9)   → StaticMethodCall (class method)
obj.property      → PropertyAccess
obj.property = x  → PropertyAssignment
```

### Stage 3: AST Validation & Desugaring → HIR

**Purpose**: Validate AST against specification constraints and transform into normalized HIR.

**Input**:
- Untyped AST from Stage 2

**Output**:
- High-level Intermediate Representation (HIR)
- DefId placeholders assigned
- Syntactic sugar normalized
- AST invariants validated

**Components**:
- `src/hir/mod.rs` - HIR definitions (new module)
- `src/hir/desugar.rs` - Desugaring transformations  
- `src/hir/validate.rs` - AST validation
- `src/hir/node_id.rs` - Node ID assignment

**Transformations**:
1. **Syntactic Sugar Normalization**:
   - `else if` chains → nested `if` statements
   - `for` loops → `while` loops (if applicable)
   - Default parameters → explicit handling
   - Method-style syntax preservation (don't desugar yet)

2. **AST Validation**:
   - `Assignment.target` is simple identifier (no dots)
   - `BaseCall` only inside constructor bodies
   - `ErrorVariable` only in error handling contexts
   - `MethodApplyBlock.method_chain` non-empty
   - List vs Matrix access arity checking

3. **Node ID Assignment**:
   - Assign unique DefId to each definition
   - Assign NodeId to expressions needing resolution
   - Build preliminary symbol table structure

**HIR Structure**:
```rust
// HIR mirrors AST but with normalized constructs
pub enum HirExpression {
    // Same variants as AST Expression
    // Plus normalized forms and DefId placeholders
    Literal(Value),
    Variable(String, NodeId),
    // ... other variants with NodeId where needed
}

pub struct HirFunction {
    id: DefId,
    name: String,
    parameters: Vec<HirParameter>,
    // ... rest matches Function but with IDs
}
```

### Stage 4: Name & Module Resolution

**Purpose**: Resolve all names to their definitions and build complete symbol tables.

**Input**:
- HIR with DefId placeholders

**Output**:
- HIR with resolved symbols
- Complete symbol table
- Scope graph for name lookup

**Components**:
- `src/semantic/symbol_table.rs` - Symbol table management
- `src/semantic/name_resolution.rs` - Name resolution logic
- `src/semantic/scope.rs` - Scope management
- `src/semantic/imports.rs` - Import resolution

**Resolution Process**:
1. **Symbol Collection**:
   - Collect all definitions (functions, classes, variables)
   - Build DefId → Definition mapping
   - Handle function blocks requirement

2. **Scope Building**:
   - Build scope hierarchy 
   - Handle private blocks
   - Class member visibility

3. **Name Resolution**:
   - Resolve variable references
   - Resolve function calls
   - Resolve method calls (prepare for type checking)
   - Handle namespace calls

4. **Import Processing**:
   - Process import statements
   - Inject standard library prelude
   - Resolve aliases

**Symbol Table Structure**:
```rust
pub struct SymbolTable {
    definitions: HashMap<DefId, Definition>,
    scopes: ScopeGraph,
    name_to_def: HashMap<(ScopeId, String), DefId>,
}

pub enum Definition {
    Function(HirFunction),
    Class(HirClass), 
    Variable(HirVariable),
    Parameter(HirParameter),
}
```

### Stage 5: Type Inference & Checking → TAST

**Purpose**: Perform type inference and checking to produce fully typed AST.

**Input**:
- HIR with resolved names
- Symbol table

**Output**:
- Typed AST (TAST) with TypeId on every expression
- Type constraints solved
- Method calls resolved to specific implementations

**Components**:
- `src/semantic/type_inference.rs` - Constraint-based type inference
- `src/semantic/type_checker.rs` - Type validation
- `src/semantic/constraints.rs` - Constraint solver
- `src/semantic/method_resolution.rs` - Method call resolution

**Type System (from specification)**:
```rust
pub enum Type {
    // Core types (§3.1)
    Boolean, Integer, Number, String, Void,
    
    // Precision modifiers (§3.2)  
    IntegerSized { bits: u8, unsigned: bool },
    NumberSized { bits: u8 },
    
    // Composite types (§3.3)
    List(Box<Type>),
    Matrix(Box<Type>),
    Pairs(Box<Type>, Box<Type>),
    
    // Generic type (§7.3)
    Any, // Universal generic type
    
    // Object types
    Object(String),
    Class { name: String, type_args: Vec<Type> },
    Function(Vec<Type>, Box<Type>),
    
    // Async types
    Future(Box<Type>),
}
```

**Inference Process**:
1. **Constraint Generation**:
   - Generate constraints from expressions
   - Handle method-style calls: `value.toString()`
   - Support generic functions with `any`

2. **Constraint Solving**:
   - Unification algorithm
   - Handle precision modifiers
   - Resolve method calls to specific implementations

3. **Type Validation**:
   - Check all expressions are well-typed
   - Validate class inheritance
   - Check function return types

### Stage 6: Lower to MIR (Optional but Recommended)

**Purpose**: Transform high-level constructs into explicit mid-level representation.

**Input**:
- TAST from Stage 5

**Output**:
- Mid-level IR with explicit control flow
- All method calls resolved to direct function IDs
- Temporaries inserted where needed

**Components**:
- `src/ir/mir.rs` - MIR definitions
- `src/ir/lower.rs` - TAST → MIR lowering
- `src/ir/validate.rs` - MIR validation

**MIR Transformations**:
1. **Explicit Control Flow**:
   - Lower complex expressions to temporaries
   - Make all control flow explicit
   - Remove nested expressions

2. **Call Resolution**:
   - All method calls → direct function calls
   - Resolve overloaded operators
   - Handle dynamic dispatch if needed

3. **Memory Layout**:
   - Explicit memory operations
   - String pool management
   - Object layout decisions

### Stage 7: Code Generation → WASM

**Purpose**: Generate WebAssembly binary from IR.

**Input**:
- MIR or TAST (depending on Stage 6 usage)

**Output**:
- WebAssembly binary (.wasm)
- Optional WAT text format for debugging

**Components**:
- `src/codegen/mod.rs` - Main codegen interface
- `src/codegen/wasm_module_builder.rs` - WASM module construction
- `src/codegen/expression_generator.rs` - Expression compilation
- `src/codegen/instruction_generator.rs` - Instruction generation
- `src/codegen/stdlib_generator.rs` - Standard library integration

**Code Generation Strategy**:
1. **Module Structure**:
   - Function definitions
   - Memory layout
   - Import declarations
   - Export declarations

2. **Expression Compilation**:
   - Stack-based evaluation
   - Type-specific instructions
   - Method call compilation

3. **Runtime Integration**:
   - Host function bindings
   - Memory management
   - String handling

## 3. Interface Specifications

### Stage Interfaces

Each stage has a well-defined interface:

```rust
pub trait CompilerStage<Input, Output> {
    type Error;
    
    fn process(&self, input: Input) -> Result<Output, Self::Error>;
    fn stage_name(&self) -> &'static str;
}

// Example implementation
impl CompilerStage<TokenStream, Ast> for Parser {
    type Error = ParseError;
    
    fn process(&self, tokens: TokenStream) -> Result<Ast, ParseError> {
        // Parse tokens to AST
    }
    
    fn stage_name(&self) -> &'static str { "Parser" }
}
```

### Error Propagation

```rust
pub enum CompilerError {
    LexError(LexError),
    ParseError(ParseError),
    ValidationError(ValidationError),
    NameResolutionError(NameResolutionError),
    TypeError(TypeError),
    CodeGenError(CodeGenError),
}

// Each error includes source location
pub struct ParseError {
    pub message: String,
    pub location: SourceLocation,
    pub suggestions: Vec<String>,
}
```

## 4. Implementation Strategy

### Phase 1: Foundation (Weeks 1-2)

1. **Create Test Infrastructure**:
   - Golden tests for each stage
   - Specification compliance test suite
   - Performance benchmarks

2. **Implement Stages 1-2**:
   - Complete lexer rewrite
   - Parser update for full compliance
   - Validate AST matches specification exactly

### Phase 2: Semantic Analysis (Weeks 3-4)

1. **Implement Stages 3-5**:
   - HIR module creation
   - Name resolution enhancement
   - Type system implementation

### Phase 3: Code Generation (Week 5)

1. **Implement Stages 6-7**:
   - MIR lowering (optional)
   - Update WASM codegen for new pipeline

### Phase 4: Integration & QA (Week 6)

1. **Full Pipeline Integration**:
   - End-to-end testing
   - Performance optimization
   - Documentation completion

## 5. Testing & Validation

### Test Categories

1. **Unit Tests**: Each stage individually
2. **Integration Tests**: Stage combinations
3. **Golden Tests**: AST output comparison
4. **Specification Tests**: Full language feature coverage
5. **Performance Tests**: Compilation speed benchmarks

### Validation Criteria

1. **Specification Compliance**: 100% AST specification alignment
2. **Feature Coverage**: All language constructs supported
3. **Error Quality**: Clear error messages with source locations
4. **Performance**: Reasonable compilation speeds
5. **Maintainability**: Clear code organization

## 6. Migration Strategy

### Current → New Pipeline

1. **Parallel Implementation**: Build new pipeline alongside existing
2. **Feature Flag**: Allow switching between pipelines
3. **Gradual Migration**: Test one stage at a time
4. **Full Cutover**: Remove old pipeline when new is validated

### Backward Compatibility

Not required per specification authority note - if specification changes, update implementation to match.

## 7. Future Enhancements

### Ready for Incremental Compilation
- Use stable identifiers (DefPathHash)
- Cache intermediate results
- Track dependencies between stages

### Ready for Parallelization  
- Parallel parsing of multiple files
- Parallel type checking of independent items
- Parallel code generation

### Extensibility
- Plugin system for additional analyses
- Custom optimization passes
- Alternative backends

---

**Authority Note**: This pipeline architecture is designed to achieve 100% compliance with dev_docs/AST_Specification.md. Any deviations from the specification are implementation errors that must be corrected.