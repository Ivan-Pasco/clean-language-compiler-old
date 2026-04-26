//! Typed Abstract Syntax Tree (TAST) for Clean Language
//!
//! The TAST represents the program after type checking and inference.
//! All expressions have concrete types and all symbols are fully resolved.

use crate::ast::SourceLocation;
use crate::resolver::{GlobalSymbolTable, ScopeId, SymbolId};
use std::collections::HashMap;
use std::sync::Arc;

/// Type-checked program representation
#[derive(Debug, Clone)]
pub struct TastProgram {
    pub functions: Vec<TastFunction>,
    pub classes: Vec<TastClass>,
    pub start_function: Option<TastFunction>,
    pub imports: Vec<TastImport>,
    pub tests: Vec<TastTest>,
    pub state: Option<TastStateBlock>,
    /// Top-level watch blocks (reactive state observers)
    pub watch_blocks: Vec<TastWatchBlock>,
    pub type_env: HashMap<SymbolId, ConcreteType>,
    pub location: SourceLocation,
    /// Symbol table with all symbols (builtins + user-defined)
    /// Used by MIR builder for dynamic SymbolId resolution
    pub symbol_table: Arc<GlobalSymbolTable>,
    /// External functions (WASM imports from host)
    pub externals: Vec<TastExternalFunction>,
}

/// Type-checked external function (WASM import)
#[derive(Debug, Clone, PartialEq)]
pub struct TastExternalFunction {
    pub name: String,
    pub parameters: Vec<TastParameter>,
    pub return_type: ConcreteType,
    pub module: String,
    pub location: SourceLocation,
}

/// Type-checked function with full type information
#[derive(Debug, Clone, PartialEq)]
pub struct TastFunction {
    pub symbol_id: SymbolId,
    pub name: String,
    pub parameters: Vec<TastParameter>,
    pub return_type: ConcreteType,
    pub body: TastBlock,
    pub generic_params: Vec<TypeParameter>,
    pub constraints: Vec<TypeConstraint>,
    pub is_background: bool,
    pub is_static: bool,
    pub visibility: Visibility,
    pub location: SourceLocation,
}

/// Type-checked function parameter
#[derive(Debug, Clone, PartialEq)]
pub struct TastParameter {
    pub symbol_id: SymbolId,
    pub name: String,
    pub param_type: ConcreteType,
    pub default_value: Option<TastExpression>,
    pub is_variadic: bool,
    pub location: SourceLocation,
}

/// Type-checked class definition
#[derive(Debug, Clone, PartialEq)]
pub struct TastClass {
    pub symbol_id: SymbolId,
    pub name: String,
    pub fields: Vec<TastField>,
    pub methods: Vec<TastFunction>,
    pub constructors: Vec<TastFunction>,
    pub parent_class: Option<SymbolId>,
    pub interfaces: Vec<SymbolId>,
    pub generic_params: Vec<TypeParameter>,
    pub is_abstract: bool,
    pub visibility: Visibility,
    /// Type-checked invariant conditions from the `invariant:` block.
    /// Each expression has been verified to be of boolean type.
    pub invariants: Vec<TastExpression>,
    pub location: SourceLocation,
}

/// Type-checked class field
#[derive(Debug, Clone, PartialEq)]
pub struct TastField {
    pub symbol_id: SymbolId,
    pub name: String,
    pub field_type: ConcreteType,
    pub default_value: Option<TastExpression>,
    pub is_static: bool,
    pub is_readonly: bool,
    pub visibility: Visibility,
    pub location: SourceLocation,
}

/// Type-checked block of statements
#[derive(Debug, Clone, PartialEq)]
pub struct TastBlock {
    pub statements: Vec<TastStatement>,
    pub scope_id: ScopeId,
    pub return_type: ConcreteType,
    pub location: SourceLocation,
}

