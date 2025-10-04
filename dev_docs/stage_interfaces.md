# Clean Language Compiler Stage Interfaces

Status: Technical specification for pipeline implementation
Authority: Based on pipeline_architecture.md and AST_Specification.md
Version: 1.0
Updated: December 2024

## 1. Overview

This document defines the exact interfaces, data structures, and error handling for each stage of the Clean Language compiler pipeline. These interfaces ensure clean separation of concerns and enable testing each stage independently.

## 2. Core Interface Trait

```rust
use std::fmt::Debug;
use crate::error::CompilerError;
use crate::ast::SourceLocation;

/// Core trait that all compilation stages must implement
pub trait CompilerStage<Input, Output> {
    type Error: Debug + Into<CompilerError>;
    
    /// Process input through this compilation stage
    fn process(&self, input: Input) -> Result<Output, Self::Error>;
    
    /// Get the name of this stage for debugging and error reporting
    fn stage_name(&self) -> &'static str;
    
    /// Get version info for this stage implementation
    fn version(&self) -> &'static str { "1.0" }
    
    /// Validate that input is in expected format (optional override)
    fn validate_input(&self, _input: &Input) -> Result<(), Self::Error> {
        Ok(())
    }
}
```

## 3. Stage-Specific Data Structures

### Stage 1: Lexical Analysis

```rust
/// Input to lexical analysis stage
#[derive(Debug, Clone)]
pub struct SourceCode {
    pub content: String,
    pub file_path: String,
    pub encoding: SourceEncoding,
}

#[derive(Debug, Clone)]
pub enum SourceEncoding {
    Utf8,
    // Future: other encodings if needed
}

/// Output from lexical analysis stage  
#[derive(Debug, Clone)]
pub struct TokenStream {
    pub tokens: Vec<Token>,
    pub source_map: SourceMap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub location: SourceLocation,
    pub text: String, // Original text for error reporting
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals (exact types from AST specification)
    IntegerLiteral(i64),
    NumberLiteral(f64),
    StringLiteral(String),
    BooleanLiteral(bool),
    
    // Precision literals (§3.2)
    Integer8Literal(i8),
    Integer8uLiteral(u8),
    Integer16Literal(i16),
    Integer16uLiteral(u16),
    Integer32Literal(i32),
    Integer64Literal(i64),
    Number32Literal(f32),
    Number64Literal(f64),
    
    // Keywords (all from Clean Language Specification)
    And, Class, Constructor, Else, Error, False, For, From,
    Function, If, Import, In, Iterate, Not, OnError, Or,
    Print, Println, Return, Start, Step, Test, Tests, This,
    To, True, While, Is, Returns, Description, Input, Unit,
    Private, Constant, Functions,
    
    // Operators and punctuation
    Plus, Minus, Multiply, Divide, Modulo, Power,
    Equal, NotEqual, Less, Greater, LessEqual, GreaterEqual,
    Assign, LeftParen, RightParen, LeftBracket, RightBracket,
    LeftBrace, RightBrace, Comma, Dot, Colon, Semicolon,
    Range, // .. for ranges
    
    // String interpolation
    InterpolationStart, // "Hello {
    InterpolationEnd,   // }"
    InterpolationMid,   // } text {
    
    // Special tokens
    Identifier(String),
    Newline,
    Indent(usize), // Number of tab levels
    Dedent(usize), // Number of tab levels reduced
    Eof,
}

/// Source map for tracking original positions
#[derive(Debug, Clone)]
pub struct SourceMap {
    pub file_path: String,
    pub line_starts: Vec<usize>, // Byte positions where each line starts
    pub total_bytes: usize,
}
```

### Stage 2: Parsing

```rust
/// Input to parsing stage (from Stage 1)
pub use TokenStream;

/// Output from parsing stage - exactly matches AST specification
pub use crate::ast::{Program, Statement, Expression, Function, Class};

#[derive(Debug, Clone)]
pub struct ParsedProgram {
    pub program: Program,
    pub source_map: SourceMap,
    pub parse_metadata: ParseMetadata,
}

#[derive(Debug, Clone)]
pub struct ParseMetadata {
    pub parsing_time: std::time::Duration,
    pub tokens_consumed: usize,
    pub recovery_points: Vec<SourceLocation>, // Where parser recovered from errors
}
```

### Stage 3: HIR (High-level IR)

