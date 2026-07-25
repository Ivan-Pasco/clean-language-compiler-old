use std::fmt;

/// `SourceLocation.file` marker for code emitted by a v1.0.0 plugin's
/// `__preamble` block. The BFS reachability scan in `mir_codegen` treats these
/// as non-roots — they're only kept if user code transitively references them.
pub const PLUGIN_OUTPUT_MARKER: &str = "<plugin-output>";

/// `SourceLocation.file` marker for code emitted by a v2 plugin's
/// `lifecycle.module_helpers` slot when `module_helpers_are_roots = true`.
/// The BFS reachability scan treats these as roots, so their bridge imports
/// are not tree-shaken even when the helper is only called from generated
/// code that the BFS cannot statically trace.
/// See foundation/spec/plugins/contracts/lifecycle.md §3.1.
pub const PLUGIN_OUTPUT_V2_ROOT_MARKER: &str = "<plugin-output-v2-root>";

/// `SourceLocation.file` marker stamped on every statement injected into the
/// program start function by a lifecycle slot dispatch (`program_init`,
/// `server_init`, `client_init`).  Lets downstream passes distinguish
/// plugin-contributed statements from user-authored `start:` body code.
/// See `plugins/expander.rs` `collect_slot_statements`.
pub const LIFECYCLE_SLOT_OUTPUT_MARKER: &str = "<lifecycle-slot-output>";

/// Prefix stamped on `SourceLocation.file` for statements emitted by a
/// specific plugin's `expand_block` hook. Format: `<plugin:NAME>` — e.g.
/// `<plugin:frame.canvas>`. Lets the telemetry classifier route diagnostics
/// on plugin-synthesized code to the plugin's owning component instead of
/// hardcoding `compiler`. See `plugins/expander.rs` and
/// `telemetry/mod.rs::extract_error_info`.
pub const PLUGIN_ORIGIN_PREFIX: &str = "<plugin:";
pub const PLUGIN_ORIGIN_SUFFIX: &str = ">";

