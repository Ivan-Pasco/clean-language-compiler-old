//! High-level Intermediate Representation (HIR) for Clean Language
//!
//! HIR is a desugared, validated intermediate representation that:
//! - Removes syntactic sugar and normalizes constructs
//! - Validates semantic consistency (but not types - that's Stage 5)
//! - Resolves implicit constructs to explicit forms
//! - Maintains source location information for error reporting
//! - Provides a clean interface for later stages

use crate::ast::{SourceLocation, Value};
use crate::error::CompilerError;
// HashMap import removed - not needed in HIR definitions

pub mod hir_builder;
pub mod validation;

#[cfg(test)]
pub mod tests;

/// HIR Program - the root of the intermediate representation
#[derive(Debug, Clone)]
pub struct HirProgram {
    pub functions: Vec<HirFunction>,
    pub classes: Vec<HirClass>,
    pub start_function: Option<HirFunction>,
    pub imports: Vec<HirImport>,
    pub tests: Vec<HirTest>,
    /// State block containing persistent state declarations
    pub state: Option<HirStateBlock>,
    /// Watch blocks for reactive state observers
    pub watch_blocks: Vec<HirWatchBlock>,
    /// External functions (WASM imports from host)
    pub externals: Vec<HirExternalFunction>,
    pub location: SourceLocation,
}

/// HIR External Function - a function provided by the WASM host (imported)
/// These generate WASM import entries during code generation
#[derive(Debug, Clone)]
pub struct HirExternalFunction {
    /// Function name (e.g., "_req_body_field", "_http_respond")
    pub name: String,
    /// Function parameters with types
    pub parameters: Vec<HirParameter>,
    /// Return type (HirType::Void for functions that don't return)
    pub return_type: HirType,
    /// WASM import module name (defaults to "env")
    pub module: String,
    /// Source location for error reporting
    pub location: SourceLocation,
}

/// HIR Function - normalized function representation
#[derive(Debug, Clone)]
pub struct HirFunction {
    pub name: String,
    pub parameters: Vec<HirParameter>,
    pub return_type: Option<HirType>,
    pub body: HirBlock,
    pub is_start: bool,
    pub location: SourceLocation,
}

/// HIR Parameter
#[derive(Debug, Clone)]
pub struct HirParameter {
    pub name: String,
    pub param_type: HirType,
    pub default_value: Option<HirExpression>,
    pub location: SourceLocation,
}

/// HIR Type - simplified type representation
#[derive(Debug, Clone, PartialEq)]
pub enum HirType {
    /// Primitive types
    Integer,
    Number,
    String,
    Boolean,
    Void,
    // BOOK: null-support - Null type for representing absence of value
    Null,

    /// Precision types (from lexer precision modifiers)
    Integer8,
    Integer8u,
    Integer16,
    Integer16u,
    Integer32,
    Integer32u,
    Integer64,
    Integer64u,
    Number32,
    Number64,

    /// Generic types
    List(Box<HirType>),
    Matrix(Box<HirType>),
    Pairs(Box<HirType>, Box<HirType>),

    /// User-defined types (resolved in Stage 4)
    Named {
        name: String,
        location: SourceLocation,
    },

    /// Type inference placeholder (resolved in Stage 5)
    Inferred {
        id: usize,
        location: SourceLocation,
    },

    /// Dynamic type for JSON values and runtime-typed data
    /// Supports bracket notation with string keys (object access) or integer indices (array access)
    Any,
}

/// HIR Class - normalized class representation
#[derive(Debug, Clone)]
pub struct HirClass {
    pub name: String,
    pub parent: Option<String>,
    pub fields: Vec<HirField>,
    pub constructor: Option<HirConstructor>,
    pub methods: Vec<HirMethod>,
    pub location: SourceLocation,
}

/// HIR Field
#[derive(Debug, Clone)]
pub struct HirField {
    pub name: String,
    pub field_type: HirType,
    pub initializer: Option<HirExpression>,
    pub location: SourceLocation,
}

/// HIR Constructor
#[derive(Debug, Clone)]
pub struct HirConstructor {
    pub parameters: Vec<HirParameter>,
    pub body: HirBlock,
    pub location: SourceLocation,
}

