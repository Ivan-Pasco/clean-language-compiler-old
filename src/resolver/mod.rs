//! Name Resolution - Stage 4 of the Clean Language compiler pipeline
//!
//! This module resolves all symbol references in the HIR, creating symbol tables
//! and handling imports/modules. It transforms HIR into Resolved HIR (RHIR) where
//! all names are resolved to their definitions.
//!
//! Responsibilities:
//! - Symbol table construction and management
//! - Name resolution for variables, functions, classes, methods
//! - Module and import resolution
//! - Scope management and shadowing detection
//! - Forward declaration handling
//! - Error reporting for unresolved symbols

use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::hir::*;

pub mod module_resolver;
pub mod resolver_impl;
pub mod symbol_table;
// pub mod passthrough_resolver;
// pub mod passthrough_resolver; // Temporarily disabled due to compilation errors

pub use module_resolver::*;
pub use resolver_impl::*;
pub use symbol_table::*;

// Type alias for backward compatibility with lib.rs
pub type Resolver = NameResolver;

/// Resolved HIR - HIR with all symbols resolved to their definitions
#[derive(Debug, Clone)]
pub struct ResolvedHirProgram {
    pub functions: Vec<ResolvedHirFunction>,
    pub classes: Vec<ResolvedHirClass>,
    pub start_function: Option<ResolvedHirFunction>,
    pub imports: Vec<ResolvedHirImport>,
    pub tests: Vec<ResolvedHirTest>,
    pub symbol_table: GlobalSymbolTable,
    pub location: SourceLocation,
}

/// Resolved HIR Function with resolved symbol references
#[derive(Debug, Clone)]
pub struct ResolvedHirFunction {
    pub name: String,
    pub symbol_id: SymbolId,
    pub parameters: Vec<ResolvedHirParameter>,
    pub return_type: Option<HirType>,
    pub body: ResolvedHirBlock,
    pub is_start: bool,
    pub is_async: bool,
    pub location: SourceLocation,
}

/// Resolved HIR Parameter with resolved type references
#[derive(Debug, Clone)]
pub struct ResolvedHirParameter {
    pub name: String,
    pub symbol_id: SymbolId,
    pub param_type: HirType,
    pub default_value: Option<ResolvedHirExpression>,
    pub is_variadic: bool,
    pub location: SourceLocation,
}

/// Resolved HIR Class with resolved inheritance and member references
#[derive(Debug, Clone)]
pub struct ResolvedHirClass {
    pub name: String,
    pub symbol_id: SymbolId,
    pub parent: Option<SymbolId>, // Resolved parent class symbol
    pub fields: Vec<ResolvedHirField>,
    pub constructor: Option<ResolvedHirConstructor>,
    pub methods: Vec<ResolvedHirMethod>,
    pub location: SourceLocation,
}

/// Resolved HIR Field with resolved type references
#[derive(Debug, Clone)]
pub struct ResolvedHirField {
    pub name: String,
    pub symbol_id: SymbolId,
    pub field_type: HirType,
    pub initializer: Option<ResolvedHirExpression>,
    pub location: SourceLocation,
}

/// Resolved HIR Constructor with resolved parameter references
#[derive(Debug, Clone)]
pub struct ResolvedHirConstructor {
    pub parameters: Vec<ResolvedHirParameter>,
    pub body: ResolvedHirBlock,
    pub location: SourceLocation,
}

/// Resolved HIR Method with resolved symbol references
#[derive(Debug, Clone)]
pub struct ResolvedHirMethod {
    pub name: String,
    pub symbol_id: SymbolId,
    pub parameters: Vec<ResolvedHirParameter>,
    pub return_type: HirType,
    pub body: ResolvedHirBlock,
    pub location: SourceLocation,
}

/// Resolved HIR Import with resolved module references
#[derive(Debug, Clone)]
pub struct ResolvedHirImport {
    pub module_name: String,
    pub module_id: ModuleId,
    pub items: Option<Vec<(String, SymbolId)>>, // (name, resolved symbol)
    pub location: SourceLocation,
}

/// Resolved HIR Test with resolved symbol references
#[derive(Debug, Clone)]
pub struct ResolvedHirTest {
    pub name: String,
    pub description: Option<String>,
    pub body: ResolvedHirBlock,
    pub location: SourceLocation,
}

/// Resolved HIR Block with resolved symbol references
#[derive(Debug, Clone)]
pub struct ResolvedHirBlock {
    pub statements: Vec<ResolvedHirStatement>,
    pub location: SourceLocation,
}