```rust
/// Output from Stage 3: AST validation and desugaring
#[derive(Debug, Clone)]
pub struct HirProgram {
    pub items: Vec<HirItem>,
    pub node_map: NodeMap,
    pub validation_metadata: ValidationMetadata,
}

#[derive(Debug, Clone)]
pub enum HirItem {
    Function(HirFunction),
    Class(HirClass),
    Import(HirImport),
    Test(HirTest),
}

#[derive(Debug, Clone)]
pub struct HirFunction {
    pub id: DefId,
    pub name: String,
    pub parameters: Vec<HirParameter>,
    pub return_type: HirType,
    pub body: Vec<HirStatement>,
    pub visibility: Visibility,
    pub modifier: FunctionModifier,
    pub syntax: FunctionSyntax,
    pub location: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct HirClass {
    pub id: DefId,
    pub name: String,
    pub base_class: Option<DefId>,
    pub fields: Vec<HirField>,
    pub methods: Vec<HirFunction>,
    pub constructor: Option<HirConstructor>,
    pub location: SourceLocation,
}

// Node ID system for tracking relationships
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefId(pub u32);

#[derive(Debug, Clone)]
pub struct NodeMap {
    pub node_to_def: HashMap<NodeId, DefId>,
    pub def_to_node: HashMap<DefId, NodeId>,
    pub next_node_id: NodeId,
    pub next_def_id: DefId,
}
```

### Stage 4: Name Resolution

```rust
/// Output from Stage 4: HIR with resolved names
#[derive(Debug, Clone)]
pub struct ResolvedHirProgram {
    pub hir: HirProgram,
    pub symbol_table: SymbolTable,
    pub resolution_metadata: ResolutionMetadata,
}

#[derive(Debug, Clone)]
pub struct SymbolTable {
    pub definitions: HashMap<DefId, Definition>,
    pub scopes: ScopeGraph,
    pub name_resolution: HashMap<NodeId, DefId>,
    pub imports: HashMap<String, DefId>,
}

#[derive(Debug, Clone)]
pub enum Definition {
    Function {
        hir_function: HirFunction,
        scope: ScopeId,
    },
    Class {
        hir_class: HirClass,
        scope: ScopeId,
    },
    Variable {
        name: String,
        type_hint: Option<HirType>,
        location: SourceLocation,
        scope: ScopeId,
    },
    Parameter {
        name: String,
        type_: HirType,
        default_value: Option<HirExpression>,
        location: SourceLocation,
    },
}

#[derive(Debug, Clone)]
pub struct ScopeGraph {
    pub scopes: HashMap<ScopeId, Scope>,
    pub parent: HashMap<ScopeId, ScopeId>,
    pub children: HashMap<ScopeId, Vec<ScopeId>>,
    pub root_scope: ScopeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

#[derive(Debug, Clone)]
pub struct Scope {
    pub id: ScopeId,
    pub kind: ScopeKind,
    pub definitions: HashMap<String, DefId>,
    pub location: Option<SourceLocation>,
}

#[derive(Debug, Clone)]
pub enum ScopeKind {
    Global,
    Function(DefId),
    Class(DefId),
    Block,
    Private,
}
```

### Stage 5: Type Checking

```rust
/// Output from Stage 5: Fully typed AST
#[derive(Debug, Clone)]
pub struct TypedProgram {
    pub resolved_hir: ResolvedHirProgram,
    pub type_table: TypeTable,
    pub method_resolution: MethodResolutionTable,
    pub type_metadata: TypeMetadata,
}

#[derive(Debug, Clone)]
pub struct TypeTable {
    pub node_types: HashMap<NodeId, TypeId>,
    pub type_definitions: HashMap<TypeId, Type>,
    pub type_constraints: Vec<TypeConstraint>,
    pub next_type_id: TypeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

// Type system exactly matching AST specification
pub use crate::ast::Type;

#[derive(Debug, Clone)]
pub struct MethodResolutionTable {
    pub method_calls: HashMap<NodeId, ResolvedMethod>,
    pub static_calls: HashMap<NodeId, DefId>,
    pub namespace_calls: HashMap<NodeId, DefId>,
}

#[derive(Debug, Clone)]
pub struct ResolvedMethod {
    pub receiver_type: TypeId,
    pub method_def: DefId,
    pub resolved_args: Vec<TypeId>,
}
```

### Stage 6: MIR (Optional)