/// Type-checked statement
#[derive(Debug, Clone, PartialEq)]
pub enum TastStatement {
    Expression {
        expression: TastExpression,
        location: SourceLocation,
    },
    VariableDeclaration {
        symbol_id: SymbolId,
        name: String,
        var_type: ConcreteType,
        initializer: Option<TastExpression>,
        is_mutable: bool,
        location: SourceLocation,
    },
    Assignment {
        target: TastExpression,
        value: TastExpression,
        location: SourceLocation,
    },
    Return {
        value: Option<TastExpression>,
        return_type: ConcreteType,
        location: SourceLocation,
    },
    If {
        condition: TastExpression,
        then_block: TastBlock,
        else_block: Option<TastBlock>,
        result_type: ConcreteType,
        location: SourceLocation,
    },
    For {
        iterator: SymbolId,
        iterator_name: String, // Variable name for the iterator
        iterable: TastExpression,
        body: TastBlock,
        location: SourceLocation,
    },
    While {
        condition: TastExpression,
        body: TastBlock,
        location: SourceLocation,
    },
    Try {
        body: TastBlock,
        catch_clause: Option<TastCatchClause>,
        finally_clause: Option<TastBlock>,
        location: SourceLocation,
    },
    Throw {
        expression: TastExpression,
        location: SourceLocation,
    },
    Break {
        location: SourceLocation,
    },
    Continue {
        location: SourceLocation,
    },
    Print {
        expression: TastExpression,
        newline: bool,
        location: SourceLocation,
    },
    LaterAssignment {
        variable: String,
        symbol_id: SymbolId,
        expression: TastExpression,
        location: SourceLocation,
    },
    /// Require statement - contract precondition that must be true
    /// Traps at runtime if condition is false
    Require {
        condition: TastExpression,
        location: SourceLocation,
    },

    /// Ensure statement - contract postcondition on the return value.
    /// `result` in the condition refers to the function's return value.
    /// Debug builds emit a WASM trap if the condition is false; release builds strip it.
    Ensure {
        condition: TastExpression,
        location: SourceLocation,
    },
}

/// Type-checked expression with concrete type
#[derive(Debug, Clone, PartialEq)]
pub struct TastExpression {
    pub kind: TastExpressionKind,
    pub expr_type: ConcreteType,
    pub location: SourceLocation,
}

/// Type-checked expression kinds
#[derive(Debug, Clone, PartialEq)]
pub enum TastExpressionKind {
    Literal {
        value: TastLiteral,
    },
    Variable {
        symbol_id: SymbolId,
        name: String,
    },
    BinaryOperation {
        operator: BinaryOperator,
        left: Box<TastExpression>,
        right: Box<TastExpression>,
    },
    UnaryOperation {
        operator: UnaryOperator,
        operand: Box<TastExpression>,
    },
    FunctionCall {
        function: Box<TastExpression>,
        arguments: Vec<TastExpression>,
        type_args: Vec<ConcreteType>,
    },
    MethodCall {
        receiver: Box<TastExpression>,
        method_name: String,
        method_symbol: SymbolId,
        arguments: Vec<TastExpression>,
        type_args: Vec<ConcreteType>,
    },
    StaticMethodCall {
        class_name: String,
        method_name: String,
        method_symbol: SymbolId,
        arguments: Vec<TastExpression>,
        type_args: Vec<ConcreteType>,
    },
    PropertyAccess {
        object: Box<TastExpression>,
        property_name: String,
        property_symbol: SymbolId,
    },
    ArrayLiteral {
        elements: Vec<TastExpression>,
        element_type: ConcreteType,
    },
    ArrayAccess {
        array: Box<TastExpression>,
        index: Box<TastExpression>,
    },
    ObjectLiteral {
        fields: Vec<TastObjectField>,
    },
    Lambda {
        parameters: Vec<TastParameter>,
        body: TastBlock,
        return_type: ConcreteType,
        captures: Vec<SymbolId>,
    },
    Cast {
        expression: Box<TastExpression>,
        target_type: ConcreteType,
        is_safe: bool,
    },
    TypeCheck {
        expression: Box<TastExpression>,
        check_type: ConcreteType,
    },
    Await {
        expression: Box<TastExpression>,
    },
    AsyncBlock {
        body: TastBlock,
    },
    OnError {
        expression: Box<TastExpression>,
        fallback: Box<TastExpression>,
    },
    Conditional {
        condition: Box<TastExpression>,
        then_expr: Box<TastExpression>,
        else_expr: Box<TastExpression>,
    },
    BaseCall {
        parent_class_symbol_id: SymbolId,
        arguments: Vec<TastExpression>,
    },
    Range {
        start: Box<TastExpression>,
        end: Box<TastExpression>,
        step: Option<Box<TastExpression>>,
        inclusive: bool,
    },
}