/// Resolved HIR Statement with resolved symbol references
#[derive(Debug, Clone)]
pub enum ResolvedHirStatement {
    /// Variable declaration with resolved type and initializer
    VariableDeclaration {
        name: String,
        symbol_id: SymbolId,
        var_type: HirType,
        initializer: Option<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Assignment with resolved target and value
    Assignment {
        target: ResolvedHirLValue,
        value: ResolvedHirExpression,
        location: SourceLocation,
    },

    /// Expression statement with resolved expression
    Expression {
        expression: ResolvedHirExpression,
        location: SourceLocation,
    },

    /// Return statement with resolved value
    Return {
        value: Option<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Conditional with resolved condition and branches
    If {
        condition: ResolvedHirExpression,
        then_branch: ResolvedHirBlock,
        else_branch: Option<ResolvedHirBlock>,
        location: SourceLocation,
    },

    /// While loop with resolved condition and body
    While {
        condition: ResolvedHirExpression,
        body: ResolvedHirBlock,
        location: SourceLocation,
    },

    /// For loop with resolved iterable and body
    For {
        variable: String,
        variable_symbol_id: SymbolId,
        iterable: ResolvedHirExpression,
        body: ResolvedHirBlock,
        location: SourceLocation,
    },

    /// Print statement with resolved expression
    Print {
        expression: ResolvedHirExpression,
        newline: bool,
        location: SourceLocation,
    },

    /// Later assignment (async variable declaration) with resolved expression
    LaterAssignment {
        variable: String,
        symbol_id: SymbolId,
        expression: ResolvedHirExpression,
        location: SourceLocation,
    },
}

/// Resolved HIR Expression with all symbol references resolved
#[derive(Debug, Clone)]
pub enum ResolvedHirExpression {
    /// Literal values (no resolution needed)
    Literal {
        value: crate::ast::Value,
        location: SourceLocation,
    },

    /// Variable reference with resolved symbol
    Variable {
        name: String,
        symbol_id: SymbolId,
        location: SourceLocation,
    },

    /// Binary operation with resolved operands
    BinaryOp {
        left: Box<ResolvedHirExpression>,
        op: HirBinaryOp,
        right: Box<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Unary operation with resolved operand
    UnaryOp {
        op: HirUnaryOp,
        operand: Box<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Function call with resolved function symbol
    Call {
        function: String,
        function_symbol_id: SymbolId,
        arguments: Vec<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Method call with resolved receiver and method symbol
    MethodCall {
        receiver: Box<ResolvedHirExpression>,
        method: String,
        method_symbol_id: Option<SymbolId>, // None for built-in methods
        arguments: Vec<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Static method call with resolved class and method symbol
    StaticMethodCall {
        namespace: Vec<String>, // Empty for two-level, ["compare"] for three-level
        class_name: String,
        class_symbol_id: SymbolId,
        method: String,
        method_symbol_id: SymbolId,
        arguments: Vec<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Field access with resolved object and field symbol
    FieldAccess {
        object: Box<ResolvedHirExpression>,
        field: String,
        field_symbol_id: SymbolId,
        location: SourceLocation,
    },

    /// Array indexing with resolved array and index
    Index {
        array: Box<ResolvedHirExpression>,
        index: Box<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Array literal with resolved elements
    Array {
        elements: Vec<ResolvedHirExpression>,
        element_type: HirType,
        location: SourceLocation,
    },

    /// Constructor call with resolved class symbol
    Constructor {
        class_name: String,
        class_symbol_id: SymbolId,
        arguments: Vec<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// This reference (resolved to current class)
    This {
        class_symbol_id: SymbolId,
        location: SourceLocation,
    },

    /// Type cast with resolved expression and target type
    Cast {
        expression: Box<ResolvedHirExpression>,
        target_type: HirType,
        location: SourceLocation,
    },

    /// Assignment expression with resolved target and value
    Assignment {
        target: ResolvedHirLValue,
        value: Box<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Error handling expression (expression onError fallback)
    OnError {
        expression: Box<ResolvedHirExpression>,
        fallback: Box<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Conditional expression (if condition then value else value)
    Conditional {
        condition: Box<ResolvedHirExpression>,
        then_expr: Box<ResolvedHirExpression>,
        else_expr: Box<ResolvedHirExpression>,
        location: SourceLocation,
    },
}

/// Resolved HIR L-value with resolved symbol references
#[derive(Debug, Clone)]
pub enum ResolvedHirLValue {
    Variable {
        name: String,
        symbol_id: SymbolId,
        location: SourceLocation,
    },
    FieldAccess {
        object: Box<ResolvedHirExpression>,
        field: String,
        field_symbol_id: SymbolId,
        location: SourceLocation,
    },
    Index {
        array: Box<ResolvedHirExpression>,
        index: Box<ResolvedHirExpression>,
        location: SourceLocation,
    },
}

/// Name resolution result
#[derive(Debug)]
pub struct ResolutionResult {
    pub resolved_hir: ResolvedHirProgram,
    pub warnings: Vec<CompilerError>,
}

/// Name resolution errors
#[derive(Debug, Clone)]
pub enum ResolutionError {
    /// Symbol not found
    SymbolNotFound {
        name: String,
        location: SourceLocation,
    },

    /// Ambiguous symbol reference
    AmbiguousSymbol {
        name: String,
        candidates: Vec<SymbolId>,
        location: SourceLocation,
    },

    /// Circular dependency detected
    CircularDependency {
        symbols: Vec<SymbolId>,
        location: SourceLocation,
    },

    /// Invalid symbol usage (e.g., using function as variable)
    InvalidUsage {
        name: String,
        symbol_id: SymbolId,
        expected_kind: SymbolKind,
        actual_kind: SymbolKind,
        location: SourceLocation,
    },

    /// Module not found
    ModuleNotFound {
        module_name: String,
        location: SourceLocation,
    },

    /// Import item not found in module
    ImportItemNotFound {
        item_name: String,
        module_name: String,
        location: SourceLocation,
    },
}