```rust
/// Mid-level IR (optional stage)
#[derive(Debug, Clone)]
pub struct MirProgram {
    pub functions: Vec<MirFunction>,
    pub classes: Vec<MirClass>,
    pub string_pool: StringPool,
    pub type_layout: TypeLayoutTable,
}

#[derive(Debug, Clone)]
pub struct MirFunction {
    pub id: DefId,
    pub name: String,
    pub parameters: Vec<MirLocal>,
    pub locals: Vec<MirLocal>,
    pub basic_blocks: Vec<MirBasicBlock>,
    pub return_type: TypeId,
}

#[derive(Debug, Clone)]
pub struct MirBasicBlock {
    pub id: BasicBlockId,
    pub statements: Vec<MirStatement>,
    pub terminator: MirTerminator,
}

#[derive(Debug, Clone)]
pub enum MirStatement {
    Assign { target: MirPlace, value: MirOperand },
    Call { target: MirPlace, function: DefId, args: Vec<MirOperand> },
    // Other MIR statements
}
```

### Stage 7: Code Generation

```rust
/// Output from final stage: WebAssembly module
#[derive(Debug, Clone)]
pub struct CompiledModule {
    pub wasm_binary: Vec<u8>,
    pub wat_text: Option<String>, // For debugging
    pub export_metadata: ExportMetadata,
    pub codegen_metadata: CodegenMetadata,
}

#[derive(Debug, Clone)]
pub struct ExportMetadata {
    pub exported_functions: Vec<String>,
    pub memory_layout: MemoryLayout,
    pub string_pool_info: StringPoolInfo,
}
```

## 4. Error Handling System

### Unified Error Type

```rust
/// Unified error type for all compilation stages
#[derive(Debug, thiserror::Error)]
pub enum CompilerError {
    #[error("Lexical analysis error: {0}")]
    LexError(#[from] LexError),
    
    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),
    
    #[error("Validation error: {0}")]
    ValidationError(#[from] ValidationError),
    
    #[error("Name resolution error: {0}")]
    NameResolutionError(#[from] NameResolutionError),
    
    #[error("Type error: {0}")]
    TypeError(#[from] TypeError),
    
    #[error("MIR error: {0}")]
    MirError(#[from] MirError),
    
    #[error("Code generation error: {0}")]
    CodegenError(#[from] CodegenError),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Base trait for all compiler errors
pub trait CompilerErrorTrait: std::error::Error {
    /// Get the primary source location for this error
    fn location(&self) -> Option<SourceLocation>;
    
    /// Get additional locations related to this error
    fn related_locations(&self) -> Vec<SourceLocation> { Vec::new() }
    
    /// Get suggestions for fixing this error
    fn suggestions(&self) -> Vec<String> { Vec::new() }
    
    /// Get error severity level
    fn severity(&self) -> ErrorSeverity { ErrorSeverity::Error }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
    Hint,
}
```

### Stage-Specific Errors

```rust
/// Lexical analysis errors
#[derive(Debug, thiserror::Error)]
pub enum LexError {
    #[error("Invalid character '{char}' at {location}")]
    InvalidCharacter { char: char, location: SourceLocation },
    
    #[error("Unterminated string literal at {location}")]
    UnterminatedString { location: SourceLocation },
    
    #[error("Invalid number format '{text}' at {location}")]
    InvalidNumber { text: String, location: SourceLocation },
    
    #[error("Invalid precision modifier '{modifier}' at {location}")]
    InvalidPrecisionModifier { modifier: String, location: SourceLocation },
    
    #[error("Unexpected end of file")]
    UnexpectedEof,
}

/// Parse errors
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Unexpected token {found}, expected {expected} at {location}")]
    UnexpectedToken {
        found: String,
        expected: String,
        location: SourceLocation,
    },
    
    #[error("Missing closing parenthesis for multi-line expression at {location}")]
    UnclosedParenthesis { location: SourceLocation },
    
    #[error("Invalid operator precedence at {location}")]
    InvalidPrecedence { location: SourceLocation },
    
    #[error("Invalid apply block syntax at {location}")]
    InvalidApplyBlock { location: SourceLocation },
}

/// Type errors
#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[error("Type mismatch: expected {expected}, found {found} at {location}")]
    TypeMismatch {
        expected: String,
        found: String,
        location: SourceLocation,
    },
    
    #[error("Unknown method '{method}' for type '{type_name}' at {location}")]
    UnknownMethod {
        method: String,
        type_name: String,
        location: SourceLocation,
    },
    
    #[error("Invalid precision modifier for type '{type_name}' at {location}")]
    InvalidPrecision {
        type_name: String,
        location: SourceLocation,
    },
}
```

