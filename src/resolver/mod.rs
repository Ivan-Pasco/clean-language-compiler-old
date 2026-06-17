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

/// Resolved HIR - HIR with all symbols resolved to their definitions
#[derive(Debug, Clone)]
pub struct ResolvedHirProgram {
    pub functions: Vec<ResolvedHirFunction>,
    pub classes: Vec<ResolvedHirClass>,
    pub start_function: Option<ResolvedHirFunction>,
    pub imports: Vec<ResolvedHirImport>,
    pub tests: Vec<ResolvedHirTest>,
    pub state: Option<ResolvedHirStateBlock>,
    /// Top-level watch blocks (reactive state observers)
    pub watch_blocks: Vec<ResolvedHirWatchBlock>,
    pub symbol_table: GlobalSymbolTable,
    pub location: SourceLocation,
    /// External functions (WASM imports from host)
    pub externals: Vec<ResolvedHirExternalFunction>,
}

/// Resolved HIR External Function (WASM import)
#[derive(Debug, Clone)]
pub struct ResolvedHirExternalFunction {
    pub name: String,
    pub parameters: Vec<ResolvedHirParameter>,
    pub return_type: HirType,
    pub module: String,
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
    pub is_background: bool,
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
    /// Resolved invariant conditions from the `always:` block.
    pub invariants: Vec<ResolvedHirExpression>,
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
    pub symbol_id: SymbolId,
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

/// Resolved state block with resolved expressions
#[derive(Debug, Clone)]
pub struct ResolvedHirStateBlock {
    pub declarations: Vec<ResolvedHirStateDeclaration>,
    /// Computed (derived) state declarations
    pub computed: Vec<ResolvedHirComputedDeclaration>,
    /// State invariants checked at operation boundaries
    pub rules: Vec<ResolvedHirExpression>,
    pub scope: HirStateScope,
    pub location: SourceLocation,
}

/// Resolved computed state declaration — a derived value that is recomputed
/// whenever its source state variables change.
///
/// The `body` must contain a `return` statement. At compile time, only the
/// types are validated; dependency tracking is a runtime concern.
#[derive(Debug, Clone)]
pub struct ResolvedHirComputedDeclaration {
    /// Symbol ID for the computed variable (registered as read-only state)
    pub symbol_id: SymbolId,
    pub name: String,
    pub computed_type: HirType,
    pub body: ResolvedHirBlock,
    pub location: SourceLocation,
}

/// Resolved watch block — a reactive observer that fires when named state
/// variables are mutated at runtime.
///
/// Compile-time validation ensures that every target name refers to a known
/// symbol in the current state scope. The body is type-checked for statement
/// correctness but does not need to return a value.
#[derive(Debug, Clone)]
pub struct ResolvedHirWatchBlock {
    /// Names of state variables being observed
    pub targets: Vec<String>,
    /// Resolved symbol IDs for each target (parallel to `targets`)
    pub target_symbol_ids: Vec<SymbolId>,
    pub body: ResolvedHirBlock,
    pub location: SourceLocation,
}

/// Resolved state declaration with resolved initializer and guard
#[derive(Debug, Clone)]
pub struct ResolvedHirStateDeclaration {
    pub symbol_id: SymbolId,
    pub name: String,
    pub state_type: HirType,
    pub initializer: ResolvedHirExpression,
    pub guard: Option<ResolvedHirGuardClause>,
    pub location: SourceLocation,
}

/// Resolved guard clause with resolved condition
#[derive(Debug, Clone)]
pub struct ResolvedHirGuardClause {
    pub condition: ResolvedHirExpression,
    pub value_symbol_id: SymbolId, // Symbol ID for the 'value' binding in guard conditions
    pub error_message: String,
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

    /// For loop with resolved iterable and body
    For {
        variable: String,
        variable_symbol_id: SymbolId,
        iterable: ResolvedHirExpression,
        body: ResolvedHirBlock,
        location: SourceLocation,
    },

    /// While loop with resolved condition and body
    While {
        condition: ResolvedHirExpression,
        body: ResolvedHirBlock,
        location: SourceLocation,
    },

    /// Break statement - exit innermost loop
    Break { location: SourceLocation },

    /// Continue statement - skip to next iteration of innermost loop
    Continue { location: SourceLocation },

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

    /// Background statement (fire-and-forget async call) with resolved expression
    Background {
        expression: ResolvedHirExpression,
        location: SourceLocation,
    },

    /// Require statement - contract precondition that must be true
    /// Traps at runtime if condition is false
    Require {
        condition: ResolvedHirExpression,
        location: SourceLocation,
    },

    /// Ensure statement - contract postcondition on the return value.
    /// `result` in the condition refers to the function's return value.
    /// Debug builds emit a WASM trap if the condition is false; release builds strip it.
    Ensure {
        condition: ResolvedHirExpression,
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
        constructor_symbol_id: SymbolId,
        arguments: Vec<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// This reference (resolved to current class - created internally for implicit field access)
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

    /// Base constructor call (in derived class constructors)
    BaseCall {
        parent_class_symbol_id: SymbolId,
        arguments: Vec<ResolvedHirExpression>,
        location: SourceLocation,
    },

    /// Range expression (start..end or start..=end)
    Range {
        start: Box<ResolvedHirExpression>,
        end: Box<ResolvedHirExpression>,
        step: Option<Box<ResolvedHirExpression>>,
        inclusive: bool,
        location: SourceLocation,
    },

    /// Anonymous object literal `{ key: expr, ... }` with resolved field
    /// expressions.  Mirrors `HirExpression::ObjectLiteral`.
    ObjectLiteral {
        fields: Vec<(crate::ast::Value, ResolvedHirExpression)>,
        location: SourceLocation,
    },
}

impl ResolvedHirExpression {
    /// Get the source location of this expression
    pub fn location(&self) -> &SourceLocation {
        match self {
            ResolvedHirExpression::Literal { location, .. } => location,
            ResolvedHirExpression::Variable { location, .. } => location,
            ResolvedHirExpression::BinaryOp { location, .. } => location,
            ResolvedHirExpression::UnaryOp { location, .. } => location,
            ResolvedHirExpression::Call { location, .. } => location,
            ResolvedHirExpression::MethodCall { location, .. } => location,
            ResolvedHirExpression::StaticMethodCall { location, .. } => location,
            ResolvedHirExpression::FieldAccess { location, .. } => location,
            ResolvedHirExpression::Index { location, .. } => location,
            ResolvedHirExpression::Array { location, .. } => location,
            ResolvedHirExpression::Constructor { location, .. } => location,
            ResolvedHirExpression::This { location, .. } => location,
            ResolvedHirExpression::Cast { location, .. } => location,
            ResolvedHirExpression::Assignment { location, .. } => location,
            ResolvedHirExpression::OnError { location, .. } => location,
            ResolvedHirExpression::Conditional { location, .. } => location,
            ResolvedHirExpression::BaseCall { location, .. } => location,
            ResolvedHirExpression::Range { location, .. } => location,
            ResolvedHirExpression::ObjectLiteral { location, .. } => location,
        }
    }
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