/// HIR Method
#[derive(Debug, Clone)]
pub struct HirMethod {
    pub name: String,
    pub parameters: Vec<HirParameter>,
    pub return_type: HirType,
    pub body: HirBlock,
    pub location: SourceLocation,
}

/// HIR Import
#[derive(Debug, Clone)]
pub struct HirImport {
    pub module_name: String,
    pub items: Option<Vec<String>>, // None = import all, Some = specific items
    pub location: SourceLocation,
}

/// HIR Test
#[derive(Debug, Clone)]
pub struct HirTest {
    pub name: String,
    pub description: Option<String>,
    pub body: HirBlock,
    pub location: SourceLocation,
}

/// HIR State Scope - determines state variable lifetime
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HirStateScope {
    /// App-level state: persists for the application lifetime
    App,
    /// Screen-level state: persists for the screen/component lifetime
    Screen,
}

/// HIR State Block - persistent state declarations
#[derive(Debug, Clone)]
pub struct HirStateBlock {
    pub declarations: Vec<HirStateDeclaration>,
    pub computed: Vec<HirComputedDeclaration>,
    pub scope: HirStateScope,
    pub location: SourceLocation,
}

/// HIR State Declaration - individual state variable
#[derive(Debug, Clone)]
pub struct HirStateDeclaration {
    pub name: String,
    pub state_type: HirType,
    pub initializer: HirExpression,
    pub guard: Option<HirGuardClause>,
    pub location: SourceLocation,
}

/// HIR Guard Clause - validation constraint for state mutations
#[derive(Debug, Clone)]
pub struct HirGuardClause {
    pub condition: HirExpression,
    pub error_message: String,
    pub location: SourceLocation,
}

/// HIR Computed Declaration - derived state value
#[derive(Debug, Clone)]
pub struct HirComputedDeclaration {
    pub name: String,
    pub computed_type: HirType,
    pub body: HirBlock, // Body that computes the value (must end with return)
    pub location: SourceLocation,
}

/// HIR Watch Block - reactive observer for state changes
#[derive(Debug, Clone)]
pub struct HirWatchBlock {
    pub targets: Vec<String>,
    pub body: HirBlock,
    pub location: SourceLocation,
}

/// HIR Block - sequence of statements
#[derive(Debug, Clone)]
pub struct HirBlock {
    pub statements: Vec<HirStatement>,
    pub location: SourceLocation,
}

/// HIR Statement - all statement forms, desugared
#[derive(Debug, Clone)]
pub enum HirStatement {
    /// Variable declaration with optional initialization
    VariableDeclaration {
        name: String,
        var_type: HirType,
        initializer: Option<HirExpression>,
        is_mutable: bool, // false for constants, true for variables
        location: SourceLocation,
    },

    /// Assignment (separate from declaration)
    Assignment {
        target: HirLValue,
        value: HirExpression,
        location: SourceLocation,
    },

    /// Expression statement (function calls, etc.)
    Expression {
        expression: HirExpression,
        location: SourceLocation,
    },

    /// Return statement
    Return {
        value: Option<HirExpression>,
        location: SourceLocation,
    },

    /// Conditional statement
    If {
        condition: HirExpression,
        then_branch: HirBlock,
        else_branch: Option<HirBlock>,
        location: SourceLocation,
    },

    /// For loop - desugared to iterate with iterator
    For {
        variable: String,
        iterable: HirExpression,
        body: HirBlock,
        location: SourceLocation,
    },

    /// While loop
    While {
        condition: HirExpression,
        body: HirBlock,
        location: SourceLocation,
    },

    /// Break statement - exit innermost loop
    Break { location: SourceLocation },

    /// Continue statement - skip to next iteration of innermost loop
    Continue { location: SourceLocation },

    /// Print statement
    Print {
        expression: HirExpression,
        newline: bool,
        location: SourceLocation,
    },

    /// Later assignment (async variable declaration)
    LaterAssignment {
        variable: String,
        expression: HirExpression,
        location: SourceLocation,
    },
}

/// HIR Expression - all expression forms, normalized
#[derive(Debug, Clone)]
pub enum HirExpression {
    /// Literal values
    Literal {
        value: Value,
        location: SourceLocation,
    },