## 5. Stage Implementation Examples

### Stage Interface Implementation

```rust
use crate::lexer::Lexer;

impl CompilerStage<SourceCode, TokenStream> for Lexer {
    type Error = LexError;
    
    fn process(&self, input: SourceCode) -> Result<TokenStream, LexError> {
        // Validate input
        self.validate_input(&input)?;
        
        // Perform lexical analysis
        let tokens = self.tokenize(&input.content, &input.file_path)?;
        let source_map = self.build_source_map(&input);
        
        Ok(TokenStream { tokens, source_map })
    }
    
    fn stage_name(&self) -> &'static str {
        "Lexical Analysis"
    }
    
    fn validate_input(&self, input: &SourceCode) -> Result<(), LexError> {
        // Ensure valid UTF-8
        if !input.content.is_ascii() && input.encoding != SourceEncoding::Utf8 {
            return Err(LexError::InvalidCharacter {
                char: '�',
                location: SourceLocation::new(1, 1, &input.file_path),
            });
        }
        Ok(())
    }
}
```

## 6. Pipeline Orchestration

```rust
/// Main pipeline orchestrator
pub struct CompilerPipeline {
    pub lexer: Box<dyn CompilerStage<SourceCode, TokenStream, Error = LexError>>,
    pub parser: Box<dyn CompilerStage<TokenStream, ParsedProgram, Error = ParseError>>,
    pub hir_builder: Box<dyn CompilerStage<ParsedProgram, HirProgram, Error = ValidationError>>,
    pub name_resolver: Box<dyn CompilerStage<HirProgram, ResolvedHirProgram, Error = NameResolutionError>>,
    pub type_checker: Box<dyn CompilerStage<ResolvedHirProgram, TypedProgram, Error = TypeError>>,
    pub mir_lowerer: Option<Box<dyn CompilerStage<TypedProgram, MirProgram, Error = MirError>>>,
    pub codegen: Box<dyn CompilerStage<TypedProgram, CompiledModule, Error = CodegenError>>,
}

impl CompilerPipeline {
    /// Run the complete compilation pipeline
    pub fn compile(&self, source: SourceCode) -> Result<CompiledModule, CompilerError> {
        // Stage 1: Lexical Analysis
        let tokens = self.lexer.process(source)?;
        
        // Stage 2: Parsing
        let parsed = self.parser.process(tokens)?;
        
        // Stage 3: HIR Building
        let hir = self.hir_builder.process(parsed)?;
        
        // Stage 4: Name Resolution
        let resolved = self.name_resolver.process(hir)?;
        
        // Stage 5: Type Checking
        let typed = self.type_checker.process(resolved)?;
        
        // Stage 6: MIR Lowering (optional)
        let lowered = if let Some(mir_lowerer) = &self.mir_lowerer {
            let mir = mir_lowerer.process(typed)?;
            // Convert back to typed program for codegen
            typed // TODO: Implement MIR → TypedProgram conversion
        } else {
            typed
        };
        
        // Stage 7: Code Generation
        let compiled = self.codegen.process(lowered)?;
        
        Ok(compiled)
    }
}
```

## 7. Testing Infrastructure

```rust
/// Test utilities for stage validation
pub mod testing {
    use super::*;
    
    /// Test a stage with known good input/output pairs
    pub fn test_stage_golden<S, I, O>(
        stage: &S,
        test_cases: &[(I, O)],
    ) -> Result<(), S::Error>
    where
        S: CompilerStage<I, O>,
        I: Clone + Debug + PartialEq,
        O: Debug + PartialEq,
        S::Error: Debug,
    {
        for (input, expected_output) in test_cases {
            let actual_output = stage.process(input.clone())?;
            assert_eq!(
                actual_output, *expected_output,
                "Stage {} produced incorrect output for input: {:?}",
                stage.stage_name(), input
            );
        }
        Ok(())
    }
    
    /// Benchmark stage performance
    pub fn benchmark_stage<S, I, O>(
        stage: &S,
        inputs: &[I],
        iterations: usize,
    ) -> std::time::Duration
    where
        S: CompilerStage<I, O>,
        I: Clone,
    {
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            for input in inputs {
                let _ = stage.process(input.clone());
            }
        }
        start.elapsed() / (iterations * inputs.len()) as u32
    }
}
```

---

**Authority Note**: These interfaces are designed to ensure 100% compliance with the AST Specification while providing clean separation of concerns and comprehensive error handling. All implementations must adhere to these interfaces exactly.