/// Type-checked literal values
#[derive(Debug, Clone, PartialEq)]
pub enum TastLiteral {
    Integer(i64),
    Number(f64),
    String(String),
    Boolean(bool),
    Null,
    Undefined,
}

/// Type-checked object field
#[derive(Debug, Clone, PartialEq)]
pub struct TastObjectField {
    pub key: String,
    pub value: TastExpression,
    pub location: SourceLocation,
}

/// Type-checked catch clause
#[derive(Debug, Clone, PartialEq)]
pub struct TastCatchClause {
    pub exception_symbol: SymbolId,
    pub exception_type: ConcreteType,
    pub body: TastBlock,
    pub location: SourceLocation,
}

/// Type-checked import statement
#[derive(Debug, Clone, PartialEq)]
pub struct TastImport {
    pub module_name: String,
    pub imported_symbols: Vec<TastImportedSymbol>,
    pub alias: Option<String>,
    pub location: SourceLocation,
}

/// Type-checked imported symbol
#[derive(Debug, Clone, PartialEq)]
pub struct TastImportedSymbol {
    pub name: String,
    pub alias: Option<String>,
    pub symbol_id: SymbolId,
    pub symbol_type: ConcreteType,
}

/// Type-checked test definition
#[derive(Debug, Clone, PartialEq)]
pub struct TastTest {
    pub name: String,
    pub body: TastBlock,
    pub is_background: bool,
    pub location: SourceLocation,
}

/// Type-checked state block
#[derive(Debug, Clone, PartialEq)]
pub struct TastStateBlock {
    pub declarations: Vec<TastStateDeclaration>,
    /// Computed (derived) state declarations whose bodies have been type-checked
    pub computed: Vec<TastComputedDeclaration>,
    /// State invariants checked at operation boundaries (after start:, frame:, handlers)
    pub rules: Vec<TastExpression>,
    pub scope: TastStateScope,
    pub location: SourceLocation,
}

/// Type-checked computed state declaration.
///
/// A computed declaration derives its value by executing `body`, which must end
/// with a `return` statement whose type matches `computed_type`.
///
/// Runtime note: the runtime is responsible for tracking which mutable state
/// variables are read during body execution and invalidating the cached result
/// when any of those variables change.
#[derive(Debug, Clone, PartialEq)]
pub struct TastComputedDeclaration {
    /// Symbol ID under which the computed value is visible as a read-only variable
    pub symbol_id: crate::resolver::SymbolId,
    pub name: String,
    pub computed_type: ConcreteType,
    /// Fully type-checked body; must terminate with a `Return` statement
    pub body: TastBlock,
    pub location: SourceLocation,
}

/// Type-checked watch block.
///
/// Each target name has been resolved to a SymbolId at parse time; the body
/// has been type-checked for correctness. The body does not need a return
/// value — it is executed purely for its side effects.
///
/// Runtime note: the runtime is responsible for hooking into state mutations
/// and invoking watch block bodies whenever a watched variable is written.
#[derive(Debug, Clone, PartialEq)]
pub struct TastWatchBlock {
    /// Names of the watched state variables (for diagnostics / code generation)
    pub targets: Vec<String>,
    /// Resolved symbol IDs for each target (parallel to `targets`)
    pub target_symbol_ids: Vec<crate::resolver::SymbolId>,
    /// Fully type-checked handler body
    pub body: TastBlock,
    pub location: SourceLocation,
}