/// Extract the plugin name from a `SourceLocation.file` marker of the form
/// `<plugin:NAME>`. Returns `None` if the file is not a plugin-origin marker.
pub fn plugin_name_from_origin_marker(file: &str) -> Option<&str> {
    let name = file.strip_prefix(PLUGIN_ORIGIN_PREFIX)?;
    let name = name.strip_suffix(PLUGIN_ORIGIN_SUFFIX)?;
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// True when the file marker is any of the compiler's known synthetic
/// placeholders. Statement-stamping logic only overwrites markers of this
/// class — real user file paths are left alone so debugging info survives.
pub fn is_synthetic_file_marker(file: &str) -> bool {
    file.is_empty()
        || file == PLUGIN_OUTPUT_MARKER
        || file == PLUGIN_OUTPUT_V2_ROOT_MARKER
        || file == LIFECYCLE_SLOT_OUTPUT_MARKER
        || file == "plugin"
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
    pub file: String,
    /// Byte offset of the start of this span in the source file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_start: Option<usize>,
    /// Byte offset of the end of this span in the source file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_end: Option<usize>,
}

impl SourceLocation {
    pub fn new(line: usize, column: usize, file: &str) -> Self {
        Self {
            line,
            column,
            file: file.to_string(),
            byte_start: None,
            byte_end: None,
        }
    }

    pub fn with_byte_span(line: usize, column: usize, file: &str, byte_start: usize) -> Self {
        Self {
            line,
            column,
            file: file.to_string(),
            byte_start: Some(byte_start),
            byte_end: None,
        }
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Value {
    Integer(i64), // Default integer (platform optimal)
    Number(f64),  // Default number (platform optimal)
    Boolean(bool),
    String(String),
    Matrix(Vec<Vec<Value>>),
    None,
    Void,
    // Advanced sized types
    Integer8(i8),
    Integer8u(u8),
    Integer16(i16),
    Integer16u(u16),
    Integer32(i32),
    Integer64(i64),
    Number32(f32),
    Number64(f64),

    // List (replaces Array)
    List(Vec<Value>),

    // Pairs (key-value associative container)
    Pairs(Vec<(Value, Value)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum ListBehavior {
    Default,        // Standard list behavior
    Line,           // Queue behavior (FIFO)
    Pile,           // Stack behavior (LIFO)
    Unique,         // Set behavior (no duplicates)
    LinePile,       // Combined line + pile (not typical, but allowed)
    LineUnique,     // Queue with unique elements
    PileUnique,     // Stack with unique elements
    LineUniquePile, // All three combined
}

impl ListBehavior {
    /// Returns the runtime behavior flags stored at offset 12 of the list header.
    /// LINE=0x01, PILE=0x02, UNIQUE=0x04
    pub fn to_flags(self) -> i32 {
        match self {
            ListBehavior::Default => 0,
            ListBehavior::Line => 0x01,
            ListBehavior::Pile => 0x02,
            ListBehavior::Unique => 0x04,
            ListBehavior::LinePile => 0x01 | 0x02,
            ListBehavior::LineUnique => 0x01 | 0x04,
            ListBehavior::PileUnique => 0x02 | 0x04,
            ListBehavior::LineUniquePile => 0x01 | 0x02 | 0x04,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub enum Type {
    // Core types from specification
    Boolean,
    Integer, // Default integer
    Number,  // Default number (floating point)
    String,
    Void,
    // null-support - None type for representing absence of value
    None,

    // Nullable wrapper: `T?` per root spec 04-type-system.md §"Nullable Types".
    // Distinct from `None` (which is the type of the `null` literal itself).
    // Codegen shares the WASM representation of `T` (pointers use 0 as the
    // null sentinel, matching the `!` (RequiredAssert) operator convention).
    Nullable(Box<Type>),

    // Advanced sized types
    IntegerSized {
        bits: u8,
        unsigned: bool,
    },
    NumberSized {
        bits: u8,
    },

    // Composite types
    /// List type with optional runtime-enforced behaviour.
    ///
    /// The behaviour modifier (`.line` / `.pile` / `.unique` and combinations,
    /// per `type-system.md` §3 "List Behavior Modes") is part of the declared
    /// type. At codegen time it is materialised as a flag byte written into
    /// the list header (offset 12) so behaviour-aware stdlib functions
    /// (`list.add`, `list.remove`, `list.peek`) can dispatch on it at
    /// runtime without recompilation.
    List(Box<Type>, ListBehavior),
    Matrix(Box<Type>),
    Pairs(Box<Type>, Box<Type>),

    // Generic types
    Generic(Box<Type>, Vec<Type>),
    TypeParameter(String),

    // Object types
    Object(String),
    Class {
        name: String,
        type_args: Vec<Type>,
    },
    Function(Vec<Type>, Box<Type>),

    // Background types
    Future(Box<Type>),

    // Handler — a first-class function reference passed to bridge functions
    // At the WASM level this is an i32 function-table index.
    Handler,

    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Is,
    Not,

    // Logical
    And,
    Or,

    // Null-coalescing (spec grammar.ebnf line 232: default_op)
    // Usage: value default fallback — returns lhs if not null, else rhs
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub enum UnaryOperator {
    Negate,
    Not,
    /// Postfix `!` (required / non-null assertion) — spec grammar.ebnf line 235.
    /// Traps at runtime if the value is null (0); otherwise returns the value unchanged.
    RequiredAssert,
}

/// Assignment target variants as defined by `assignment_target` in foundation/spec/grammar.ebnf:
///   assignment_target = identifier
///                     , [ ( "[" , additive_expression , "]" )
///                       | ( "." , identifier , { "." , identifier } ) ]
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum AssignmentTarget {
    /// Simple variable assignment: `name = value`
    Variable(String),
    /// List index assignment: `list[index] = value`
    Index {
        collection: String,
        index: Box<Expression>,
    },
    /// Property chain assignment: `obj.prop = value` or `obj.a.b = value`
    Property { object: String, path: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Parameter {
    pub name: String,
    pub type_: Type,
    pub default_value: Option<Expression>,
}

impl Parameter {
    pub fn new(name: String, type_: Type) -> Self {
        Self {
            name,
            type_,
            default_value: None,
        }
    }

    pub fn new_with_default(name: String, type_: Type, default_value: Expression) -> Self {
        Self {
            name,
            type_,
            default_value: Some(default_value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Expression {
    Literal(Value),
    Variable(String),
    Binary(Box<Expression>, BinaryOperator, Box<Expression>),
    Unary(UnaryOperator, Box<Expression>),
    Call(String, Vec<Expression>),

    // Namespace calls (math.sqrt(), string.length(), etc.)
    NamespaceCall {
        namespace: String,
        function: String,
        arguments: Vec<Expression>,
        location: SourceLocation,
    },

    // Property and method access
    PropertyAccess {
        object: Box<Expression>,
        property: String,
        location: SourceLocation,
    },

    // Property assignment (for list.type = behavior)
    PropertyAssignment {
        object: Box<Expression>,
        property: String,
        value: Box<Expression>,
        location: SourceLocation,
    },

    // List assignment (for list[index] = value)
    ListAssignment {
        list: Box<Expression>,
        index: Box<Expression>,
        value: Box<Expression>,
        location: SourceLocation,
    },
    MethodCall {
        object: Box<Expression>,
        method: String,
        arguments: Vec<Expression>,
        location: SourceLocation,
    },

    /// `chained_method_call` — (function_call | property_access | identifier)
    /// followed by one or more `.method()` segments.
    /// foundation/spec/grammar.ebnf: `chained_method_call`
    ChainedMethodCall {
        receiver: Box<Expression>,
        chain: Vec<(String, Vec<Expression>)>, // (method_name, args) pairs
        location: SourceLocation,
    },

    /// `multiple_method_call` — a base receiver followed by two or more
    /// `.method()` segments (superset of `method_call`; kept distinct per spec).
    /// foundation/spec/grammar.ebnf: `multiple_method_call`
    ///
    /// Note: structurally identical to `ChainedMethodCall` (two or more
    /// segments); mapped to the same WASM codegen path. Kept as a separate
    /// variant to preserve spec fidelity.
    MultipleMethodCall {
        receiver: Box<Expression>,
        chain: Vec<(String, Vec<Expression>)>,
        location: SourceLocation,
    },

    /// `three_level_method_call` — `a.b.method(args)` where all three parts
    /// are identifiers.
    /// foundation/spec/grammar.ebnf: `three_level_method_call`
    ThreeLevelMethodCall {
        first: String,
        second: String,
        method: String,
        arguments: Vec<Expression>,
        location: SourceLocation,
    },

    /// `property_method_call` — property chain ending in a method call:
    /// `a.b.c...method(args)` where the path is 3+ identifiers.
    /// foundation/spec/grammar.ebnf: `property_method_call`
    PropertyMethodCall {
        object: String,
        path: Vec<String>, // intermediate property segments (may be empty)
        method: String,
        arguments: Vec<Expression>,
        location: SourceLocation,
    },

    // Static method call (ClassName.method() or namespace.ClassName.method())
    StaticMethodCall {
        namespace: Vec<String>, // Empty for two-level calls, ["compare"] for three-level
        class_name: String,
        method: String,
        arguments: Vec<Expression>,
        location: SourceLocation,
    },

    // List and Matrix access
    ListAccess(Box<Expression>, Box<Expression>),
    MatrixAccess(Box<Expression>, Box<Expression>, Box<Expression>),

    // String interpolation
    StringInterpolation(Vec<StringPart>),

    // Object creation
    ObjectCreation {
        class_name: String,
        arguments: Vec<Expression>,
        location: SourceLocation,
    },

    // Error handling
    OnError {
        expression: Box<Expression>,
        fallback: Box<Expression>,
        location: SourceLocation,
    },

    // Error handling with block
    OnErrorBlock {
        expression: Box<Expression>,
        error_handler: Vec<Statement>,
        location: SourceLocation,
    },

    // Error variable access (only valid in error handling contexts)
    ErrorVariable {
        location: SourceLocation,
    },

    // Conditional expressions: if condition then value else value
    Conditional {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
        location: SourceLocation,
    },

    // Base constructor call: base(args...)
    BaseCall {
        arguments: Vec<Expression>,
        location: SourceLocation,
    },

    // Background expressions
    StartExpression {
        expression: Box<Expression>,
        location: SourceLocation,
    },

    // Later assignment (for background tasks)
    LaterAssignment {
        variable: String,
        expression: Box<Expression>,
        location: SourceLocation,
    },

    // Input expressions (for console input)
    Input {
        prompt: Option<String>,
        input_type: InputType,
        location: SourceLocation,
    },

    // Range expressions (1..10)
    Range {
        start: Box<Expression>,
        end: Box<Expression>,
        inclusive: bool,
        location: SourceLocation,
    },

    /// Anonymous object literal `{ key: expr, ... }` — `pairs_literal` per
    /// grammar.ebnf:167-169.  Field values are `single_line_expression`, so
    /// they may be variables, calls, or binary expressions — not only
    /// constant `Value`s.  Lowered through HIR / resolver / typechecker into
    /// `TastExpressionKind::ObjectLiteral`, which the MIR builder handles.
    ///
    /// Keys are restricted by the grammar to `string_literal | identifier |
    /// decimal_integer`, so the `Value` slot is always `Value::String(_)` or
    /// `Value::Integer(_)`.  Identifier keys (bare names like `title:`) are
    /// represented as `Value::String(name)`.
    ObjectLiteral {
        fields: Vec<(Value, Expression)>,
        location: SourceLocation,
    },

    /// Named argument binding — appears ONLY as an element of a call argument list.
    ///
    /// `label: value` inside any function/method/constructor call.
    ///
    /// Semantic rules (foundation/spec/semantic-rules.md FUNC008–FUNC011):
    /// - FUNC008: `label` must match a declared parameter name of the callee.
    /// - FUNC009: No duplicate `label` within the same call.
    /// - FUNC010: All positional arguments must precede named arguments in the same call.
    /// - FUNC011: Every parameter must be covered exactly once across positional + named args.
    ///
    /// This variant is consumed and erased by the HIR builder during lowering.
    /// It MUST NOT reach codegen or any stage after HIR construction.
    NamedArgBinding {
        label: String,
        value: Box<Expression>,
        location: SourceLocation,
    },

    /// ORM query block expression — `Model.verb:` followed by an indented block body.
    ///
    /// Created by the parser when a `PropertyAccess` (e.g., `User.find`) is immediately
    /// followed by `:` and an indented block of sub-clauses (`join:`, `where:`, `order:`,
    /// etc.).  The raw block content is preserved verbatim so that the `frame.data` plugin
    /// can process it without loss of formatting.
    ///
    /// This variant MUST be expanded by the plugin expander before reaching the HIR builder.
    /// If it reaches HIR construction, the HIR builder will emit a `SEM001` diagnostic.
    ///
    /// Grammar: `foundation/spec/plugins/frame-data.ebnf` `query_expression`
    OrmQuery {
        /// The model name, e.g. `"User"`
        model: String,
        /// The query verb, e.g. `"find"`, `"first"`, `"insert"`, `"update"`, `"delete"`
        verb: String,
        /// Raw block body (the indented sub-clauses: join:, where:, order:, etc.)
        content: String,
        location: SourceLocation,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum StringPart {
    Text(String),
    Interpolation(Expression),
}

/// Input types for console input expressions
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum InputType {
    String,  // input("prompt")
    Integer, // input.integer("prompt")
    Number,  // input.number("prompt")
    Boolean, // input.yesNo("prompt")
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Statement {
    // Contract: Require statement - precondition that must be true
    // Can only appear inside functions or class methods
    // Always checked at runtime (cannot be disabled)
    Require {
        condition: Expression,
        location: Option<SourceLocation>,
    },

    // Contract: Ensure statement - postcondition that the return value must satisfy.
    // `result` is a special identifier referring to the function's return value.
    // Debug builds: checked at runtime via WASM trap; release builds: stripped.
    // `--contracts` flag forces checks in release builds.
    Ensure {
        condition: Expression,
        location: Option<SourceLocation>,
    },

    // Variable declarations (type-first)
    VariableDecl {
        name: String,
        type_: Type,
        initializer: Option<Expression>,
        location: Option<SourceLocation>,
    },

    // Functions block statement
    FunctionsBlock {
        functions: Vec<Function>,
        location: Option<SourceLocation>,
    },

    // Apply blocks - Three types as per specification
    TypeApplyBlock {
        type_: Type,
        assignments: Vec<VariableAssignment>,
        location: Option<SourceLocation>,
    },

    FunctionApplyBlock {
        function_name: String,
        expressions: Vec<Expression>,
        location: Option<SourceLocation>,
    },

    MethodApplyBlock {
        object_name: String,
        method_chain: Vec<String>,
        expressions: Vec<Expression>,
        location: Option<SourceLocation>,
    },

    ConstantApplyBlock {
        constants: Vec<ConstantAssignment>,
        location: Option<SourceLocation>,
    },

    // Assignment — target supports variable, index, and property-chain forms
    // per foundation/spec/grammar.ebnf `assignment_target` rule.
    Assignment {
        target: AssignmentTarget,
        value: Expression,
        location: Option<SourceLocation>,
    },

    // Print statements
    Print {
        expression: Expression,
        newline: bool,
        location: Option<SourceLocation>,
    },

    // Print block (multiple expressions)
    PrintBlock {
        expressions: Vec<Expression>,
        newline: bool,
        location: Option<SourceLocation>,
    },

    // Return
    Return {
        value: Option<Expression>,
        location: Option<SourceLocation>,
    },

    // Break - exits the innermost loop
    Break {
        location: Option<SourceLocation>,
    },

    // Continue - skips to next iteration of innermost loop
    Continue {
        location: Option<SourceLocation>,
    },

    // Control flow
    If {
        condition: Expression,
        then_branch: Vec<Statement>,
        else_branch: Option<Vec<Statement>>,
        location: Option<SourceLocation>,
    },

    // Iteration
    Iterate {
        iterator: String,
        collection: Expression,
        body: Vec<Statement>,
        location: Option<SourceLocation>,
    },

    RangeIterate {
        iterator: String,
        start: Expression,
        end: Expression,
        step: Option<Expression>,
        inclusive: bool,
        body: Vec<Statement>,
        location: Option<SourceLocation>,
    },

    // While loop
    While {
        condition: Expression,
        body: Vec<Statement>,
        location: Option<SourceLocation>,
    },

    // Test
    Test {
        name: String,
        body: Vec<Statement>,
        location: Option<SourceLocation>,
    },

    // Tests block
    TestsBlock {
        tests: Vec<TestCase>,
        location: Option<SourceLocation>,
    },

    // Expression statement
    Expression {
        expr: Expression,
        location: Option<SourceLocation>,
    },

    // Error statement (throw error)
    Error {
        message: Expression,
        location: Option<SourceLocation>,
    },

    // Module imports
    Import {
        imports: Vec<ImportItem>,
        location: Option<SourceLocation>,
    },

    // Background statements
    LaterAssignment {
        variable: String,
        expression: Expression,
        location: Option<SourceLocation>,
    },

    Background {
        expression: Expression,
        location: Option<SourceLocation>,
    },

    // Error handling with block
    OnErrorBlock {
        expression: Expression,
        error_block: Vec<Statement>,
        location: Option<SourceLocation>,
    },

    // Standalone error handler (global error handling for previous statements)
    StandaloneErrorHandler {
        body: Vec<Statement>,
        location: Option<SourceLocation>,
    },

    // Class definition statement
    ClassDefinition {
        class: Class,
        location: Option<SourceLocation>,
    },

    // Description statement (function metadata)
    Description {
        text: String,
        location: Option<SourceLocation>,
    },

    // AI metadata: links function to specification document
    Spec {
        path: String,
        location: Option<SourceLocation>,
    },

    // AI metadata: describes function purpose in natural language
    Intent {
        description: String,
        location: Option<SourceLocation>,
    },

    // Top-level: marks file as generated from a spec
    SourceBlock {
        spec_path: String,
        version: Option<String>,
        location: Option<SourceLocation>,
    },

    // Top-level: build configuration block
    // Configures compiler behaviour for features like rules checking.
    // `rules_enabled` is true, false, or the string "development".
    BuildBlock {
        rules_enabled: Expression,
        location: Option<SourceLocation>,
    },

    // Framework extension block (for Clean Frame plugins)
    // Generic container for DSL blocks like endpoints:, data, component
    // These are expanded by plugins before HIR transformation
    FrameworkBlock {
        name: String,    // Block identifier: "endpoints", "data", "component", etc.
        content: String, // Raw content of the block (unparsed DSL)
        attributes: Vec<FrameworkAttribute>, // Optional attributes like @pk, @unique
        location: Option<SourceLocation>,
    },

    // Clean UI screen definition
    // Parsed structured form of screen "Name": ... blocks
    ScreenBlock {
        screen: Screen,
        location: Option<SourceLocation>,
    },

    // Clean UI inline block (ui.column, ui.text, etc. used as statements)
    // Note: Boxed to break recursive type cycle with UiNode::ExpressionStatement
    UiBlock {
        node: Box<UiNode>,
        location: Option<SourceLocation>,
    },

    // ========================================================================
    // STATE MANAGEMENT STATEMENTS
    // ========================================================================

    // State block - declares persistent state variables
    // Top-level state: = app scope, state: inside screen: = screen scope
    StateBlockStmt {
        state_block: StateBlock,
        location: Option<SourceLocation>,
    },

    // Watch block - react to state changes
    WatchBlockStmt {
        watch_block: WatchBlock,
        location: Option<SourceLocation>,
    },

    // Reset statement - reset state to initial value
    // reset count (single variable) or reset state (all state in scope)
    ResetStmt {
        target: ResetTarget,
        location: Option<SourceLocation>,
    },

    // Screen block - UI screen with its own state scope
    ScreenBlockStmt {
        name: String,
        state: Option<StateBlock>,
        watch_blocks: Vec<WatchBlock>,
        functions: Vec<Function>,
        location: Option<SourceLocation>,
    },

    // ========================================================================
    // VALIDATE BLOCK — Named, immutable validation schema declaration
    // validate userSchema:
    //     name: string required length: 1 to 50
    //     ...
    //     messages:
    //         default: "Invalid"
    // ========================================================================
    /// Top-level validate block: declares a named, immutable validation schema.
    ValidateDeclaration {
        schema: ValidateBlock,
        location: Option<SourceLocation>,
    },

    /// validate_check_stmt: runs a schema against an input and branches on result.
    /// schemaName.check inputExpr:
    ///     ok: ...
    ///     error: ...
    ValidateCheck {
        check: ValidateCheckBlock,
        location: Option<SourceLocation>,
    },
}

impl Statement {
    /// Return a mutable reference to this statement's location field.
    /// Used to stamp lifecycle-slot-injected statements with
    /// [`LIFECYCLE_SLOT_OUTPUT_MARKER`] so they are distinguishable from
    /// user-authored `start:` body statements.
    pub fn location_mut(&mut self) -> &mut Option<SourceLocation> {
        match self {
            Statement::Require { location, .. } => location,
            Statement::Ensure { location, .. } => location,
            Statement::VariableDecl { location, .. } => location,
            Statement::FunctionsBlock { location, .. } => location,
            Statement::TypeApplyBlock { location, .. } => location,
            Statement::FunctionApplyBlock { location, .. } => location,
            Statement::MethodApplyBlock { location, .. } => location,
            Statement::ConstantApplyBlock { location, .. } => location,
            Statement::Assignment { location, .. } => location,
            Statement::Print { location, .. } => location,
            Statement::PrintBlock { location, .. } => location,
            Statement::Return { location, .. } => location,
            Statement::Break { location, .. } => location,
            Statement::Continue { location, .. } => location,
            Statement::If { location, .. } => location,
            Statement::Iterate { location, .. } => location,
            Statement::RangeIterate { location, .. } => location,
            Statement::While { location, .. } => location,
            Statement::Test { location, .. } => location,
            Statement::TestsBlock { location, .. } => location,
            Statement::Expression { location, .. } => location,
            Statement::Error { location, .. } => location,
            Statement::Import { location, .. } => location,
            Statement::LaterAssignment { location, .. } => location,
            Statement::Background { location, .. } => location,
            Statement::OnErrorBlock { location, .. } => location,
            Statement::StandaloneErrorHandler { location, .. } => location,
            Statement::ClassDefinition { location, .. } => location,
            Statement::Description { location, .. } => location,
            Statement::Spec { location, .. } => location,
            Statement::Intent { location, .. } => location,
            Statement::SourceBlock { location, .. } => location,
            Statement::BuildBlock { location, .. } => location,
            Statement::FrameworkBlock { location, .. } => location,
            Statement::ScreenBlock { location, .. } => location,
            Statement::UiBlock { location, .. } => location,
            Statement::StateBlockStmt { location, .. } => location,
            Statement::WatchBlockStmt { location, .. } => location,
            Statement::ResetStmt { location, .. } => location,
            Statement::ScreenBlockStmt { location, .. } => location,
            Statement::ValidateDeclaration { location, .. } => location,
            Statement::ValidateCheck { location, .. } => location,
        }
    }
}

// ============================================================================
// Validate block supporting types
// ============================================================================

/// A named validation schema declaration (`validate name:`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ValidateBlock {
    /// The name bound to this schema (e.g. `userSchema`).
    pub name: String,
    /// Ordered list of field rules.
    pub fields: Vec<ValidateField>,
    /// Optional `messages:` sub-block for per-field and default error messages.
    pub messages: Option<ValidateMessages>,
}

impl ValidateBlock {
    /// Create a new named validation schema with no fields or messages.
    pub fn new(name: String) -> Self {
        Self {
            name,
            fields: Vec::new(),
            messages: None,
        }
    }
}

/// A single field entry inside a `validate` block.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ValidateField {
    pub name: String,
    pub field_type: ValidateFieldType,
    pub constraints: Vec<ValidateConstraint>,
}

/// The declared type of a validate field.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ValidateFieldType {
    String,
    Integer,
    Number,
    Boolean,
}

/// A single constraint on a validate field.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ValidateConstraint {
    /// `required` — field must be present and non-empty.
    Required,
    /// `trim` — strip whitespace before other constraints are checked.
    Trim,
    /// `length: min to max` — string character count bounds.
    Length {
        min: Box<Expression>,
        max: Box<Expression>,
    },
    /// `min: expr` — numeric lower bound.
    Min(Box<Expression>),
    /// `max: expr` — numeric upper bound.
    Max(Box<Expression>),
    /// `match: patternName` — named or user-defined regex pattern.
    Match(String),
    /// `oneOf: val, val, ...` — string or integer literal enumeration.
    OneOf(Vec<Expression>),
    /// `custom: functionName` — user-defined boolean validator function.
    Custom(String),
}

/// Optional `messages:` sub-block inside a `validate` block.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ValidateMessages {
    /// `default: "text"` — fallback message for any field not specifically listed.
    pub default_message: Option<String>,
    /// `fieldName: "text"` — per-field overrides.
    pub field_messages: Vec<(String, String)>,
}

/// A `schemaName.check expr:` statement.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ValidateCheckBlock {
    /// The name of the schema variable to call `.check` on.
    pub schema_name: String,
    /// The input expression to validate.
    pub input: Box<Expression>,
    /// Body executed when validation passes; `value` (pairs) is implicitly bound.
    pub ok_branch: Vec<Statement>,
    /// Body executed when validation fails; `errors` (list<string>) is implicitly bound.
    pub error_branch: Vec<Statement>,
}

impl ValidateCheckBlock {
    /// Create a new validate check block.
    pub fn new(
        schema_name: String,
        input: Expression,
        ok_branch: Vec<Statement>,
        error_branch: Vec<Statement>,
    ) -> Self {
        Self {
            schema_name,
            input: Box::new(input),
            ok_branch,
            error_branch,
        }
    }
}

// ============================================================================

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct VariableAssignment {
    pub name: String,
    pub initializer: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ConstantAssignment {
    pub type_: Type,
    pub name: String,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ImportItem {
    /// Module name (e.g., "Math", "math.sqrt") or file path (e.g., "app/data/models.cln")
    pub name: String,
    /// Optional alias for the import
    pub alias: Option<String>,
    /// True if this is a file path import (import "path/to/file.cln")
    /// False if this is a module import (import Math)
    pub is_file_import: bool,
}

// ============================================================================
// Endpoint Test AST Types (tests: block endpoint syntax)
// ============================================================================

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HttpTestRequest {
    pub method: HttpMethod,
    pub path: String,
    pub body: Option<Vec<(String, Expression)>>,
    pub header: Option<(String, String)>,
}

/// Comparison operator for endpoint test assertions.
/// Distinct from BinaryOperator to keep the test AST self-contained.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum HttpComparisonOp {
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum HttpTestAssertion {
    Status {
        op: HttpComparisonOp,
        value: i64,
    },
    JsonField {
        path: Vec<String>,
        op: HttpComparisonOp,
        value: Expression,
    },
    JsonFieldNotNull {
        path: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EndpointTest {
    pub name: String,
    pub request: HttpTestRequest,
    pub assertions: Vec<HttpTestAssertion>,
    pub location: Option<SourceLocation>,
}

/// The two kinds of test case inside a `tests:` block.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TestCaseKind {
    /// `"desc": expr = expected`  or  `expr = expected`
    Expression {
        test_expression: Expression,
        expected_value: Expression,
    },
    /// `test "name"\n    METHOD "path"\n    assertions…`
    Endpoint(EndpointTest),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TestCase {
    pub description: Option<String>,
    pub kind: TestCaseKind,
    pub location: Option<SourceLocation>,
}

/// Attribute for framework DSL elements (e.g., @pk, @unique, @required)
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FrameworkAttribute {
    pub name: String,          // Attribute name: "pk", "unique", "required"
    pub value: Option<String>, // Optional value: @default("value")
    pub location: Option<SourceLocation>,
}

// ============================================================================
// Clean UI AST Types
// ============================================================================

/// UI Screen definition: screen "Name": ...
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Screen {
    pub name: String,
    pub state: Option<Vec<StateVariable>>,
    pub body: Vec<UiNode>,
    pub location: Option<SourceLocation>,
}

impl Screen {
    /// Create a new screen with the given name and no state or body nodes.
    pub fn new(name: String, location: Option<SourceLocation>) -> Self {
        Self {
            name,
            state: None,
            body: Vec::new(),
            location,
        }
    }
}

/// State variable in a screen: count: integer = 0
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StateVariable {
    pub name: String,
    pub type_name: String, // "integer", "number", "text", "bool", "list"
    pub default_value: Option<Expression>,
    pub location: Option<SourceLocation>,
}

/// UI node types for Clean UI widgets
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum UiNode {
    /// Layout containers: ui.column, ui.row, ui.stack
    Container {
        kind: UiContainerKind,
        props: UiProps,
        children: Vec<UiNode>,
        location: Option<SourceLocation>,
    },

    /// Text widget: ui.text "Hello"
    Text {
        content: Expression,
        props: UiProps,
        location: Option<SourceLocation>,
    },

    /// Button widget: ui.button "Click"
    Button {
        label: Expression,
        props: UiProps,
        events: Vec<UiEvent>,
        location: Option<SourceLocation>,
    },

    /// Text input: ui.textField
    TextField {
        props: UiProps,
        events: Vec<UiEvent>,
        location: Option<SourceLocation>,
    },

    /// Text area: ui.textArea
    TextArea {
        props: UiProps,
        events: Vec<UiEvent>,
        location: Option<SourceLocation>,
    },

    /// Checkbox: ui.checkbox
    Checkbox {
        props: UiProps,
        events: Vec<UiEvent>,
        location: Option<SourceLocation>,
    },

    /// Toggle switch: ui.switch
    Switch {
        props: UiProps,
        events: Vec<UiEvent>,
        location: Option<SourceLocation>,
    },

    /// Select dropdown: ui.select
    Select {
        props: UiProps,
        events: Vec<UiEvent>,
        location: Option<SourceLocation>,
    },

    /// Image: ui.image
    Image {
        props: UiProps,
        location: Option<SourceLocation>,
    },

    /// Divider line: ui.divider
    Divider {
        props: UiProps,
        location: Option<SourceLocation>,
    },

    /// Card container: ui.card
    Card {
        props: UiProps,
        children: Vec<UiNode>,
        location: Option<SourceLocation>,
    },

    /// Link: ui.link
    Link {
        props: UiProps,
        events: Vec<UiEvent>,
        location: Option<SourceLocation>,
    },

    /// Spacer: ui.spacer
    Spacer { location: Option<SourceLocation> },

    /// Canvas region: ui.region target "canvas"
    Region {
        props: UiProps,
        children: Vec<UiNode>,
        location: Option<SourceLocation>,
    },

    /// Canvas scene: ui.canvasScene
    CanvasScene {
        draw: Vec<Statement>,
        events: Vec<UiEvent>,
        location: Option<SourceLocation>,
    },

    /// For loop in UI: for item in items: ui.text item
    ForLoop {
        iterator: String,
        collection: Expression,
        body: Vec<UiNode>,
        location: Option<SourceLocation>,
    },

    /// If condition in UI: if state.show: ui.text "Visible"
    IfBlock {
        condition: Expression,
        then_branch: Vec<UiNode>,
        else_branch: Option<Vec<UiNode>>,
        location: Option<SourceLocation>,
    },

    /// Expression statement (e.g., print inside event handlers gets wrapped)
    /// Note: Boxed to break recursive type cycle with Statement::UiBlock
    ExpressionStatement {
        statement: Box<Statement>,
        location: Option<SourceLocation>,
    },
}

/// Container types for layout
#[derive(Debug, Clone, PartialEq, Copy, serde::Serialize)]
pub enum UiContainerKind {
    Column,
    Row,
    Stack,
}

/// Common properties for UI widgets
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct UiProps {
    pub gap: Option<Expression>,
    pub padding: Option<Expression>,
    pub visible: Option<Expression>,
    pub disabled: Option<Expression>,
    pub label: Option<Expression>,
    pub value: Option<Expression>,
    pub placeholder: Option<Expression>,
    pub size: Option<Expression>,
    pub weight: Option<Expression>,
    pub tone: Option<Expression>,
    pub color: Option<Expression>,
    pub background: Option<Expression>,
    pub radius: Option<Expression>,
    pub align: Option<Expression>,
    pub justify: Option<Expression>,
    pub target: Option<String>, // For ui.region target "canvas"
    pub height: Option<Expression>,
    pub width: Option<Expression>,
    pub options: Option<Expression>, // For ui.select
}

/// Event handler for UI widgets
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UiEvent {
    pub name: String,        // "onClick", "onChange", "onFocus", "onBlur", "draw"
    pub params: Vec<String>, // ["value"] for onChange, ["dt"] for draw
    pub body: Vec<Statement>,
    pub location: Option<SourceLocation>,
}

/// Framework block container for plugin expansion
/// This is the structured form passed to plugins for DSL transformation
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FrameworkBlock {
    pub name: String,    // Block identifier: "endpoints", "data", "component"
    pub content: String, // Raw content of the block (unparsed DSL)
    pub attributes: Vec<FrameworkAttribute>, // Optional attributes
    pub location: Option<SourceLocation>,
}

// ============================================================================
// STATE MANAGEMENT AST TYPES
// State is a first-class language concept with persistent memory, observability,
// and explicit scoping (app-level vs screen-level)
// ============================================================================

/// State scope - determines lifetime and visibility of state
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, serde::Serialize)]
pub enum StateScope {
    App,    // Application lifetime, visible everywhere
    Screen, // Screen lifetime, visible only within screen
}

/// State block - declares persistent state variables
/// Top-level state: = app scope, state: inside screen: = screen scope
/// May optionally contain a rules: block for state invariants
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StateBlock {
    pub declarations: Vec<StateDeclaration>,
    pub computed: Vec<ComputedDeclaration>,
    pub rules: Option<RulesBlock>, // State invariants (requires build: block)
    pub scope: StateScope,
    pub location: Option<SourceLocation>,
}

impl StateBlock {
    /// Create a new state block with no declarations, computed values, or rules.
    pub fn new(scope: StateScope, location: Option<SourceLocation>) -> Self {
        Self {
            declarations: Vec::new(),
            computed: Vec::new(),
            rules: None,
            scope,
            location,
        }
    }
}

/// Individual state declaration with type, name, initial value, and optional guard
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StateDeclaration {
    pub name: String,
    pub type_: Type,
    pub initializer: Expression,
    pub guard: Option<GuardClause>,
    /// True when declared inside a `private:` sub-section of a `state:` block.
    /// Private state variables may not be read or written from importing modules.
    /// See SEM005 in foundation/spec/semantic-rules.md.
    pub is_private: bool,
    pub location: Option<SourceLocation>,
}

impl StateDeclaration {
    /// Create a public state declaration with no guard clause.
    pub fn new(
        name: String,
        type_: Type,
        initializer: Expression,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            name,
            type_,
            initializer,
            guard: None,
            is_private: false,
            location,
        }
    }

    /// Create a private state declaration with no guard clause.
    pub fn new_private(
        name: String,
        type_: Type,
        initializer: Expression,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            name,
            type_,
            initializer,
            guard: None,
            is_private: true,
            location,
        }
    }
}

/// Guard clause - validates state before mutation
/// Example: guard value >= 0 else "Count cannot be negative"
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GuardClause {
    pub condition: Expression, // Condition to validate (uses 'value' for proposed new value)
    pub error_message: String, // Error message if validation fails
    pub location: Option<SourceLocation>,
}

impl GuardClause {
    /// Create a new guard clause.
    pub fn new(
        condition: Expression,
        error_message: String,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            condition,
            error_message,
            location,
        }
    }
}

/// Computed state declaration - derived values that auto-update when dependencies change
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ComputedDeclaration {
    pub name: String,
    pub type_: Type,
    pub body: Vec<Statement>, // Body that computes the value (must end with return)
    pub location: Option<SourceLocation>,
}

/// Watch block - react to state changes
/// Example: watch count: print("Count changed")
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WatchBlock {
    pub targets: Vec<String>, // State variable names to watch
    pub body: Vec<Statement>, // Code to execute when state changes
    pub location: Option<SourceLocation>,
}

impl WatchBlock {
    /// Create a new watch block.
    pub fn new(
        targets: Vec<String>,
        body: Vec<Statement>,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            targets,
            body,
            location,
        }
    }
}

/// Reset target - what to reset
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ResetTarget {
    Variable(String), // Reset single state variable
    AllState,         // Reset all state in current scope (reset state)
}

// ============================================================================
// CONTRACT AST TYPES
// Contracts provide runtime correctness guarantees:
// - require: preconditions in functions/methods (always checked)
// - rules: state invariants (configurable via build: block)
// ============================================================================

/// Require statement - declares a precondition that must be true
/// Can only appear inside functions or class methods
/// Always checked at runtime (cannot be disabled)
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RequireStatement {
    pub condition: Expression,
    pub location: Option<SourceLocation>,
}

/// Rules block - declares state invariants that must always be true
/// Must appear inside state: block, after all state declarations
/// Requires build: block to configure when rules are checked
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RulesBlock {
    pub rules: Vec<Expression>,
    pub location: Option<SourceLocation>,
}

// ============================================================================
// END CONTRACT AST TYPES
// ============================================================================

// ============================================================================
// END STATE MANAGEMENT AST TYPES
// ============================================================================

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum FunctionModifier {
    None,
    Background,
    /// Method declared inside a class's `events:` section.
    ///
    /// Semantically identical to a method declared inside `functions:` —
    /// same signature, same body, called by user code as `instance.method()`.
    /// Distinguished only so the post-expansion shim pass can find these
    /// methods, locate the singleton state-block instance of the owning
    /// class, and emit a bare-named top-level dispatch shim for each. The
    /// shim is what loader.js looks up via `instance.exports[handlerName]()`
    /// when a click event fires. Closes HYDRATE_AUTO Gap 2 once frame.ui's
    /// `normalize_handlers` rewrite is reverted.
    EventHandler,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum FunctionSyntax {
    Simple,     // function integer add() ...
    Detailed,   // function integer add() with description/input blocks
    Block,      // functions: block
    Standalone, // start() function (can be outside functions block)
    Background, // background function modifier
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TypeConstraint {
    pub type_parameter: String,
    pub constraint_type: Type,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Function {
    pub name: String,
    pub type_parameters: Vec<String>,
    pub type_constraints: Vec<TypeConstraint>,
    pub parameters: Vec<Parameter>,
    pub return_type: Type,
    pub body: Vec<Statement>,
    pub description: Option<String>,
    pub syntax: FunctionSyntax,
    pub visibility: Visibility,
    pub modifier: FunctionModifier,
    pub location: Option<SourceLocation>,
}

impl Function {
    pub fn new(
        name: String,
        parameters: Vec<Parameter>,
        return_type: Type,
        body: Vec<Statement>,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            name,
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters,
            return_type,
            body,
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Private,
            modifier: FunctionModifier::None,
            location,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Visibility {
    Public,
    Private,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Field {
    pub name: String,
    pub type_: Type,
    pub visibility: Visibility,
    pub is_static: bool,
    pub default_value: Option<Expression>,
}

impl Field {
    /// Create a private, non-static field with no default value.
    /// Visibility defaults to Private per the spec (2026-06-25 flip);
    /// callers that need a public field must set `visibility` explicitly
    /// after construction (the parser does this when the field appears
    /// inside a `public:` sub-section).
    pub fn new(name: String, type_: Type) -> Self {
        Self {
            name,
            type_,
            visibility: Visibility::Private,
            is_static: false,
            default_value: None,
        }
    }

    /// Create a private, non-static field with a default value.
    /// See `Field::new` for the visibility default rationale.
    pub fn new_with_default(name: String, type_: Type, default_value: Expression) -> Self {
        Self {
            name,
            type_,
            visibility: Visibility::Private,
            is_static: false,
            default_value: Some(default_value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Constructor {
    pub parameters: Vec<Parameter>,
    pub body: Vec<Statement>,
    pub location: Option<SourceLocation>,
}

impl Constructor {
    pub fn new(
        parameters: Vec<Parameter>,
        body: Vec<Statement>,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            parameters,
            body,
            location,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Class {
    pub name: String,
    pub type_parameters: Vec<String>,
    pub description: Option<String>,
    pub base_class: Option<String>, // Using "is" inheritance
    pub base_class_type_args: Vec<Type>,
    pub fields: Vec<Field>,
    pub methods: Vec<Function>,
    pub constructor: Option<Constructor>,
    /// Class invariant expressions parsed from `always:` block.
    /// Each expression must evaluate to boolean.
    /// Checked after every public method call on the class (debug builds only).
    pub invariants: Vec<Expression>,
    /// Capabilities claimed via `can C1, C2, ...` clause on the class header.
    /// Nominal — each name must resolve to a `Capability` declaration.
    /// See Clean Language Specification §Capabilities and grammar.ebnf §6.4a.
    pub capabilities: Vec<String>,
    pub location: Option<SourceLocation>,
}

impl Class {
    /// Create a bare class with no fields, methods, constructor, or invariants.
    pub fn new(name: String, location: Option<SourceLocation>) -> Self {
        Self {
            name,
            type_parameters: Vec::new(),
            description: None,
            base_class: None,
            base_class_type_args: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            constructor: None,
            invariants: Vec::new(),
            capabilities: Vec::new(),
            location,
        }
    }
}

/// A capability declaration (`can Name:`). Names a contract of methods
/// that classes may claim via `can Name` on their class declaration.
/// See Clean Language Specification §Capabilities and grammar.ebnf §6.4a.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Capability {
    pub name: String,
    pub methods: Vec<CapabilityMethod>,
    pub location: Option<SourceLocation>,
}

impl Capability {
    pub fn new(name: String, location: Option<SourceLocation>) -> Self {
        Self {
            name,
            methods: Vec::new(),
            location,
        }
    }
}

/// One method entry in a capability. `default_body` is `None` for a
/// required signature (class MUST implement) and `Some(...)` for a
/// default the class inherits unless it overrides.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CapabilityMethod {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Type,
    pub default_body: Option<Vec<Statement>>,
    pub location: Option<SourceLocation>,
}

/// External function declaration - a function provided by the WASM host (imported)
/// These functions are declared in external: blocks and generate WASM import entries
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ExternalFunction {
    /// Function name (e.g., "_req_body_field", "_http_respond")
    pub name: String,
    /// Function parameters
    pub parameters: Vec<Parameter>,
    /// Return type (Type::Void for functions that don't return)
    pub return_type: Type,
    /// WASM import module name (defaults to "env")
    pub module: String,
    /// Source location for error reporting
    pub location: Option<SourceLocation>,
}

impl ExternalFunction {
    /// Create an external function declaration in the default `"env"` WASM module.
    pub fn new(
        name: String,
        parameters: Vec<Parameter>,
        return_type: Type,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            name,
            parameters,
            return_type,
            module: "env".to_string(),
            location,
        }
    }

    /// Create an external function declaration with an explicit WASM module name.
    pub fn new_in_module(
        name: String,
        parameters: Vec<Parameter>,
        return_type: Type,
        module: String,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            name,
            parameters,
            return_type,
            module,
            location,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Program {
    pub imports: Vec<ImportItem>,
    pub plugins: Vec<String>, // Framework plugins (e.g., "frame.ui", "frame.data")
    pub statements: Vec<Statement>,
    pub functions: Vec<Function>,
    pub classes: Vec<Class>,
    /// Top-level capability declarations (`can Name:`).
    pub capabilities: Vec<Capability>,
    pub start_function: Option<Function>,
    pub tests: Vec<TestCase>,
    pub screens: Vec<Screen>,             // Clean UI screens (legacy)
    pub state: Option<StateBlock>,        // App-level state
    pub watch_blocks: Vec<WatchBlock>,    // Top-level watch observers
    pub screen_blocks: Vec<Statement>,    // Screen blocks with state scope
    pub externals: Vec<ExternalFunction>, // External functions (WASM imports)
    pub source_block: Option<Statement>,  // AI metadata: source: block
    pub location: Option<SourceLocation>,
}

impl Program {
    /// Create an empty program with the given source location.
    /// All collections are empty and optional fields are None.
    pub fn new(location: Option<SourceLocation>) -> Self {
        Self {
            imports: Vec::new(),
            plugins: Vec::new(),
            statements: Vec::new(),
            functions: Vec::new(),
            classes: Vec::new(),
            capabilities: Vec::new(),
            start_function: None,
            tests: Vec::new(),
            screens: Vec::new(),
            state: None,
            watch_blocks: Vec::new(),
            screen_blocks: Vec::new(),
            externals: Vec::new(),
            source_block: None,
            location,
        }
    }

    /// Create an empty program with no location information.
    pub fn empty() -> Self {
        Self::new(None)
    }
}

// Display implementations for better error messages
impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Type::Boolean => f.write_str("boolean"),
            Type::Integer => f.write_str("integer"),
            Type::Number => f.write_str("number"),
            Type::String => f.write_str("string"),
            Type::Void => f.write_str("void"),
            // null-support - Display none type
            Type::None => f.write_str("none"),
            Type::Nullable(inner) => write!(f, "{}?", inner),
            Type::IntegerSized { bits, unsigned } => {
                if *unsigned {
                    write!(f, "integer:{bits}u")
                } else {
                    write!(f, "integer:{bits}")
                }
            }
            Type::NumberSized { bits } => write!(f, "number:{bits}"),
            // Type::Array removed - now using Type::List
            Type::List(inner, behavior) => {
                use crate::stdlib::list_behavior::behavior_to_string;
                match behavior {
                    ListBehavior::Default => write!(f, "list<{inner}>"),
                    _ => {
                        // Non-default behaviors use dot notation: list<T>.line, list<T>.unique, etc.
                        // This matches the parser's expected syntax (grammar.ebnf list_behavior).
                        let suffix = behavior_to_string(*behavior);
                        write!(f, "list<{inner}>.{suffix}")
                    }
                }
            }
            Type::Matrix(inner) => write!(f, "matrix<{inner}>"),
            Type::Pairs(key, value) => write!(f, "pairs<{key}, {value}>"),
            Type::Function(params, ret) => {
                write!(f, "function(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{param}")?;
                }
                write!(f, ") returns {ret}")
            }
            Type::Object(name) => write!(f, "{name}"),
            Type::Class { name, type_args } => {
                if type_args.is_empty() {
                    write!(f, "{name}")
                } else {
                    write!(f, "{name}<")?;
                    for (i, arg) in type_args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    write!(f, ">")
                }
            }
            Type::Generic(base, args) => {
                write!(f, "{base}<")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ">")
            }
            Type::TypeParameter(name) => write!(f, "{name}"),
            Type::Future(inner) => write!(f, "Future<{inner}>"),
            Type::Handler => f.write_str("handler"),
            Type::Any => f.write_str("any"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(i) => write!(f, "{i}"),
            Value::Number(n) => write!(f, "{n}"),
            Value::String(s) => write!(f, "\"{s}\""),
            Value::Boolean(b) => write!(f, "{b}"),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Value::Matrix(rows) => {
                write!(f, "[")?;
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "[")?;
                    for (j, value) in row.iter().enumerate() {
                        if j > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{value}")?;
                    }
                    write!(f, "]")?;
                }
                write!(f, "]")
            }
            Value::None => write!(f, "none"),
            Value::Void => write!(f, "()"),
            Value::Integer8(i) => write!(f, "{i}:8"),
            Value::Integer8u(u) => write!(f, "{u}:8u"),
            Value::Integer16(i) => write!(f, "{i}:16"),
            Value::Integer16u(u) => write!(f, "{u}:16u"),
            Value::Integer32(i) => write!(f, "{i}:32"),
            Value::Integer64(i) => write!(f, "{i}:64"),
            Value::Number32(f_val) => write!(f, "{f_val}:32"),
            Value::Number64(f_val) => write!(f, "{f_val}:64"),
            Value::Pairs(pairs) => {
                write!(f, "{{")?;
                for (i, (key, value)) in pairs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{key}: {value}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl Type {
    pub fn as_type_ref(&self) -> &Type {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_origin_marker_round_trips() {
        // Format is <plugin:NAME>; parser must recover NAME exactly.
        let marker = format!(
            "{}frame.canvas{}",
            PLUGIN_ORIGIN_PREFIX, PLUGIN_ORIGIN_SUFFIX
        );
        assert_eq!(
            plugin_name_from_origin_marker(&marker),
            Some("frame.canvas")
        );
        // Not a marker at all.
        assert_eq!(plugin_name_from_origin_marker(""), None);
        assert_eq!(plugin_name_from_origin_marker("/tmp/user.cln"), None);
        // Malformed markers must not be silently accepted.
        assert_eq!(plugin_name_from_origin_marker("<plugin:>"), None);
        assert_eq!(
            plugin_name_from_origin_marker("<plugin:missing-suffix"),
            None
        );
    }

    #[test]
    fn synthetic_file_markers_are_recognized() {
        assert!(is_synthetic_file_marker(""));
        assert!(is_synthetic_file_marker(PLUGIN_OUTPUT_MARKER));
        assert!(is_synthetic_file_marker(PLUGIN_OUTPUT_V2_ROOT_MARKER));
        assert!(is_synthetic_file_marker(LIFECYCLE_SLOT_OUTPUT_MARKER));
        assert!(is_synthetic_file_marker("plugin"));
        // Real user paths must not be treated as synthetic — otherwise the
        // stamping logic would overwrite them and destroy debug info.
        assert!(!is_synthetic_file_marker("/home/user/app.cln"));
        assert!(!is_synthetic_file_marker("src/main.cln"));
        // A plugin-origin marker is itself synthetic, but is already tagged;
        // the stamping logic keys off `is_synthetic_file_marker` for the
        // pre-stamp state so plugin markers are NOT reconsidered synthetic.
        assert!(!is_synthetic_file_marker("<plugin:frame.canvas>"));
    }

    #[test]
    fn test_ast_serialization() {
        // Test that all major AST types can be serialized to JSON

        // Test simple types
        let int_type = Type::Integer;
        let json = serde_json::to_string(&int_type).expect("test: Type::Integer must serialize");
        assert!(json.contains("Integer"));

        // Test Value
        let value = Value::Integer(42);
        let json = serde_json::to_string(&value).expect("test: Value::Integer must serialize");
        assert!(json.contains("42"));

        // Test Expression
        let expr = Expression::Literal(Value::String("test".to_string()));
        let json = serde_json::to_string(&expr).expect("test: Expression::Literal must serialize");
        assert!(json.contains("test"));

        // Test Statement
        let stmt = Statement::Return {
            value: Some(Expression::Literal(Value::Boolean(true))),
            location: None,
        };
        let json = serde_json::to_string(&stmt).expect("test: Statement::Return must serialize");
        assert!(json.contains("Return"));

        // Test Function
        let func = Function::new("test_func".to_string(), vec![], Type::Void, vec![], None);
        let json = serde_json::to_string(&func).expect("test: Function must serialize");
        assert!(json.contains("test_func"));

        // Test Program
        let program = Program {
            imports: vec![],
            plugins: vec![],
            statements: vec![],
            functions: vec![func],
            classes: vec![],
            capabilities: vec![],
            start_function: None,
            tests: vec![],
            screens: vec![],
            state: None,
            watch_blocks: vec![],
            screen_blocks: vec![],
            externals: vec![],
            source_block: None,
            location: None,
        };
        let json = serde_json::to_string(&program).expect("test: Program must serialize");
        assert!(json.contains("test_func"));
        assert!(!json.is_empty());
    }
}
