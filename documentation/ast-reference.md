# Clean Language AST Reference Documentation

This document provides a comprehensive reference for understanding and working with the Clean Language Abstract Syntax Tree (AST). This information is essential for Claude to effectively develop, debug, and extend the Clean Language compiler.

> 🔗 **Related Documentation**: [Parser Documentation](./parser.md) • [Semantic Analysis](./semantic-analysis.md) • [Language Specification](./Clean_Language_Specification.md) • [Development Guide](./development-guide.md)

## Overview

The Clean Language AST represents the hierarchical structure of Clean Language source code after parsing. It serves as the foundation for semantic analysis, type checking, and code generation. The AST is designed to be both comprehensive and efficient, supporting the language's modern features while maintaining simplicity.

## Core AST Architecture

### 1. AST Node Base Structure (`src/ast/mod.rs`)

All AST nodes implement common traits and contain essential metadata:

```rust
/// Base trait for all AST nodes
pub trait ASTNode {
    fn span(&self) -> Span;
    fn node_id(&self) -> NodeId;
    fn accept<V: ASTVisitor>(&self, visitor: &mut V) -> Result<(), CompilerError>;
}

/// Unique identifier for AST nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// Source code location information
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: Position,
    pub end: Position,
    pub file_id: FileId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub line: u32,
    pub column: u32,
    pub offset: u32,
}
```

**Key Design Principles:**
- **Unique Identification**: Every AST node has a unique NodeId for cross-referencing
- **Source Mapping**: Spans provide precise source location for error reporting
- **Visitor Pattern**: Standard traversal mechanism for analysis passes
- **Immutable Structure**: AST nodes are immutable after construction

### 2. Top-Level Program Structure

```rust
/// Root AST node representing a complete Clean Language program
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub node_id: NodeId,
    pub span: Span,
    pub declarations: Vec<Declaration>,
    pub start_function: Option<Function>,
}

/// All possible top-level declarations
#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    Function(Function),
    Class(Class),
    Data(DataType),
    Import(Import),
    Package(PackageDecl),
}
```

**Program Structure Notes:**
- **Declarations**: Top-level functions, classes, data types
- **Start Function**: Optional main entry point
- **Modular Design**: Supports future module/package systems

## Expression AST Nodes

### 1. Expression Hierarchy

```rust
/// All expression types in Clean Language
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    // Literals
    Literal(Literal),
    
    // Variable access
    Variable(Variable),
    
    // Binary operations
    Binary(BinaryExpression),
    
    // Unary operations
    Unary(UnaryExpression),
    
    // Function calls
    Call(CallExpression),
    
    // Method calls
    MethodCall(MethodCallExpression),
    
    // Field access
    FieldAccess(FieldAccessExpression),
    
    // Array indexing
    Index(IndexExpression),
    
    // List literals
    ListLiteral(ListLiteral),
    
    // Matrix literals
    MatrixLiteral(MatrixLiteral),
    
    // Conditional expressions
    Conditional(ConditionalExpression),
    
    // Lambda expressions
    Lambda(LambdaExpression),
    
    // Type casting
    Cast(CastExpression),
    
    // Async expressions
    Async(AsyncExpression),
    
    // Await expressions
    Await(AwaitExpression),
    
    // Error handling
    OnError(OnErrorExpression),
}
```