/// State scope level
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TastStateScope {
    App,
    Screen,
}

/// Type-checked state declaration
#[derive(Debug, Clone, PartialEq)]
pub struct TastStateDeclaration {
    pub symbol_id: SymbolId,
    pub name: String,
    pub state_type: ConcreteType,
    pub initializer: TastExpression,
    pub guard: Option<TastGuardClause>,
    pub location: SourceLocation,
}

/// Type-checked guard clause
#[derive(Debug, Clone, PartialEq)]
pub struct TastGuardClause {
    pub condition: TastExpression,
    pub value_symbol_id: crate::resolver::SymbolId, // Symbol ID for the 'value' binding
    pub error_message: String,
    pub location: SourceLocation,
}

/// Concrete type after type inference
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConcreteType {
    /// Built-in primitive types
    Integer,
    Number,
    String,
    Boolean,
    Null,
    Undefined,

    /// Precision integer types (bits: 8/16/32/64, unsigned: true/false)
    IntegerSized {
        bits: u8,
        unsigned: bool,
    },
    /// Precision number types (bits: 32/64)
    NumberSized {
        bits: u8,
    },

    /// Array type with element type
    Array(Box<ConcreteType>),

    /// Matrix type with element type (2D array)
    Matrix(Box<ConcreteType>),

    /// Pairs type with key and value types
    Pairs(Box<ConcreteType>, Box<ConcreteType>),

    /// Function type
    Function {
        parameters: Vec<ConcreteType>,
        return_type: Box<ConcreteType>,
        is_background: bool,
    },

    /// Class type
    Class {
        symbol_id: SymbolId,
        type_args: Vec<ConcreteType>,
    },

    /// Interface type
    Interface {
        symbol_id: SymbolId,
        type_args: Vec<ConcreteType>,
    },

    /// Tuple type
    Tuple(Vec<ConcreteType>),

    /// Union type (sum type)
    Union(Vec<ConcreteType>),

    /// Intersection type
    Intersection(Vec<ConcreteType>),

    /// Generic type parameter
    Generic {
        name: String,
        bounds: Vec<ConcreteType>,
    },

    /// Unknown type (for error recovery)
    Unknown,

    /// Never type (for functions that never return)
    Never,

    /// Namespace type (for namespace identifiers like 'math', 'string')
    Namespace,

    /// Any type - dynamically typed value with runtime type tag
    /// Used for generic type erasure and dynamic typing scenarios
    /// At runtime: boxed value with [tag:i32][value1:i32][value2:i32] layout
    Any,

    /// Optional type — inferred when a function has any `return none` path alongside
    /// non-none return paths.  At the WASM level the representation is identical to
    /// the inner type; `none` maps to 0/null pointer.  The wrapper is purely a
    /// compile-time safety annotation that forces the caller to guard the value with
    /// `or` or `if … == none` before using it in a non-optional context.
    Optional(Box<ConcreteType>),
}

/// Generic type parameter
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParameter {
    pub name: String,
    pub bounds: Vec<ConcreteType>,
    pub default: Option<ConcreteType>,
    pub location: SourceLocation,
}

/// Type constraint for type inference
#[derive(Debug, Clone, PartialEq)]
pub enum TypeConstraint {
    /// Two types must be equal
    Equality {
        left: ConcreteType,
        right: ConcreteType,
        location: SourceLocation,
    },

    /// Left type must be subtype of right type
    Subtype {
        subtype: ConcreteType,
        supertype: ConcreteType,
        location: SourceLocation,
    },

    /// Type must implement interface
    Implements {
        type_: ConcreteType,
        interface: ConcreteType,
        location: SourceLocation,
    },

    /// Type must have specific member
    HasMember {
        type_: ConcreteType,
        member_name: String,
        member_type: ConcreteType,
        location: SourceLocation,
    },
}