    /// Variable reference
    Variable {
        name: String,
        location: SourceLocation,
    },

    /// Binary operation
    BinaryOp {
        left: Box<HirExpression>,
        op: HirBinaryOp,
        right: Box<HirExpression>,
        location: SourceLocation,
    },

    /// Unary operation
    UnaryOp {
        op: HirUnaryOp,
        operand: Box<HirExpression>,
        location: SourceLocation,
    },

    /// Function call
    Call {
        function: String,
        arguments: Vec<HirExpression>,
        location: SourceLocation,
    },

    /// Method call - always explicit with receiver
    MethodCall {
        receiver: Box<HirExpression>,
        method: String,
        arguments: Vec<HirExpression>,
        location: SourceLocation,
    },

    /// Field access
    FieldAccess {
        object: Box<HirExpression>,
        field: String,
        location: SourceLocation,
    },

    /// Array/list indexing
    Index {
        array: Box<HirExpression>,
        index: Box<HirExpression>,
        location: SourceLocation,
    },

    /// Array literal
    Array {
        elements: Vec<HirExpression>,
        element_type: HirType, // Inferred or explicit
        location: SourceLocation,
    },

    /// Constructor call (explicit)
    Constructor {
        class_name: String,
        arguments: Vec<HirExpression>,
        location: SourceLocation,
    },

    /// Type cast (explicit type conversion)
    Cast {
        expression: Box<HirExpression>,
        target_type: HirType,
        location: SourceLocation,
    },

    /// Assignment expression (returns the assigned value)
    Assignment {
        target: HirLValue,
        value: Box<HirExpression>,
        location: SourceLocation,
    },

    /// Namespace function call (e.g., math.sin, string.length)
    NamespaceCall {
        namespace: String,
        function: String,
        arguments: Vec<HirExpression>,
        location: SourceLocation,
    },

    /// Static method call (e.g., Math.abs, compare.integer.greaterThan)
    StaticMethodCall {
        namespace: Vec<String>, // Empty for two-level, ["compare"] for three-level
        class_name: String,
        method: String,
        arguments: Vec<HirExpression>,
        location: SourceLocation,
    },

    /// Error handling expression (expression onError fallback)
    OnError {
        expression: Box<HirExpression>,
        fallback: Box<HirExpression>,
        location: SourceLocation,
    },

    /// Conditional expression (if condition then value else value)
    Conditional {
        condition: Box<HirExpression>,
        then_expr: Box<HirExpression>,
        else_expr: Box<HirExpression>,
        location: SourceLocation,
    },

    /// Base constructor call (in derived class constructors)
    BaseCall {
        arguments: Vec<HirExpression>,
        location: SourceLocation,
    },

    /// Range expression (start..end or start..=end)
    Range {
        start: Box<HirExpression>,
        end: Box<HirExpression>,
        step: Option<Box<HirExpression>>,
        inclusive: bool,
        location: SourceLocation,
    },
}

/// HIR L-value (left-hand side of assignment)
#[derive(Debug, Clone)]
pub enum HirLValue {
    Variable {
        name: String,
        location: SourceLocation,
    },
    FieldAccess {
        object: Box<HirExpression>,
        field: String,
        location: SourceLocation,
    },
    Index {
        array: Box<HirExpression>,
        index: Box<HirExpression>,
        location: SourceLocation,
    },
}

/// HIR Binary operators
#[derive(Debug, Clone, PartialEq)]
pub enum HirBinaryOp {
    // Arithmetic
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,

    // Comparison
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Is,    // Identity comparison (same as Equal for value types)
    IsNot, // Negated identity comparison

    // Logical
    And,
    Or,

    // BOOK: null-coalescing - Null coalescing operator
    // Usage: value default fallback (returns fallback if value is null)
    NullCoalesce,

    // String operations (desugared)
    StringConcat,
}

/// HIR Unary operators
#[derive(Debug, Clone, PartialEq)]
pub enum HirUnaryOp {
    Negate, // -x
    Not,    // not x
    // BOOK: required-operator - Postfix ! assertion for null check
    Required, // x! (asserts x is not null, fails at runtime if null)
}