### 2. Literal Expressions

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    pub node_id: NodeId,
    pub span: Span,
    pub kind: LiteralKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralKind {
    Integer {
        value: i64,
        precision: Option<IntegerPrecision>,  // 8, 16, 32, 64
    },
    Number {
        value: f64,
        precision: Option<NumberPrecision>,   // 32, 64
    },
    String {
        value: String,
        raw: bool,  // Raw string literal r"..."
    },
    Boolean(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntegerPrecision {
    I8, I16, I32, I64
}

#[derive(Debug, Clone, PartialEq)]
pub enum NumberPrecision {
    F32, F64
}
```

**Literal Design Features:**
- **Precision Specification**: Support for `integer:64`, `number:32` syntax
- **Raw Strings**: Handle escape sequence differences
- **Null Handling**: Explicit null value support

### 3. Binary Operations

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpression {
    pub node_id: NodeId,
    pub span: Span,
    pub left: Box<Expression>,
    pub operator: BinaryOperator,
    pub right: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    // Arithmetic
    Add, Subtract, Multiply, Divide, Modulo, Power,
    
    // Comparison
    Equal, NotEqual, Less, LessEqual, Greater, GreaterEqual,
    
    // Logical
    And, Or,
    
    // Bitwise
    BitwiseAnd, BitwiseOr, BitwiseXor,
    LeftShift, RightShift,
    
    // Assignment
    Assign, AddAssign, SubtractAssign, MultiplyAssign, DivideAssign,
    
    // Type checking
    Is, IsNot,
    
    // String operations
    Contains, StartsWith, EndsWith,
}
```

**Binary Expression Features:**
- **Operator Precedence**: Handled during parsing, not in AST
- **Short-Circuit Evaluation**: Logical operators require special handling
- **Type-Specific Operators**: String operations and type checking

### 4. Function Calls

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct CallExpression {
    pub node_id: NodeId,
    pub span: Span,
    pub function: Box<Expression>,  // Can be variable or field access
    pub arguments: Vec<Argument>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodCallExpression {
    pub node_id: NodeId,
    pub span: Span,
    pub receiver: Box<Expression>,
    pub method: Identifier,
    pub arguments: Vec<Argument>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Argument {
    pub node_id: NodeId,
    pub span: Span,
    pub expression: Expression,
    pub name: Option<Identifier>,  // Named arguments
}
```

**Call Expression Features:**
- **First-Class Functions**: Function expression can be any expression
- **Method Calls**: Distinct handling for object methods
- **Named Arguments**: Support for future named parameter syntax

### 5. Collection Literals

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ListLiteral {
    pub node_id: NodeId,
    pub span: Span,
    pub elements: Vec<Expression>,
    pub type_annotation: Option<TypeAnnotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatrixLiteral {
    pub node_id: NodeId,
    pub span: Span,
    pub rows: Vec<Vec<Expression>>,
    pub type_annotation: Option<TypeAnnotation>,
}
```

**Collection Features:**
- **Type Inference**: Optional type annotations for explicit typing
- **Nested Structure**: Matrix literals support 2D structure
- **Empty Collections**: Support for `[]` and `[[]]` literals

## Statement AST Nodes

### 1. Statement Types

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Expression(ExpressionStatement),
    VariableDeclaration(VariableDeclaration),
    Assignment(AssignmentStatement),
    Block(BlockStatement),
    If(IfStatement),
    While(WhileStatement),
    For(ForStatement),
    Iterate(IterateStatement),
    Return(ReturnStatement),
    Break(BreakStatement),
    Continue(ContinueStatement),
    Try(TryStatement),
}
```

### 2. Variable Declarations

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct VariableDeclaration {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub type_annotation: Option<TypeAnnotation>,
    pub initializer: Option<Expression>,
    pub is_mutable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssignmentStatement {
    pub node_id: NodeId,
    pub span: Span,
    pub target: Box<Expression>,  // Variable, field access, or index
    pub value: Box<Expression>,
}
```

**Variable Declaration Features:**
- **Type Inference**: Optional type annotations
- **Mutability**: Explicit mutable variable support
- **Assignment Targets**: Support for complex left-hand sides

### 3. Control Flow Statements

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct IfStatement {
    pub node_id: NodeId,
    pub span: Span,
    pub condition: Box<Expression>,
    pub then_block: Box<Statement>,
    pub else_block: Option<Box<Statement>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhileStatement {
    pub node_id: NodeId,
    pub span: Span,
    pub condition: Box<Expression>,
    pub body: Box<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForStatement {
    pub node_id: NodeId,
    pub span: Span,
    pub variable: Identifier,
    pub start: Box<Expression>,
    pub end: Box<Expression>,
    pub step: Option<Box<Expression>>,
    pub body: Box<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IterateStatement {
    pub node_id: NodeId,
    pub span: Span,
    pub variable: Identifier,
    pub iterable: Box<Expression>,
    pub body: Box<Statement>,
}
```

**Control Flow Features:**
- **Flexible Conditions**: Any expression type for conditions
- **Loop Variables**: Scoped iteration variables
- **Optional Step**: For loops support optional step expressions

### 4. Error Handling Statements

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct TryStatement {
    pub node_id: NodeId,
    pub span: Span,
    pub body: Box<Statement>,
    pub error_variable: Option<Identifier>,
    pub error_handler: Box<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnErrorExpression {
    pub node_id: NodeId,
    pub span: Span,
    pub expression: Box<Expression>,
    pub default_value: Box<Expression>,
}
```

**Error Handling Features:**
- **Expression-Level**: `onError` for expression-level error handling
- **Statement-Level**: `try` blocks for complex error handling
- **Error Variables**: Optional access to error information

## Declaration AST Nodes

### 1. Function Declarations

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,
    pub body: Box<Statement>,
    pub is_async: bool,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub type_annotation: TypeAnnotation,
    pub default_value: Option<Expression>,
}
```

**Function Features:**
- **Async Support**: First-class async function support
- **Default Parameters**: Optional parameter defaults
- **Type Annotations**: Required for parameters, optional for return

### 2. Class Declarations

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub parent: Option<Identifier>,
    pub fields: Vec<ClassField>,
    pub methods: Vec<Method>,
    pub constructor: Option<Constructor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassField {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub type_annotation: TypeAnnotation,
    pub default_value: Option<Expression>,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,
    pub body: Box<Statement>,
    pub is_static: bool,
    pub is_async: bool,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Constructor {
    pub node_id: NodeId,
    pub span: Span,
    pub parameters: Vec<Parameter>,
    pub body: Box<Statement>,
    pub parent_call: Option<ParentCall>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParentCall {
    pub node_id: NodeId,
    pub span: Span,
    pub arguments: Vec<Expression>,
}
```

**Class Features:**
- **Inheritance**: Single inheritance with parent classes
- **Static Methods**: Class-level methods
- **Constructor Chaining**: Parent constructor calls with `base()`
- **Visibility Control**: Public/private field and method access

### 3. Data Type Declarations

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct DataType {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub fields: Vec<DataField>,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataField {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub type_annotation: TypeAnnotation,
    pub default_value: Option<Expression>,
}
```

**Data Type Features:**
- **Simple Structure**: Focused on data modeling
- **Method Support**: Data types can have associated methods
- **Default Values**: Field-level defaults

## Type System AST Nodes

### 1. Type Annotations

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation {
    Named(NamedType),
    Generic(GenericType),
    List(ListType),
    Matrix(MatrixType),
    Function(FunctionType),
    Optional(OptionalType),
    Union(UnionType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NamedType {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub precision: Option<TypePrecision>,  // integer:64, number:32
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericType {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Identifier,
    pub type_arguments: Vec<TypeAnnotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListType {
    pub node_id: NodeId,
    pub span: Span,
    pub element_type: Box<TypeAnnotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatrixType {
    pub node_id: NodeId,
    pub span: Span,
    pub element_type: Box<TypeAnnotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionType {
    pub node_id: NodeId,
    pub span: Span,
    pub parameter_types: Vec<TypeAnnotation>,
    pub return_type: Box<TypeAnnotation>,
    pub is_async: bool,
}
```

**Type System Features:**
- **Generic Types**: Support for parameterized types
- **Precision Modifiers**: Explicit precision for numeric types
- **Function Types**: First-class function type support
- **Collection Types**: Specialized list and matrix types

### 2. Common Type Utilities

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TypePrecision {
    Integer(IntegerPrecision),
    Number(NumberPrecision),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Visibility {
    Public,
    Private,
    Protected,  // For future use
}

#[derive(Debug, Clone, PartialEq)]
pub struct Identifier {
    pub node_id: NodeId,
    pub span: Span,
    pub name: String,
}
```

## AST Construction Patterns

### 1. Builder Pattern for Complex Nodes

```rust
impl Function {
    pub fn builder(name: &str) -> FunctionBuilder {
        FunctionBuilder::new(name)
    }
}

pub struct FunctionBuilder {
    name: String,
    parameters: Vec<Parameter>,
    return_type: Option<TypeAnnotation>,
    body: Option<Statement>,
    is_async: bool,
    visibility: Visibility,
}

impl FunctionBuilder {
    pub fn parameter(mut self, name: &str, type_annotation: TypeAnnotation) -> Self {
        self.parameters.push(Parameter {
            node_id: NodeId::new(),
            span: Span::dummy(),
            name: Identifier::new(name),
            type_annotation,
            default_value: None,
        });
        self
    }
    
    pub fn async_fn(mut self) -> Self {
        self.is_async = true;
        self
    }
    
    pub fn body(mut self, body: Statement) -> Self {
        self.body = Some(body);
        self
    }
    
    pub fn build(self) -> Result<Function, ASTError> {
        Ok(Function {
            node_id: NodeId::new(),
            span: Span::dummy(),
            name: Identifier::new(&self.name),
            parameters: self.parameters,
            return_type: self.return_type,
            body: Box::new(self.body.ok_or(ASTError::MissingBody)?),
            is_async: self.is_async,
            visibility: self.visibility,
        })
    }
}
```

### 2. AST Visitor Pattern

```rust
pub trait ASTVisitor {
    type Result;
    
    fn visit_program(&mut self, node: &Program) -> Self::Result;
    fn visit_expression(&mut self, node: &Expression) -> Self::Result;
    fn visit_statement(&mut self, node: &Statement) -> Self::Result;
    fn visit_declaration(&mut self, node: &Declaration) -> Self::Result;
    
    // Specific node visitors
    fn visit_binary_expression(&mut self, node: &BinaryExpression) -> Self::Result {
        self.visit_expression(&node.left)?;
        self.visit_expression(&node.right)
    }
    
    fn visit_function(&mut self, node: &Function) -> Self::Result {
        for param in &node.parameters {
            self.visit_parameter(param)?;
        }
        self.visit_statement(&node.body)
    }
}

// Example implementation for symbol collection
pub struct SymbolCollector {
    symbols: HashMap<String, NodeId>,
}

impl ASTVisitor for SymbolCollector {
    type Result = Result<(), CompilerError>;
    
    fn visit_function(&mut self, node: &Function) -> Self::Result {
        self.symbols.insert(node.name.name.clone(), node.node_id);
        Ok(())
    }
    
    fn visit_variable_declaration(&mut self, node: &VariableDeclaration) -> Self::Result {
        self.symbols.insert(node.name.name.clone(), node.node_id);
        Ok(())
    }
}
```

## AST Transformation and Utilities

### 1. AST Transformations

```rust
pub trait ASTTransform {
    fn transform_expression(&mut self, expr: Expression) -> Result<Expression, CompilerError>;
    fn transform_statement(&mut self, stmt: Statement) -> Result<Statement, CompilerError>;
    
    // Default implementations for common patterns
    fn transform_binary_expression(&mut self, mut expr: BinaryExpression) -> Result<BinaryExpression, CompilerError> {
        expr.left = Box::new(self.transform_expression(*expr.left)?);
        expr.right = Box::new(self.transform_expression(*expr.right)?);
        Ok(expr)
    }
}

// Example: Constant folding transformation
pub struct ConstantFolder;

impl ASTTransform for ConstantFolder {
    fn transform_expression(&mut self, expr: Expression) -> Result<Expression, CompilerError> {
        match expr {
            Expression::Binary(bin_expr) => {
                let transformed = self.transform_binary_expression(bin_expr)?;
                
                // Check if both sides are literals
                if let (Expression::Literal(left), Expression::Literal(right)) = 
                    (&*transformed.left, &*transformed.right) {
                    if let Some(result) = self.fold_constants(left, &transformed.operator, right) {
                        return Ok(Expression::Literal(result));
                    }
                }
                
                Ok(Expression::Binary(transformed))
            },
            _ => Ok(expr)
        }
    }
}
```

### 2. AST Validation

```rust
pub struct ASTValidator {
    errors: Vec<CompilerError>,
}

impl ASTValidator {
    pub fn validate(&mut self, program: &Program) -> Result<(), Vec<CompilerError>> {
        self.validate_program(program);
        
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }
    
    fn validate_function(&mut self, func: &Function) {
        // Check for duplicate parameter names
        let mut param_names = HashSet::new();
        for param in &func.parameters {
            if !param_names.insert(&param.name.name) {
                self.errors.push(CompilerError::DuplicateParameter {
                    name: param.name.name.clone(),
                    span: param.span.clone(),
                });
            }
        }
        
        // Validate async consistency
        if func.is_async {
            self.validate_async_function_body(&func.body);
        }
    }
}
```

## AST Serialization and Debugging

### 1. Debug Output

```rust
impl std::fmt::Display for Expression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Expression::Literal(lit) => write!(f, "{}", lit),
            Expression::Variable(var) => write!(f, "{}", var.name),
            Expression::Binary(bin) => write!(f, "({} {} {})", bin.left, bin.operator, bin.right),
            Expression::Call(call) => {
                write!(f, "{}(", call.function)?;
                for (i, arg) in call.arguments.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", arg.expression)?;
                }
                write!(f, ")")
            },
            _ => write!(f, "<{:?}>", self)
        }
    }
}
```

### 2. AST Metrics and Analysis

```rust
pub struct ASTMetrics {
    pub node_count: usize,
    pub max_depth: usize,
    pub function_count: usize,
    pub class_count: usize,
    pub complexity_score: f64,
}

impl ASTMetrics {
    pub fn analyze(program: &Program) -> Self {
        let mut analyzer = MetricsAnalyzer::new();
        analyzer.analyze_program(program);
        analyzer.metrics
    }
}

struct MetricsAnalyzer {
    metrics: ASTMetrics,
    current_depth: usize,
}

impl ASTVisitor for MetricsAnalyzer {
    type Result = ();
    
    fn visit_expression(&mut self, expr: &Expression) -> Self::Result {
        self.metrics.node_count += 1;
        self.current_depth += 1;
        self.metrics.max_depth = self.metrics.max_depth.max(self.current_depth);
        
        // Visit children
        match expr {
            Expression::Binary(bin) => {
                self.visit_expression(&bin.left);
                self.visit_expression(&bin.right);
            },
            // ... handle other expression types
            _ => {}
        }
        
        self.current_depth -= 1;
    }
}
```

## Integration with Compiler Pipeline

### 1. AST to HIR Lowering Interface

```rust
pub trait ASTLowering {
    fn lower_program(&mut self, program: Program) -> Result<HIRProgram, CompilerError>;
    fn lower_expression(&mut self, expr: Expression) -> Result<HIRExpression, CompilerError>;
    fn lower_statement(&mut self, stmt: Statement) -> Result<HIRStatement, CompilerError>;
}
```

### 2. Error Recovery Integration

```rust
pub struct ParseResult {
    pub ast: Option<Program>,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<CompilerWarning>,
}

impl ParseResult {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
    
    pub fn can_continue(&self) -> bool {
        self.ast.is_some() && self.errors.iter().all(|e| e.is_recoverable())
    }
}
```

## Best Practices for Claude

When working with the Clean Language AST:

1. **Node ID Management**: Always assign unique NodeIds to new nodes
2. **Span Tracking**: Preserve accurate span information for error reporting  
3. **Memory Efficiency**: Use `Box<T>` for recursive structures and `Rc<T>` for shared references
4. **Visitor Pattern**: Use visitors for analysis and transformation passes
5. **Error Handling**: Collect multiple errors rather than failing on the first one
6. **Type Safety**: Leverage Rust's type system for AST invariants
7. **Testing**: Create AST nodes programmatically for unit tests
8. **Debugging**: Implement Display traits for readable AST debugging

This AST reference provides the foundation for understanding Clean Language's internal representation and enables effective compiler development and maintenance.