/// Binary operators with type information
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
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
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    Is,    // Identity comparison (same as Equal for value types)
    IsNot, // Negated identity comparison

    // Logical
    And,
    Or,

    // BOOK: null-coalescing - Null coalescing operator
    // Usage: value default fallback (returns fallback if value is null)
    NullCoalesce,

    // Bitwise
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    LeftShift,
    RightShift,

    // String
    Concatenate,
}

/// Unary operators with type information
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Negate,
    Not,
    BitwiseNot,
    Plus,
    PreIncrement,
    PostIncrement,
    PreDecrement,
    PostDecrement,
    // BOOK: required-operator - Postfix ! assertion for null check
    Required,
}

/// Visibility modifiers
#[derive(Debug, Clone, PartialEq)]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}

impl ConcreteType {
    /// Check if this type is assignable to another type
    pub fn is_assignable_to(&self, other: &ConcreteType) -> bool {
        match (self, other) {
            // Exact type match
            (a, b) if a == b => true,

            // Integer can be assigned to Number (implicit widening)
            (ConcreteType::Integer, ConcreteType::Number) => true,

            // Sized integers assignable to base Integer and Number
            (ConcreteType::IntegerSized { .. }, ConcreteType::Integer) => true,
            (ConcreteType::IntegerSized { .. }, ConcreteType::Number) => true,
            (ConcreteType::Integer, ConcreteType::IntegerSized { .. }) => true,

            // Sized integers assignable to wider sized integers of the same signedness.
            // spec/type-system.md §5: "integer:N → integer:M (M >= N, same signedness) YES"
            // spec/type-system.md §5 Disallowed: "integer:8 signed → integer:8u unsigned — Signedness mismatch"
            (
                ConcreteType::IntegerSized {
                    bits: b1,
                    unsigned: u1,
                },
                ConcreteType::IntegerSized {
                    bits: b2,
                    unsigned: u2,
                },
            ) if b1 <= b2 && u1 == u2 => true,

            // Sized numbers assignable to base Number
            (ConcreteType::NumberSized { .. }, ConcreteType::Number) => true,
            (ConcreteType::Number, ConcreteType::NumberSized { .. }) => true,

            // Sized numbers assignable to wider sized numbers
            (ConcreteType::NumberSized { bits: b1 }, ConcreteType::NumberSized { bits: b2 })
                if b1 <= b2 =>
            {
                true
            }

            // Integer can be assigned to sized number
            (ConcreteType::Integer, ConcreteType::NumberSized { .. }) => true,
            (ConcreteType::IntegerSized { .. }, ConcreteType::NumberSized { .. }) => true,

            // Null can be assigned to any type (nullable types)
            (ConcreteType::Null, _) => true,

            // Array covariance (if element types are assignable)
            (ConcreteType::Array(a), ConcreteType::Array(b)) => a.is_assignable_to(b),

            // Matrix covariance (if element types are assignable)
            (ConcreteType::Matrix(a), ConcreteType::Matrix(b)) => a.is_assignable_to(b),

            // Function type compatibility
            (
                ConcreteType::Function {
                    parameters: p1,
                    return_type: r1,
                    is_background: a1,
                },
                ConcreteType::Function {
                    parameters: p2,
                    return_type: r2,
                    is_background: a2,
                },
            ) => {
                a1 == a2 &&
                p1.len() == p2.len() &&
                p1.iter().zip(p2.iter()).all(|(t1, t2)| t2.is_assignable_to(t1)) && // contravariant
                r1.is_assignable_to(r2) // covariant
            }

            // Union types (can assign to union if assignable to any member)
            (t, ConcreteType::Union(types)) => types.iter().any(|ut| t.is_assignable_to(ut)),

            // Union types on left (can assign from union if all members are assignable to target)
            (ConcreteType::Union(types), t) => types.iter().all(|ut| ut.is_assignable_to(t)),

            // Intersection types (can assign from intersection if all members are assignable)
            (ConcreteType::Intersection(types), t) => types.iter().all(|it| it.is_assignable_to(t)),

            // Unknown type is assignable to anything (for error recovery)
            (ConcreteType::Unknown, _) | (_, ConcreteType::Unknown) => true,

            // Optional(T) is assignable to Optional(U) if T is assignable to U
            (ConcreteType::Optional(a), ConcreteType::Optional(b)) => a.is_assignable_to(b),

            // A definite value T satisfies an Optional(T) slot (subtype relation)
            (t, ConcreteType::Optional(inner)) => t.is_assignable_to(inner),

            // Optional(T) is not assignable to a non-optional T (guarded access required)
            (ConcreteType::Optional(_), _) => false,

            // Nothing else is assignable
            _ => false,
        }
    }