impl HirExpression {
    /// Get the source location of this expression
    pub fn location(&self) -> &SourceLocation {
        match self {
            HirExpression::Literal { location, .. } => location,
            HirExpression::Variable { location, .. } => location,
            HirExpression::BinaryOp { location, .. } => location,
            HirExpression::UnaryOp { location, .. } => location,
            HirExpression::Call { location, .. } => location,
            HirExpression::MethodCall { location, .. } => location,
            HirExpression::FieldAccess { location, .. } => location,
            HirExpression::Index { location, .. } => location,
            HirExpression::Array { location, .. } => location,
            HirExpression::Constructor { location, .. } => location,
            HirExpression::Cast { location, .. } => location,
            HirExpression::Assignment { location, .. } => location,
            HirExpression::NamespaceCall { location, .. } => location,
            HirExpression::StaticMethodCall { location, .. } => location,
            HirExpression::OnError { location, .. } => location,
            HirExpression::Conditional { location, .. } => location,
            HirExpression::BaseCall { location, .. } => location,
            HirExpression::Range { location, .. } => location,
        }
    }
}

impl HirType {
    /// Check if this is a primitive type
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            HirType::Integer
                | HirType::Number
                | HirType::String
                | HirType::Boolean
                | HirType::Void
                | HirType::Integer8
                | HirType::Integer8u
                | HirType::Integer16
                | HirType::Integer16u
                | HirType::Integer32
                | HirType::Integer64
                | HirType::Number32
                | HirType::Number64
        )
    }

    /// Check if this is a numeric type
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            HirType::Integer
                | HirType::Number
                | HirType::Integer8
                | HirType::Integer8u
                | HirType::Integer16
                | HirType::Integer16u
                | HirType::Integer32
                | HirType::Integer64
                | HirType::Number32
                | HirType::Number64
        )
    }

    /// Get the default type for a literal value
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::Integer(_) => HirType::Integer,
            Value::Number(_) => HirType::Number,
            Value::String(_) => HirType::String,
            Value::Boolean(_) => HirType::Boolean,
            // BOOK: null-support - Null value maps to Null type
            Value::Null => HirType::Null,
            Value::Void => HirType::Void,
            Value::Integer8(_) => HirType::Integer8,
            Value::Integer8u(_) => HirType::Integer8u,
            Value::Integer16(_) => HirType::Integer16,
            Value::Integer16u(_) => HirType::Integer16u,
            Value::Integer32(_) => HirType::Integer32,
            Value::Integer64(_) => HirType::Integer64,
            Value::Number32(_) => HirType::Number32,
            Value::Number64(_) => HirType::Number64,
            Value::List(_) => HirType::List(Box::new(HirType::Inferred {
                id: 0,
                location: SourceLocation::default(),
            })),
            Value::Matrix(_) => HirType::Matrix(Box::new(HirType::Number)),
            Value::Pairs(_) => HirType::Pairs(
                Box::new(HirType::Inferred {
                    id: 0,
                    location: SourceLocation::default(),
                }),
                Box::new(HirType::Inferred {
                    id: 0,
                    location: SourceLocation::default(),
                }),
            ),
        }
    }
}

/// Validation results from HIR construction
#[derive(Debug, Clone)]
pub struct HirValidationResult {
    pub hir: HirProgram,
    pub warnings: Vec<CompilerError>,
    pub type_inference_count: usize,
}

/// HIR Construction errors
#[derive(Debug, Clone)]
pub enum HirError {
    /// Invalid syntax that passed parsing but is semantically incorrect
    InvalidConstruct {
        message: String,
        location: SourceLocation,
    },

    /// Missing required construct
    MissingConstruct {
        construct: String,
        location: SourceLocation,
    },

    /// Duplicate definition
    DuplicateDefinition {
        name: String,
        first_location: SourceLocation,
        second_location: SourceLocation,
    },

    /// Invalid type annotation
    InvalidType {
        type_name: String,
        location: SourceLocation,
    },

    /// Unsupported language feature
    UnsupportedFeature {
        feature: String,
        location: SourceLocation,
    },
}