    /// Get the common supertype of two types
    pub fn common_supertype(&self, other: &ConcreteType) -> ConcreteType {
        if self == other {
            return self.clone();
        }

        match (self, other) {
            // Integer and Number -> Number
            (ConcreteType::Integer, ConcreteType::Number)
            | (ConcreteType::Number, ConcreteType::Integer) => ConcreteType::Number,

            // Sized integer + Integer -> wider type
            (ConcreteType::IntegerSized { bits, unsigned }, ConcreteType::Integer)
            | (ConcreteType::Integer, ConcreteType::IntegerSized { bits, unsigned }) => {
                ConcreteType::IntegerSized {
                    bits: *bits,
                    unsigned: *unsigned,
                }
            }

            // Sized integer + Number -> Number
            (ConcreteType::IntegerSized { .. }, ConcreteType::Number)
            | (ConcreteType::Number, ConcreteType::IntegerSized { .. }) => ConcreteType::Number,

            // Sized number + Number -> wider
            (ConcreteType::NumberSized { bits }, ConcreteType::Number)
            | (ConcreteType::Number, ConcreteType::NumberSized { bits }) => {
                ConcreteType::NumberSized { bits: *bits }
            }

            // Two sized integers -> wider
            (
                ConcreteType::IntegerSized {
                    bits: b1,
                    unsigned: u1,
                },
                ConcreteType::IntegerSized {
                    bits: b2,
                    unsigned: u2,
                },
            ) => ConcreteType::IntegerSized {
                bits: *b1.max(b2),
                unsigned: *u1 && *u2,
            },

            // Two sized numbers -> wider
            (ConcreteType::NumberSized { bits: b1 }, ConcreteType::NumberSized { bits: b2 }) => {
                ConcreteType::NumberSized { bits: *b1.max(b2) }
            }

            // Array types with compatible elements
            (ConcreteType::Array(a), ConcreteType::Array(b)) => {
                ConcreteType::Array(Box::new(a.common_supertype(b)))
            }

            // Matrix types with compatible elements
            (ConcreteType::Matrix(a), ConcreteType::Matrix(b)) => {
                ConcreteType::Matrix(Box::new(a.common_supertype(b)))
            }

            // Optional + Optional -> Optional of common supertype
            (ConcreteType::Optional(a), ConcreteType::Optional(b)) => {
                ConcreteType::Optional(Box::new(a.common_supertype(b)))
            }

            // T + Optional(T) or Optional(T) + T -> Optional(T)
            (t, ConcreteType::Optional(inner)) | (ConcreteType::Optional(inner), t) => {
                ConcreteType::Optional(Box::new(t.common_supertype(inner)))
            }

            // Different types -> Union
            _ => ConcreteType::Union(vec![self.clone(), other.clone()]),
        }
    }

    /// Check if this is a numeric type
    pub fn is_numeric(&self) -> bool {
        match self {
            ConcreteType::Integer
            | ConcreteType::Number
            | ConcreteType::IntegerSized { .. }
            | ConcreteType::NumberSized { .. } => true,
            // Delegate into Optional so Optional(integer) is still numeric
            ConcreteType::Optional(inner) => inner.is_numeric(),
            _ => false,
        }
    }

    /// Check if this is a primitive type
    pub fn is_primitive(&self) -> bool {
        match self {
            ConcreteType::Integer
            | ConcreteType::Number
            | ConcreteType::String
            | ConcreteType::Boolean => true,
            ConcreteType::Optional(inner) => inner.is_primitive(),
            _ => false,
        }
    }

    /// If this type is Optional(T), return T.  Otherwise return self unchanged.
    /// Used by codegen to strip the compile-time Optional wrapper before emitting WASM.
    pub fn unwrap_optional(&self) -> &ConcreteType {
        match self {
            ConcreteType::Optional(inner) => inner.unwrap_optional(),
            other => other,
        }
    }

    /// Returns true iff this type is Optional(_).
    pub fn is_optional(&self) -> bool {
        matches!(self, ConcreteType::Optional(_))
    }

    /// Get the default value for this type
    pub fn default_value(&self) -> TastLiteral {
        match self {
            ConcreteType::Integer => TastLiteral::Integer(0),
            ConcreteType::Number => TastLiteral::Number(0.0),
            ConcreteType::String => TastLiteral::String(String::new()),
            ConcreteType::Boolean => TastLiteral::Boolean(false),
            // Optional default is none (represented as null/0)
            ConcreteType::Optional(_) => TastLiteral::Null,
            _ => TastLiteral::Null,
        }
    }
}

impl std::fmt::Display for ConcreteType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConcreteType::Integer => write!(f, "integer"),
            ConcreteType::Number => write!(f, "number"),
            ConcreteType::IntegerSized { bits, unsigned } => {
                if *unsigned {
                    write!(f, "integer:{}u", bits)
                } else {
                    write!(f, "integer:{}", bits)
                }
            }
            ConcreteType::NumberSized { bits } => write!(f, "number:{}", bits),
            ConcreteType::String => write!(f, "string"),
            ConcreteType::Boolean => write!(f, "boolean"),
            ConcreteType::Null => write!(f, "null"),
            ConcreteType::Undefined => write!(f, "undefined"),
            ConcreteType::Array(element_type) => write!(f, "Array<{}>", element_type),
            ConcreteType::Matrix(element_type) => write!(f, "Matrix<{}>", element_type),
            ConcreteType::Pairs(key_type, value_type) => {
                write!(f, "Pairs<{}, {}>", key_type, value_type)
            }
            ConcreteType::Function {
                parameters,
                return_type,
                is_background,
            } => {
                let async_str = if *is_background { "async " } else { "" };
                let params = parameters
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{}({}) -> {}", async_str, params, return_type)
            }
            ConcreteType::Class {
                symbol_id,
                type_args,
            } => {
                if type_args.is_empty() {
                    write!(f, "Class#{}", symbol_id.0)
                } else {
                    let args = type_args
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(f, "Class#{}<{}>", symbol_id.0, args)
                }
            }
            ConcreteType::Interface {
                symbol_id,
                type_args,
            } => {
                if type_args.is_empty() {
                    write!(f, "Interface#{}", symbol_id.0)
                } else {
                    let args = type_args
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(f, "Interface#{}<{}>", symbol_id.0, args)
                }
            }
            ConcreteType::Tuple(types) => {
                let types_str = types
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({})", types_str)
            }
            ConcreteType::Union(types) => {
                let types_str = types
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(f, "{}", types_str)
            }
            ConcreteType::Intersection(types) => {
                let types_str = types
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(" & ");
                write!(f, "{}", types_str)
            }
            ConcreteType::Generic { name, bounds } => {
                if bounds.is_empty() {
                    write!(f, "{}", name)
                } else {
                    let bounds_str = bounds
                        .iter()
                        .map(|b| b.to_string())
                        .collect::<Vec<_>>()
                        .join(" + ");
                    write!(f, "{}: {}", name, bounds_str)
                }
            }
            ConcreteType::Unknown => write!(f, "?"),
            ConcreteType::Never => write!(f, "!"),
            ConcreteType::Namespace => write!(f, "namespace"),
            ConcreteType::Any => write!(f, "any"),
            ConcreteType::Optional(inner) => write!(f, "{}?", inner),
        }
    }
}
