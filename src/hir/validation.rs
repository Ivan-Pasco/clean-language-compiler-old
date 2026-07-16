//! HIR Validation - Semantic consistency validation for HIR
//!
//! This module validates the HIR for semantic consistency without performing type checking.
//! Type checking is handled in Stage 5. This validation ensures:
//! - All referenced symbols are defined
//! - Control flow is valid (returns in functions, etc.)
//! - Method/field access is structurally valid
//! - Constructor calls are valid
//! - Inheritance relationships are valid

use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::hir::*;
use std::collections::{HashMap, HashSet};

/// HIR validation context that tracks defined symbols
#[derive(Debug)]
pub struct ValidationContext {
    /// Functions available in the current scope
    pub functions: HashMap<String, HirFunction>,

    /// Classes available in the current scope
    pub classes: HashMap<String, HirClass>,

    /// Variables in the current scope stack
    pub variables: Vec<HashMap<String, HirType>>,

    /// Current class being validated (for 'this' references)
    pub current_class: Option<String>,

    /// Current function return type (for return validation)
    pub current_return_type: Option<HirType>,

    /// Plugin-registered namespace prefixes (e.g. "req", "db") derived from
    /// external functions whose names contain a dot (e.g. "req.query").
    /// These are valid as method-call receivers without being declared variables.
    pub plugin_namespaces: HashSet<String>,

    /// Names of all module-level mutable state variables (from the top-level `state:` block).
    /// Used by the CONC001 check to detect unsynchronised shared-state access from
    /// inside `background:` expressions.
    pub state_var_names: HashSet<String>,

    /// Set to `true` while validating the expression of a `background:` statement.
    /// Any access to a module-level state variable in this context triggers CONC001.
    pub inside_background: bool,

    /// Set to `true` while validating a function that is a request handler.
    /// Request handlers are identified by: a parameter named `req` or `request`,
    /// a parameter whose type is named `Request`, a function whose name starts
    /// with `__route_handler_` (the compiler-generated route handler prefix), or
    /// a function whose name appears as the third argument of an `_http_route`
    /// or `_http_route_protected` call anywhere in the program (plugins register
    /// handlers this way — see [[fix_conc002_plugin_handlers]]).
    /// Any use of request-context builtins (req.*, session.*, res.*) outside a
    /// request handler triggers CONC002.
    pub inside_request_handler: bool,

    /// Function names that are registered as request handlers by an
    /// `_http_route(_, _, handler)` or `_http_route_protected(_, _, handler, _)`
    /// call anywhere in the program. Populated in a pre-pass before validation
    /// so that both user-written and plugin-emitted handlers are recognised.
    pub plugin_registered_handlers: HashSet<String>,

    /// Validation errors and warnings
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<CompilerError>,
}

impl Default for ValidationContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationContext {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            classes: HashMap::new(),
            variables: vec![HashMap::new()], // Global scope
            current_class: None,
            current_return_type: None,
            plugin_namespaces: HashSet::new(),
            state_var_names: HashSet::new(),
            inside_background: false,
            inside_request_handler: false,
            plugin_registered_handlers: HashSet::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Push a new variable scope
    pub fn push_scope(&mut self) {
        self.variables.push(HashMap::new());
    }

    /// Pop the current variable scope
    pub fn pop_scope(&mut self) {
        if self.variables.len() > 1 {
            self.variables.pop();
        }
    }

    /// Add a variable to the current scope
    pub fn declare_variable(&mut self, name: String, var_type: HirType) {
        if let Some(current_scope) = self.variables.last_mut() {
            current_scope.insert(name, var_type);
        }
    }

    /// Look up a variable in all scopes
    pub fn lookup_variable(&self, name: &str) -> Option<&HirType> {
        for scope in self.variables.iter().rev() {
            if let Some(var_type) = scope.get(name) {
                return Some(var_type);
            }
        }
        None
    }

    /// Determine whether `function` qualifies as a request handler for CONC002 purposes,
    /// using only the function's own signature and name.
    ///
    /// A function is a request handler when ANY of the following hold:
    /// - Its name starts with `__route_handler_` (compiler-generated route wrapper)
    /// - It has a parameter named exactly `req` or `request`
    /// - It has a parameter whose type is `HirType::Named { name: "Request", .. }`
    ///
    /// Note: this static form does NOT know about plugin-emitted `_http_route`
    /// registrations. Callers that have a `ValidationContext` should prefer
    /// [`Self::is_request_handler`] which also consults the plugin-registered set.
    pub fn function_is_request_handler(function: &HirFunction) -> bool {
        if function.name.starts_with("__route_handler_") {
            return true;
        }
        for param in &function.parameters {
            if param.name == "req" || param.name == "request" {
                return true;
            }
            if let HirType::Named { ref name, .. } = param.param_type {
                if name == "Request" {
                    return true;
                }
            }
        }
        false
    }

    /// Determine whether `function` qualifies as a request handler for CONC002 purposes,
    /// consulting both the function's own signature and the set of plugin-registered
    /// handlers collected in the pre-pass (`_http_route` third-arg).
    pub fn is_request_handler(&self, function: &HirFunction) -> bool {
        if Self::function_is_request_handler(function) {
            return true;
        }
        self.plugin_registered_handlers.contains(&function.name)
    }

    /// Add an error
    pub fn error(&mut self, message: &str, location: SourceLocation) {
        self.errors
            .push(CompilerError::validation_error(message, location));
    }

    /// Add an error with a spec error code (e.g. "FUNC006")
    pub fn error_with_code(&mut self, message: &str, location: SourceLocation, code: &str) {
        self.errors.push(CompilerError::Validation {
            context: Box::new(
                crate::error::ErrorContext::new(
                    message,
                    None,
                    crate::error::ErrorType::Validation,
                    Some(location),
                )
                .with_error_code(code),
            ),
        });
    }

    /// Add a warning
    pub fn warning(&mut self, message: &str, location: SourceLocation) {
        self.warnings
            .push(CompilerError::validation_warning(message, location));
    }

    /// Add a warning with a spec error code (e.g. "FUNC007")
    pub fn warning_with_code(&mut self, message: &str, location: SourceLocation, code: &str) {
        self.warnings.push(CompilerError::Validation {
            context: Box::new(
                crate::error::ErrorContext::new(
                    message,
                    None,
                    crate::error::ErrorType::Validation,
                    Some(location),
                )
                .with_severity(crate::error::ErrorSeverity::Warning)
                .with_error_code(code),
            ),
        });
    }
}

/// HIR Validator - performs semantic validation on HIR
pub struct HirValidator;

impl HirValidator {
    /// Validate a complete HIR program
    pub fn validate(hir: &HirProgram) -> Result<(), Vec<CompilerError>> {
        let mut context = ValidationContext::new();

        // Derive plugin-registered namespace prefixes from externals whose names
        // contain a dot (e.g. "req.query" → namespace "req", "db.query" → "db").
        // These must be accepted as valid method-call receivers without being
        // declared as local variables — the resolver wires them up in stage 4.
        for external in &hir.externals {
            if let Some(dot_pos) = external.name.find('.') {
                let ns = &external.name[..dot_pos];
                if !ns.is_empty() {
                    context.plugin_namespaces.insert(ns.to_string());
                }
            }
        }

        // First pass: collect all function and class definitions
        Self::collect_definitions(&mut context, hir);

        // Pre-pass for CONC002: walk every function body (plus start, screen
        // functions, class methods, and tests) looking for
        // `_http_route(method, path, handler)` and
        // `_http_route_protected(method, path, handler, role)` calls. Any
        // function whose name appears as the third argument is a
        // plugin-registered request handler. This is how framework plugins
        // (frame.server, frame.pages, …) attach handlers at compile time;
        // without walking these calls, plugin-emitted handlers appear to the
        // validator as ordinary functions and every `req.*` call inside them
        // raises a spurious CONC002.
        Self::collect_plugin_registered_handlers(&mut context, hir);

        // Second pass: validate all constructs
        Self::validate_program(&mut context, hir);

        // Return errors if any
        if context.errors.is_empty() {
            Ok(())
        } else {
            Err(context.errors)
        }
    }

    /// Collect all top-level definitions (functions and classes)
    fn collect_definitions(context: &mut ValidationContext, hir: &HirProgram) {
        // Collect functions
        for function in &hir.functions {
            if context.functions.contains_key(&function.name) {
                // SEM003: symbol declared more than once in the same scope.
                context.error_with_code(
                    &format!("Function '{}' is already defined", function.name),
                    function.location.clone(),
                    "SEM003",
                );
            } else {
                context
                    .functions
                    .insert(function.name.clone(), function.clone());
            }
        }

        // Collect start function if present
        if let Some(start_func) = &hir.start_function {
            if context.functions.contains_key(&start_func.name) {
                // SEM003: `start` cannot share a name with an existing function.
                context.error_with_code(
                    &format!(
                        "Function '{}' conflicts with start function",
                        start_func.name
                    ),
                    start_func.location.clone(),
                    "SEM003",
                );
            } else {
                context
                    .functions
                    .insert(start_func.name.clone(), start_func.clone());
            }
        }

        // Collect classes
        for class in &hir.classes {
            if context.classes.contains_key(&class.name) {
                // SEM003: class declared more than once.
                context.error_with_code(
                    &format!("Class '{}' is already defined", class.name),
                    class.location.clone(),
                    "SEM003",
                );
            } else {
                context.classes.insert(class.name.clone(), class.clone());
            }
        }

        // Collect top-level state variables into global scope so expressions
        // referencing them do not produce spurious "Undefined variable" errors.
        // (SCOPE005 enforcement is handled later by the resolver.)
        // Also record all mutable state variable names for the CONC001 check —
        // accessing these from inside a `background:` block without synchronisation
        // is a concurrency violation.
        if let Some(ref state_block) = hir.state {
            for decl in &state_block.declarations {
                context.declare_variable(decl.name.clone(), decl.state_type.clone());
                context.state_var_names.insert(decl.name.clone());
            }
        }

        // Collect computed: state variables into global scope so `area = width * height`
        // computed declarations do not produce spurious "Undefined variable" errors.
        if let Some(ref state_block) = hir.state {
            for computed in &state_block.computed {
                context.declare_variable(computed.name.clone(), computed.computed_type.clone());
            }
        }

        // Collect screen-local state variables into global scope.
        // The validator cannot enforce SCOPE005 (that requires symbol table metadata);
        // we simply make the names known so the validator does not reject them.
        // The resolver will emit SCOPE005 when a screen-local variable is accessed
        // outside its owning screen.
        for screen in &hir.screen_blocks {
            if let Some(ref screen_state) = screen.state {
                for decl in &screen_state.declarations {
                    context.declare_variable(decl.name.clone(), decl.state_type.clone());
                }
                for computed in &screen_state.computed {
                    context.declare_variable(computed.name.clone(), computed.computed_type.clone());
                }
            }
        }

        // Register imported module names as valid namespaces so `MathUtils.square(5)`
        // is not rejected as "Undefined variable 'MathUtils'" before the resolver runs.
        for import in &hir.imports {
            if !import.module_name.is_empty() {
                context.plugin_namespaces.insert(import.module_name.clone());
            }
        }
    }

    /// Walk every block in the program looking for `_http_route(_, _, handler)`
    /// and `_http_route_protected(_, _, handler, _)` calls, and add the third
    /// argument's function name to `context.plugin_registered_handlers`.
    ///
    /// The third argument is expected to be either:
    /// - `HirExpression::Variable { name, .. }` — the typed-emission bridge form
    ///   (see `src/plugins/typed_emission/bridges.rs::_emit_route`), or
    /// - `HirExpression::Literal { value: Value::String(name), .. }` — the
    ///   textual-assembler form (see `src/plugins/builtin_assemblers.rs`).
    ///
    /// Anything else (dynamic dispatch, computed handler names) is not
    /// currently supported and simply won't be added — those handlers would
    /// need to declare a `req`/`request`/`Request` parameter to be recognised.
    fn collect_plugin_registered_handlers(context: &mut ValidationContext, hir: &HirProgram) {
        for function in &hir.functions {
            Self::scan_block_for_route_registrations(context, &function.body);
        }
        if let Some(start_func) = &hir.start_function {
            Self::scan_block_for_route_registrations(context, &start_func.body);
        }
        for class in &hir.classes {
            if let Some(constructor) = &class.constructor {
                Self::scan_block_for_route_registrations(context, &constructor.body);
            }
            for method in &class.methods {
                Self::scan_block_for_route_registrations(context, &method.body);
            }
        }
        for screen in &hir.screen_blocks {
            for function in &screen.functions {
                Self::scan_block_for_route_registrations(context, &function.body);
            }
        }
        for test in &hir.tests {
            Self::scan_block_for_route_registrations(context, &test.body);
        }
    }

    fn scan_block_for_route_registrations(context: &mut ValidationContext, block: &HirBlock) {
        for stmt in &block.statements {
            Self::scan_stmt_for_route_registrations(context, stmt);
        }
    }

    fn scan_stmt_for_route_registrations(context: &mut ValidationContext, stmt: &HirStatement) {
        match stmt {
            HirStatement::VariableDeclaration { initializer, .. } => {
                if let Some(expr) = initializer {
                    Self::scan_expr_for_route_registrations(context, expr);
                }
            }
            HirStatement::Assignment { value, .. } => {
                Self::scan_expr_for_route_registrations(context, value);
            }
            HirStatement::Expression { expression, .. } => {
                Self::scan_expr_for_route_registrations(context, expression);
            }
            HirStatement::Return { value, .. } => {
                if let Some(expr) = value {
                    Self::scan_expr_for_route_registrations(context, expr);
                }
            }
            HirStatement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                Self::scan_expr_for_route_registrations(context, condition);
                Self::scan_block_for_route_registrations(context, then_branch);
                if let Some(else_block) = else_branch {
                    Self::scan_block_for_route_registrations(context, else_block);
                }
            }
            HirStatement::For { iterable, body, .. } => {
                Self::scan_expr_for_route_registrations(context, iterable);
                Self::scan_block_for_route_registrations(context, body);
            }
            HirStatement::While {
                condition, body, ..
            } => {
                Self::scan_expr_for_route_registrations(context, condition);
                Self::scan_block_for_route_registrations(context, body);
            }
            HirStatement::Print { expression, .. } => {
                Self::scan_expr_for_route_registrations(context, expression);
            }
            HirStatement::LaterAssignment { expression, .. } => {
                Self::scan_expr_for_route_registrations(context, expression);
            }
            HirStatement::Background { expression, .. } => {
                Self::scan_expr_for_route_registrations(context, expression);
            }
            HirStatement::Require { condition, .. } => {
                Self::scan_expr_for_route_registrations(context, condition);
            }
            HirStatement::Ensure { condition, .. } => {
                Self::scan_expr_for_route_registrations(context, condition);
            }
            HirStatement::Break { .. } | HirStatement::Continue { .. } => {}
        }
    }

    fn scan_expr_for_route_registrations(context: &mut ValidationContext, expr: &HirExpression) {
        match expr {
            HirExpression::Call {
                function,
                arguments,
                ..
            } => {
                if (function == "_http_route" || function == "_http_route_protected")
                    && arguments.len() >= 3
                {
                    if let Some(name) = Self::handler_name_from_expr(&arguments[2]) {
                        context.plugin_registered_handlers.insert(name);
                    }
                }
                for arg in arguments {
                    Self::scan_expr_for_route_registrations(context, arg);
                }
            }
            HirExpression::BinaryOp { left, right, .. } => {
                Self::scan_expr_for_route_registrations(context, left);
                Self::scan_expr_for_route_registrations(context, right);
            }
            HirExpression::UnaryOp { operand, .. } => {
                Self::scan_expr_for_route_registrations(context, operand);
            }
            HirExpression::MethodCall {
                receiver,
                arguments,
                ..
            } => {
                Self::scan_expr_for_route_registrations(context, receiver);
                for arg in arguments {
                    Self::scan_expr_for_route_registrations(context, arg);
                }
            }
            HirExpression::FieldAccess { object, .. } => {
                Self::scan_expr_for_route_registrations(context, object);
            }
            HirExpression::Index { array, index, .. } => {
                Self::scan_expr_for_route_registrations(context, array);
                Self::scan_expr_for_route_registrations(context, index);
            }
            HirExpression::Array { elements, .. } => {
                for el in elements {
                    Self::scan_expr_for_route_registrations(context, el);
                }
            }
            HirExpression::Constructor { arguments, .. } => {
                for arg in arguments {
                    Self::scan_expr_for_route_registrations(context, arg);
                }
            }
            HirExpression::Cast { expression, .. } => {
                Self::scan_expr_for_route_registrations(context, expression);
            }
            HirExpression::Assignment { value, .. } => {
                Self::scan_expr_for_route_registrations(context, value);
            }
            HirExpression::NamespaceCall { arguments, .. } => {
                for arg in arguments {
                    Self::scan_expr_for_route_registrations(context, arg);
                }
            }
            HirExpression::StaticMethodCall { arguments, .. } => {
                for arg in arguments {
                    Self::scan_expr_for_route_registrations(context, arg);
                }
            }
            HirExpression::OnError {
                expression,
                fallback,
                ..
            } => {
                Self::scan_expr_for_route_registrations(context, expression);
                Self::scan_expr_for_route_registrations(context, fallback);
            }
            HirExpression::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                Self::scan_expr_for_route_registrations(context, condition);
                Self::scan_expr_for_route_registrations(context, then_expr);
                Self::scan_expr_for_route_registrations(context, else_expr);
            }
            HirExpression::BaseCall { arguments, .. } => {
                for arg in arguments {
                    Self::scan_expr_for_route_registrations(context, arg);
                }
            }
            HirExpression::Range {
                start, end, step, ..
            } => {
                Self::scan_expr_for_route_registrations(context, start);
                Self::scan_expr_for_route_registrations(context, end);
                if let Some(step_expr) = step {
                    Self::scan_expr_for_route_registrations(context, step_expr);
                }
            }
            HirExpression::ObjectLiteral { fields, .. } => {
                for (_, value_expr) in fields {
                    Self::scan_expr_for_route_registrations(context, value_expr);
                }
            }
            HirExpression::Literal { .. } | HirExpression::Variable { .. } => {}
        }
    }

    /// Extract a handler function name from the third argument of an
    /// `_http_route(...)` call. Supports the variable form (typed-emission) and
    /// the string-literal form (textual assemblers).
    fn handler_name_from_expr(expr: &HirExpression) -> Option<String> {
        match expr {
            HirExpression::Variable { name, .. } => Some(name.clone()),
            HirExpression::Literal {
                value: Value::String(s),
                ..
            } => Some(s.clone()),
            _ => None,
        }
    }

    /// Validate the entire program structure
    fn validate_program(context: &mut ValidationContext, hir: &HirProgram) {
        // Validate imports (basic structure check)
        for import in &hir.imports {
            Self::validate_import(context, import);
        }

        // Validate all functions
        for function in &hir.functions {
            Self::validate_function(context, function);
        }

        // Validate start function
        if let Some(start_func) = &hir.start_function {
            Self::validate_start_function(context, start_func);
        }

        // Validate all classes
        for class in &hir.classes {
            Self::validate_class(context, class);
        }

        // Validate tests
        for test in &hir.tests {
            Self::validate_test(context, test);
        }
    }

    /// Validate an import statement
    fn validate_import(context: &mut ValidationContext, import: &HirImport) {
        // Basic validation - ensure module name is not empty
        if import.module_name.is_empty() {
            context.error(
                "Import module name cannot be empty",
                import.location.clone(),
            );
        }

        // Validate specific items if present
        if let Some(items) = &import.items {
            if items.is_empty() {
                context.warning(
                    "Empty import list - consider importing entire module",
                    import.location.clone(),
                );
            }

            // Check for duplicates
            let mut seen = HashSet::new();
            for item in items {
                if !seen.insert(item) {
                    context.error_with_code(
                        &format!("Duplicate import item '{}'", item),
                        import.location.clone(),
                        "IMPORT004",
                    );
                }
            }
        }
    }

    /// Validate a function
    fn validate_function(context: &mut ValidationContext, function: &HirFunction) {
        // Set return type context
        let old_return_type = context.current_return_type.clone();
        context.current_return_type = function.return_type.clone();

        // CONC002: track whether we are inside a request handler so that uses of
        // request-context builtins (req.*, session.*, res.*) are only allowed in
        // handler bodies.
        let old_inside_handler = context.inside_request_handler;
        if context.is_request_handler(function) {
            context.inside_request_handler = true;
        }

        // Create new scope for function parameters and body
        context.push_scope();

        // Add parameters to scope
        for param in &function.parameters {
            Self::validate_parameter(context, param);
            context.declare_variable(param.name.clone(), param.param_type.clone());
        }

        // Validate function body
        Self::validate_block(context, &function.body);

        // Check if non-void function has return
        if let Some(return_type) = &function.return_type {
            if *return_type != HirType::Void && !Self::block_has_return(&function.body) {
                context.warning(
                    &format!(
                        "Function '{}' may not return a value on all paths",
                        function.name
                    ),
                    function.location.clone(),
                );
            }
        }

        // Restore context
        context.pop_scope();
        context.current_return_type = old_return_type;
        context.inside_request_handler = old_inside_handler;
    }

    /// Validate the start function
    fn validate_start_function(context: &mut ValidationContext, function: &HirFunction) {
        // Start function must have no parameters
        if !function.parameters.is_empty() {
            context.error_with_code(
                "Start function cannot have parameters",
                function.location.clone(),
                "FUNC006",
            );
        }

        // Start function return type should be void or None
        if let Some(return_type) = &function.return_type {
            if *return_type != HirType::Void {
                context.warning_with_code(
                    "Start function should return void",
                    function.location.clone(),
                    "FUNC007",
                );
            }
        }

        // Validate the body
        let old_return_type = context.current_return_type.clone();
        context.current_return_type = Some(HirType::Void);
        context.push_scope();

        Self::validate_block(context, &function.body);

        context.pop_scope();
        context.current_return_type = old_return_type;
    }

    /// Validate a class
    fn validate_class(context: &mut ValidationContext, class: &HirClass) {
        let old_class = context.current_class.clone();
        context.current_class = Some(class.name.clone());

        // Validate parent class exists if specified
        if let Some(parent_name) = &class.parent {
            if !context.classes.contains_key(parent_name) {
                // CLASS001: parent class not found
                context.error_with_code(
                    &format!("Parent class '{}' is not defined", parent_name),
                    class.location.clone(),
                    "CLASS001",
                );
            }

            // SEM008: direct or indirect circular inheritance.
            if Self::has_circular_inheritance(&context.classes, &class.name, parent_name) {
                context.error_with_code(
                    &format!("Circular inheritance detected for class '{}'", class.name),
                    class.location.clone(),
                    "SEM008",
                );
            }
        }

        // Validate fields
        let mut field_names = HashSet::new();
        for field in &class.fields {
            if !field_names.insert(&field.name) {
                // CLASS002: duplicate field in class
                context.error_with_code(
                    &format!("Duplicate field '{}' in class '{}'", field.name, class.name),
                    field.location.clone(),
                    "CLASS002",
                );
            }

            Self::validate_field(context, field);
        }

        // Validate constructor
        if let Some(constructor) = &class.constructor {
            Self::validate_constructor(context, constructor, &class.name);
        } else if !class.fields.is_empty() && class.fields.iter().any(|f| f.initializer.is_none()) {
            // CLASS004: class has uninitialized fields but declares no constructor.
            // This is a warning because the compiler may synthesize a default constructor,
            // but the programmer should be explicit about field initialization.
            context.warning_with_code(
                &format!(
                    "Class '{}' has fields without initializers but declares no constructor",
                    class.name
                ),
                class.location.clone(),
                "CLASS004",
            );
        }

        // Validate methods
        let mut method_names = HashSet::new();
        for method in &class.methods {
            if !method_names.insert(&method.name) {
                // CLASS003: duplicate method in class
                context.error_with_code(
                    &format!(
                        "Duplicate method '{}' in class '{}'",
                        method.name, class.name
                    ),
                    method.location.clone(),
                    "CLASS003",
                );
            }

            Self::validate_method(context, method);
        }

        context.current_class = old_class;
    }

    /// Check for circular inheritance
    fn has_circular_inheritance(
        classes: &HashMap<String, HirClass>,
        start_class: &str,
        current_parent: &str,
    ) -> bool {
        if current_parent == start_class {
            return true;
        }

        if let Some(parent_class) = classes.get(current_parent) {
            if let Some(grandparent) = &parent_class.parent {
                return Self::has_circular_inheritance(classes, start_class, grandparent);
            }
        }

        false
    }

    /// Validate a field
    fn validate_field(context: &mut ValidationContext, field: &HirField) {
        Self::validate_type(context, &field.field_type, &field.location);

        if let Some(initializer) = &field.initializer {
            Self::validate_expression(context, initializer);
        }
    }

    /// Validate a constructor
    fn validate_constructor(
        context: &mut ValidationContext,
        constructor: &HirConstructor,
        _class_name: &str,
    ) {
        context.push_scope();

        // `this` is available inside constructors for field assignments.
        context.declare_variable("this".to_string(), HirType::Any);

        // Add constructor parameters to scope
        for param in &constructor.parameters {
            Self::validate_parameter(context, param);
            context.declare_variable(param.name.clone(), param.param_type.clone());
        }

        // Validate constructor body
        Self::validate_block(context, &constructor.body);

        context.pop_scope();
    }

    /// Validate a method
    fn validate_method(context: &mut ValidationContext, method: &HirMethod) {
        let old_return_type = context.current_return_type.clone();
        context.current_return_type = Some(method.return_type.clone());

        context.push_scope();

        // `this` is implicitly available inside every class method; inject it so
        // explicit `this.field` access does not produce "Undefined variable 'this'".
        context.declare_variable("this".to_string(), HirType::Any);

        // Add method parameters to scope
        for param in &method.parameters {
            Self::validate_parameter(context, param);
            context.declare_variable(param.name.clone(), param.param_type.clone());
        }

        // Validate method body
        Self::validate_block(context, &method.body);

        // Check return paths for non-void methods
        if method.return_type != HirType::Void && !Self::block_has_return(&method.body) {
            context.warning(
                &format!(
                    "Method '{}' may not return a value on all paths",
                    method.name
                ),
                method.location.clone(),
            );
        }

        context.pop_scope();
        context.current_return_type = old_return_type;
    }

    /// Validate a parameter
    fn validate_parameter(context: &mut ValidationContext, param: &HirParameter) {
        Self::validate_type(context, &param.param_type, &param.location);
    }

    /// Validate a test
    fn validate_test(context: &mut ValidationContext, test: &HirTest) {
        context.push_scope();
        Self::validate_block(context, &test.body);
        context.pop_scope();
    }

    /// Validate a block of statements
    fn validate_block(context: &mut ValidationContext, block: &HirBlock) {
        // CLASS005: `ensure` must appear before any non-require/non-ensure logic.
        // Track whether we have seen a statement that is neither `require` nor `ensure`.
        let mut seen_non_contract = false;
        for statement in &block.statements {
            match statement {
                HirStatement::Require { .. } => {
                    // `require` may appear anywhere before other logic — no ordering constraint here.
                }
                HirStatement::Ensure { location, .. } => {
                    if seen_non_contract {
                        context.error_with_code(
                            "`ensure` must appear before other statements in the function body",
                            location.clone(),
                            "CLASS005",
                        );
                    }
                }
                _ => {
                    seen_non_contract = true;
                }
            }
        }

        for statement in &block.statements {
            Self::validate_statement(context, statement);
        }
    }

    /// Validate a statement
    fn validate_statement(context: &mut ValidationContext, statement: &HirStatement) {
        match statement {
            HirStatement::VariableDeclaration {
                name,
                var_type,
                initializer,
                is_mutable: _,
                location,
            } => {
                Self::validate_type(context, var_type, location);

                if let Some(init_expr) = initializer {
                    Self::validate_expression(context, init_expr);
                }

                // Check for redeclaration in current scope
                if let Some(current_scope) = context.variables.last() {
                    if current_scope.contains_key(name) {
                        context.error(
                            &format!("Variable '{}' is already declared in this scope", name),
                            location.clone(),
                        );
                    }
                }

                context.declare_variable(name.clone(), var_type.clone());
            }

            HirStatement::Assignment {
                target,
                value,
                location: _,
            } => {
                Self::validate_lvalue(context, target);
                Self::validate_expression(context, value);
            }

            HirStatement::Expression { expression, .. } => {
                Self::validate_expression(context, expression);
            }

            HirStatement::Return { value, location } => {
                if let Some(return_expr) = value {
                    Self::validate_expression(context, return_expr);
                } else if let Some(expected_type) = &context.current_return_type {
                    if *expected_type != HirType::Void {
                        // FUNC005: empty return statement in a non-void function
                        context.warning_with_code(
                            "Empty return in non-void function",
                            location.clone(),
                            "FUNC005",
                        );
                    }
                }
            }

            HirStatement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                Self::validate_expression(context, condition);

                context.push_scope();
                Self::validate_block(context, then_branch);
                context.pop_scope();

                if let Some(else_block) = else_branch {
                    context.push_scope();
                    Self::validate_block(context, else_block);
                    context.pop_scope();
                }
            }

            HirStatement::For {
                variable,
                iterable,
                body,
                ..
            } => {
                Self::validate_expression(context, iterable);

                context.push_scope();
                // Add loop variable (type will be inferred later)
                context.declare_variable(
                    variable.clone(),
                    HirType::Inferred {
                        id: 0,
                        location: body.location.clone(),
                    },
                );
                Self::validate_block(context, body);
                context.pop_scope();
            }

            HirStatement::While {
                condition, body, ..
            } => {
                Self::validate_expression(context, condition);

                context.push_scope();
                Self::validate_block(context, body);
                context.pop_scope();
            }

            HirStatement::Print { expression, .. } => {
                Self::validate_expression(context, expression);
            }

            HirStatement::LaterAssignment {
                variable,
                expression,
                location: _,
            } => {
                Self::validate_expression(context, expression);
                // Declare the variable for later use (async context)
                let inferred_type = HirType::Inferred {
                    id: 0,
                    location: expression.location().clone(),
                };
                context.declare_variable(variable.clone(), inferred_type);
            }

            HirStatement::Background {
                expression,
                location,
            } => {
                // CONC001: expressions inside a `background:` statement run asynchronously
                // in a fire-and-forget task.  Direct reads or writes of module-level mutable
                // state variables without synchronisation are a data-race hazard.
                // Flag every state-variable access found within this expression.
                let was_inside_background = context.inside_background;
                context.inside_background = true;
                Self::validate_expression(context, expression);
                context.inside_background = was_inside_background;
                let _ = location; // location used by error messages inside validate_expression
            }

            HirStatement::Break { .. } => {
                // Break statements are validated for loop context at compile time
                // No additional validation needed here
            }

            HirStatement::Continue { .. } => {
                // Continue statements are validated for loop context at compile time
                // No additional validation needed here
            }

            HirStatement::Require { condition, .. } => {
                // Validate the condition expression
                Self::validate_expression(context, condition);
            }

            HirStatement::Ensure { condition, .. } => {
                // `result` is the synthetic postcondition binding for the function's return
                // value. Inject it into a temporary scope so the expression validator does
                // not emit "Undefined variable 'result'". The resolver resolves it properly
                // during MIR lowering.
                context.push_scope();
                context.declare_variable("result".to_string(), HirType::Any);
                Self::validate_expression(context, condition);
                context.pop_scope();
            }
        }
    }

    /// Validate an expression
    fn validate_expression(context: &mut ValidationContext, expression: &HirExpression) {
        match expression {
            HirExpression::Literal { .. } => {
                // Literals are always valid
            }

            HirExpression::Variable { name, location } => {
                // CONC001: accessing a module-level mutable state variable from inside a
                // `background:` expression is unsynchronised shared-state access.
                // The background task runs concurrently with the main execution and any other
                // tasks; without a mutex or atomic operation the access is a data race.
                if context.inside_background && context.state_var_names.contains(name.as_str()) {
                    context.error_with_code(
                        &format!(
                            "Shared state variable '{}' is accessed inside a `background:` block \
                             without synchronisation — this is an unsynchronised concurrent \
                             access (CONC001). Capture the value before the `background:` call \
                             or use an atomic/mutex wrapper.",
                            name
                        ),
                        location.clone(),
                        "CONC001",
                    );
                }

                // Check if it's a variable or a function reference.
                // Builtin namespaces (string, list, math, etc.) are valid as receiver
                // expressions in method calls even though they are not declared as local
                // variables.  The resolver converts these to qualified namespace calls in
                // stage 4; at the HIR validation stage we simply skip the undefined-variable
                // check for known namespace names.
                const BUILTIN_NAMESPACES: &[&str] = &[
                    // Core language namespaces
                    "string",
                    "list",
                    "math",
                    "http",
                    "file",
                    "json",
                    "input",
                    "validator",
                    "compare",
                    "logical",
                    "conditional",
                    "integer",
                    "number",
                    "boolean",
                    // Layer 2 host bridge namespaces (registered in symbol_table.rs builtins).
                    // These are available without a plugin declaration because they are part
                    // of the host bridge contract, not plugin-specific DSL.
                    "db",
                    "crypto",
                    "jwt",
                    "env",
                    "time",
                    // fs — bytes-safe filesystem bridges (opaque handle
                    // convention). See foundation/spec/type-system.md §9b
                    // and foundation/spec/plugins/frame-server.ebnf
                    // `fs_expression`.
                    "fs",
                    // Layer 3 server namespaces (registered in symbol_table.rs builtins).
                    // These are server-only but still declared as builtins so the resolver
                    // can validate them; the host enforces server-context restrictions at runtime.
                    "req",
                    "auth",
                    "session",
                    "server",
                ];
                // Inside a class constructor or method, unqualified field names are valid
                // as expressions (the resolver adds implicit `this.` in stage 4). Walk
                // the full inheritance hierarchy so parent-class fields are accepted too.
                let is_class_field = context
                    .current_class
                    .as_deref()
                    .map(|cn| Self::class_has_field_in_hierarchy(&context.classes, cn, name))
                    .unwrap_or(false);
                if context.lookup_variable(name).is_none()
                    && !context.functions.contains_key(name)
                    && !BUILTIN_NAMESPACES.contains(&name.as_str())
                    && !context.plugin_namespaces.contains(name.as_str())
                    && !context.classes.contains_key(name)
                    && !is_class_field
                    && !Self::is_registry_alias_namespace(name)
                {
                    // SEM007: emit a context-aware message for well-known
                    // plugin-injected implicit variables so developers get
                    // actionable guidance rather than a generic error.
                    const SERVER_IMPLICIT_VARS: &[&str] = &["req", "res", "session"];
                    if SERVER_IMPLICIT_VARS.contains(&name.as_str()) {
                        context.error_with_code(
                            &format!(
                                "Undefined variable '{}' — '{}' is provided by the \
                                 frame.server plugin and is only available inside \
                                 endpoint handler bodies (e.g. `GET /path:`, \
                                 `POST /path:`). Make sure frame.server is loaded \
                                 and that you are inside a route handler.",
                                name, name
                            ),
                            location.clone(),
                            "SEM007",
                        );
                    } else {
                        context.error(&format!("Undefined variable '{}'", name), location.clone());
                    }
                }
            }

            HirExpression::BinaryOp { left, right, .. } => {
                Self::validate_expression(context, left);
                Self::validate_expression(context, right);
            }

            HirExpression::UnaryOp { operand, .. } => {
                Self::validate_expression(context, operand);
            }

            HirExpression::Call {
                function,
                arguments,
                location,
            } => {
                if context.lookup_variable(function).is_some()
                    && !context.functions.contains_key(function)
                    && !context.classes.contains_key(function)
                {
                    // FUNC003: identifier is a variable, not a callable function
                    context.error_with_code(
                        &format!("'{}' is not a function and cannot be called", function),
                        location.clone(),
                        "FUNC003",
                    );
                }
                // NOTE: FUNC001 (undefined function) is intentionally NOT checked here.
                // The HIR validator runs before name resolution (stage 4) and does not
                // have access to builtin functions, bridge functions, or class constructors.
                // The resolver and type-checker catch undefined function calls with full
                // context.  Emitting FUNC001 here would produce false positives for every
                // builtin (print, math.*, string.*, input, etc.) and class constructor call.
                if let Some(func_def) = context.functions.get(function).cloned() {
                    // FUNC002: argument count must be within [required, max] where required
                    // is the number of parameters without defaults and max is all parameters.
                    let required = func_def
                        .parameters
                        .iter()
                        .filter(|p| p.default_value.is_none())
                        .count();
                    let max_params = func_def.parameters.len();
                    let actual = arguments.len();
                    if actual < required || actual > max_params {
                        let expected_msg = if required == max_params {
                            format!("{}", required)
                        } else {
                            format!("{}-{}", required, max_params)
                        };
                        context.error_with_code(
                            &format!(
                                "Function '{}' expects {} argument(s) but {} were provided",
                                function, expected_msg, actual
                            ),
                            location.clone(),
                            "FUNC002",
                        );
                    }
                }

                for arg in arguments {
                    Self::validate_expression(context, arg);
                }
            }

            HirExpression::MethodCall {
                receiver,
                method,
                arguments,
                location,
            } => {
                // CONC002: detect `req.method(...)`, `session.method(...)`, etc. expressed
                // as MethodCall nodes (some parser paths produce these instead of NamespaceCall).
                // Inspect the receiver: if it is a plain variable reference whose name is a
                // request-context namespace and we are outside a request handler, emit CONC002.
                if !context.inside_request_handler {
                    if let HirExpression::Variable {
                        name: recv_name, ..
                    } = receiver.as_ref()
                    {
                        if Self::is_request_context_namespace(recv_name) {
                            context.error_with_code(
                                &format!(
                                    "Request-context builtin '{}.{}' is used outside a request \
                                     handler (CONC002). Request-context values are only available \
                                     inside endpoint handler functions (e.g. `GET /path:`, \
                                     `POST /path:`, or functions that receive a `Request` \
                                     parameter). Move this call into a handler or pass the needed \
                                     value as a parameter.",
                                    recv_name, method
                                ),
                                location.clone(),
                                "CONC002",
                            );
                        }
                    }
                }

                Self::validate_expression(context, receiver);

                for arg in arguments {
                    Self::validate_expression(context, arg);
                }

                // Basic method name validation (detailed type checking in Stage 5)
                if method.is_empty() {
                    context.error("Method name cannot be empty", location.clone());
                }
            }

            HirExpression::FieldAccess {
                object,
                field,
                location,
            } => {
                Self::validate_expression(context, object);

                if field.is_empty() {
                    context.error("Field name cannot be empty", location.clone());
                }
            }

            HirExpression::Index { array, index, .. } => {
                Self::validate_expression(context, array);
                Self::validate_expression(context, index);
            }

            HirExpression::Array {
                elements,
                element_type,
                location,
            } => {
                Self::validate_type(context, element_type, location);

                for element in elements {
                    Self::validate_expression(context, element);
                }
            }

            HirExpression::Constructor {
                class_name,
                arguments,
                location,
            } => {
                if !context.classes.contains_key(class_name) {
                    context.error(
                        &format!("Undefined class '{}'", class_name),
                        location.clone(),
                    );
                }

                for arg in arguments {
                    Self::validate_expression(context, arg);
                }
            }

            HirExpression::Cast {
                expression,
                target_type,
                location,
            } => {
                Self::validate_expression(context, expression);
                Self::validate_type(context, target_type, location);
            }

            HirExpression::Assignment {
                target,
                value,
                location,
            } => {
                // CONC001: writing to a module-level state variable inside a `background:`
                // expression is an unsynchronised write.  Flag it the same way as a read.
                if context.inside_background {
                    if let HirLValue::Variable {
                        name: lval_name, ..
                    } = target
                    {
                        if context.state_var_names.contains(lval_name.as_str()) {
                            context.error_with_code(
                                &format!(
                                    "Shared state variable '{}' is assigned inside a \
                                     `background:` block without synchronisation — this is \
                                     an unsynchronised concurrent write (CONC001). Use an \
                                     atomic/mutex wrapper or perform the mutation before \
                                     scheduling the background task.",
                                    lval_name
                                ),
                                location.clone(),
                                "CONC001",
                            );
                        }
                    }
                }
                Self::validate_lvalue(context, target);
                Self::validate_expression(context, value);
            }

            HirExpression::NamespaceCall {
                namespace,
                function,
                arguments,
                location,
            } => {
                // CONC002: request-context builtins (req.*, session.*, res.*) are only
                // valid inside a request handler function.  Calling them from non-handler
                // functions (e.g. utility functions, start:, tests) produces undefined
                // behaviour at runtime because no request is active in those contexts.
                if !context.inside_request_handler && Self::is_request_context_namespace(namespace)
                {
                    context.error_with_code(
                        &format!(
                            "Request-context builtin '{}.{}' is used outside a request handler \
                             (CONC002). Request-context values are only available inside \
                             endpoint handler functions (e.g. `GET /path:`, `POST /path:`, or \
                             functions that receive a `Request` parameter). Move this call into \
                             a handler or pass the needed value as a parameter.",
                            namespace, function
                        ),
                        location.clone(),
                        "CONC002",
                    );
                }
                for arg in arguments {
                    Self::validate_expression(context, arg);
                }
            }

            HirExpression::StaticMethodCall { arguments, .. } => {
                for arg in arguments {
                    Self::validate_expression(context, arg);
                }
            }

            HirExpression::OnError {
                expression,
                fallback,
                ..
            } => {
                Self::validate_expression(context, expression);
                Self::validate_expression(context, fallback);
            }

            HirExpression::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                Self::validate_expression(context, condition);
                Self::validate_expression(context, then_expr);
                Self::validate_expression(context, else_expr);
            }

            HirExpression::BaseCall { arguments, .. } => {
                // Validate arguments
                for arg in arguments {
                    Self::validate_expression(context, arg);
                }
                // Note: Validation of whether base() is used in proper context (constructor of derived class)
                // is done during resolution phase, not here
            }

            HirExpression::Range { start, end, .. } => {
                // Validate start and end expressions
                Self::validate_expression(context, start);
                Self::validate_expression(context, end);
            }

            HirExpression::ObjectLiteral { fields, .. } => {
                // Validate each field value expression. Keys are static literal
                // `Value`s (string or integer) per the grammar — no validation needed.
                for (_key, value) in fields {
                    Self::validate_expression(context, value);
                }
            }
        }
    }

    /// Validate an L-value (assignment target)
    fn validate_lvalue(context: &mut ValidationContext, lvalue: &HirLValue) {
        match lvalue {
            HirLValue::Variable { name, location } => {
                // Inside a class constructor or method, unqualified field names are valid
                // assignment targets even though they are not in the variable scope.  The
                // resolver adds the implicit `this.` prefix in stage 4; at the HIR validation
                // stage we accept any name that belongs to the current class's fields.
                let is_class_field = context
                    .current_class
                    .as_deref()
                    .map(|cn| Self::class_has_field_in_hierarchy(&context.classes, cn, name))
                    .unwrap_or(false);

                if context.lookup_variable(name).is_none() && !is_class_field {
                    context.error(&format!("Undefined variable '{}'", name), location.clone());
                }
            }

            HirLValue::FieldAccess {
                object,
                field,
                location,
            } => {
                Self::validate_expression(context, object);

                if field.is_empty() {
                    context.error("Field name cannot be empty", location.clone());
                }
            }

            HirLValue::Index { array, index, .. } => {
                Self::validate_expression(context, array);
                Self::validate_expression(context, index);
            }
        }
    }

    /// Validate a type reference
    fn validate_type(
        context: &mut ValidationContext,
        hir_type: &HirType,
        location: &SourceLocation,
    ) {
        match hir_type {
            HirType::Named { name, .. } if !context.classes.contains_key(name) => {
                context.error(&format!("Undefined type '{}'", name), location.clone());
            }
            HirType::Named { .. } => {}

            HirType::List(element_type) | HirType::Matrix(element_type) => {
                Self::validate_type(context, element_type, location);
            }

            // Primitive types and inferred types are always valid
            _ => {}
        }
    }

    /// Check if a block has a return statement
    fn block_has_return(block: &HirBlock) -> bool {
        for statement in &block.statements {
            match statement {
                HirStatement::Return { .. } => return true,

                HirStatement::If {
                    then_branch,
                    else_branch: Some(else_block),
                    ..
                } if Self::block_has_return(then_branch) && Self::block_has_return(else_block) => {
                    return true;
                }
                HirStatement::If { .. } => {}

                _ => {}
            }
        }
        false
    }

    /// Return `true` when `namespace` is a request-context namespace.
    ///
    /// These namespaces (`req`, `res`, `session`, `auth`) provide access to live
    /// HTTP request/response objects and are only valid inside a request handler.
    /// Using them elsewhere (start:, utility functions, background tasks, tests) is
    /// a CONC002 violation: no active request context exists at those call sites.
    fn is_request_context_namespace(namespace: &str) -> bool {
        matches!(namespace, "req" | "res" | "session" | "auth")
    }

    /// Check whether `namespace` is the receiver-prefix of a registry-declared
    /// bridge alias (e.g. `dev` from `_dev_snapshot`'s `aliases = ["dev.snapshot"]`).
    /// Used at validation time so that a namespace call like `dev.snapshot()`
    /// reaches the resolver instead of being rejected as an undefined
    /// variable — the resolver then registers the bridge on demand.
    ///
    /// Consults the embedded `function-registry.toml`. Load failures return
    /// false: a missing registry surfaces separately through the
    /// registry-loader validation path, and this check is only a permissive
    /// gate before the resolver runs.
    fn is_registry_alias_namespace(namespace: &str) -> bool {
        static CACHE: std::sync::OnceLock<std::collections::HashSet<String>> =
            std::sync::OnceLock::new();
        let prefixes = CACHE.get_or_init(|| {
            let mut set = std::collections::HashSet::new();
            if let Ok(idx) = crate::plugins::registry_loader::RegistryIndex::load() {
                for reg_fn in idx.functions() {
                    for alias in &reg_fn.aliases {
                        if let Some(dot_pos) = alias.find('.') {
                            set.insert(alias[..dot_pos].to_string());
                        }
                    }
                }
            }
            set
        });
        prefixes.contains(namespace)
    }

    /// Walk the class inheritance chain to check whether `field_name` is declared
    /// in `class_name` or any of its ancestors. Guards against inheritance cycles.
    fn class_has_field_in_hierarchy(
        classes: &HashMap<String, HirClass>,
        class_name: &str,
        field_name: &str,
    ) -> bool {
        let mut current = class_name.to_string();
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                break; // cycle guard
            }
            if let Some(class_def) = classes.get(&current) {
                if class_def.fields.iter().any(|f| f.name == field_name) {
                    return true;
                }
                match class_def.parent.clone() {
                    Some(parent) => current = parent,
                    None => break,
                }
            } else {
                break;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test location
    fn test_location() -> SourceLocation {
        SourceLocation {
            file: "test.cln".to_string(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        }
    }

    #[test]
    fn test_valid_program() {
        let program = HirProgram {
            functions: vec![HirFunction {
                name: "add".to_string(),
                parameters: vec![
                    HirParameter {
                        name: "a".to_string(),
                        param_type: HirType::Integer,
                        default_value: None,
                        location: test_location(),
                    },
                    HirParameter {
                        name: "b".to_string(),
                        param_type: HirType::Integer,
                        default_value: None,
                        location: test_location(),
                    },
                ],
                return_type: Some(HirType::Integer),
                body: HirBlock {
                    statements: vec![HirStatement::Return {
                        value: Some(HirExpression::BinaryOp {
                            left: Box::new(HirExpression::Variable {
                                name: "a".to_string(),
                                location: test_location(),
                            }),
                            op: HirBinaryOp::Add,
                            right: Box::new(HirExpression::Variable {
                                name: "b".to_string(),
                                location: test_location(),
                            }),
                            location: test_location(),
                        }),
                        location: test_location(),
                    }],
                    location: test_location(),
                },
                is_start: false,
                is_private: false,
                owner_screen: None,
                location: test_location(),
            }],
            classes: vec![],
            start_function: None,
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: test_location(),
        };

        let result = HirValidator::validate(&program);
        assert!(result.is_ok(), "Valid program should pass validation");
    }

    #[test]
    fn test_undefined_variable() {
        let program = HirProgram {
            functions: vec![HirFunction {
                name: "test".to_string(),
                parameters: vec![],
                return_type: Some(HirType::Integer),
                body: HirBlock {
                    statements: vec![HirStatement::Return {
                        value: Some(HirExpression::Variable {
                            name: "undefined_var".to_string(),
                            location: test_location(),
                        }),
                        location: test_location(),
                    }],
                    location: test_location(),
                },
                is_start: false,
                is_private: false,
                owner_screen: None,
                location: test_location(),
            }],
            classes: vec![],
            start_function: None,
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: test_location(),
        };

        let result = HirValidator::validate(&program);
        assert!(
            result.is_err(),
            "Program with undefined variable should fail"
        );

        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors[0]
            .message()
            .contains("Undefined variable 'undefined_var'"));
    }

    #[test]
    fn test_duplicate_function() {
        let program = HirProgram {
            functions: vec![
                HirFunction {
                    name: "duplicate".to_string(),
                    parameters: vec![],
                    return_type: Some(HirType::Void),
                    body: HirBlock {
                        statements: vec![],
                        location: test_location(),
                    },
                    is_start: false,
                    is_private: false,
                    owner_screen: None,
                    location: test_location(),
                },
                HirFunction {
                    name: "duplicate".to_string(),
                    parameters: vec![],
                    return_type: Some(HirType::Integer),
                    body: HirBlock {
                        statements: vec![],
                        location: test_location(),
                    },
                    is_start: false,
                    is_private: false,
                    owner_screen: None,
                    location: test_location(),
                },
            ],
            classes: vec![],
            start_function: None,
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: test_location(),
        };

        let result = HirValidator::validate(&program);
        assert!(
            result.is_err(),
            "Program with duplicate function should fail"
        );

        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors[0].message().contains("already defined"));
    }

    #[test]
    fn test_class_inheritance() {
        let program = HirProgram {
            functions: vec![],
            classes: vec![
                HirClass {
                    name: "Parent".to_string(),
                    parent: None,
                    fields: vec![],
                    constructor: None,
                    methods: vec![],
                    invariants: vec![],
                    type_parameters: vec![],
                    location: test_location(),
                },
                HirClass {
                    name: "Child".to_string(),
                    parent: Some("Parent".to_string()),
                    fields: vec![],
                    constructor: None,
                    methods: vec![],
                    invariants: vec![],
                    type_parameters: vec![],
                    location: test_location(),
                },
            ],
            start_function: None,
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: test_location(),
        };

        let result = HirValidator::validate(&program);
        assert!(result.is_ok(), "Valid inheritance should pass validation");
    }

    #[test]
    fn test_circular_inheritance() {
        let program = HirProgram {
            functions: vec![],
            classes: vec![
                HirClass {
                    name: "A".to_string(),
                    parent: Some("B".to_string()),
                    fields: vec![],
                    constructor: None,
                    methods: vec![],
                    invariants: vec![],
                    type_parameters: vec![],
                    location: test_location(),
                },
                HirClass {
                    name: "B".to_string(),
                    parent: Some("A".to_string()),
                    fields: vec![],
                    constructor: None,
                    methods: vec![],
                    invariants: vec![],
                    type_parameters: vec![],
                    location: test_location(),
                },
            ],
            start_function: None,
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: test_location(),
        };

        let result = HirValidator::validate(&program);
        assert!(
            result.is_err(),
            "Circular inheritance should fail validation"
        );

        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.message().contains("Circular inheritance")));
    }
}

#[cfg(test)]
mod class005_tests {
    use super::*;
    use crate::hir::hir_builder::HirBuilder;
    use crate::lexer::specification_lexer::{SourceCode, SpecificationLexer};
    use crate::parser::SpecificationParser;

    fn build_hir(source: &str) -> crate::hir::HirProgram {
        let source_code = SourceCode::new(source.to_string(), "test.cln".to_string());
        let mut lexer = SpecificationLexer::new(&source_code);
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = SpecificationParser::new(tokens, "test.cln".to_string());
        let ast = parser.parse_program().expect("parse failed");
        let mut hir_builder = HirBuilder::new();
        hir_builder.build_hir(ast).expect("hir build failed").hir
    }

    /// CLASS005: `ensure` appearing after non-contract logic must be rejected.
    #[test]
    fn test_class005_ensure_after_logic_is_error() {
        let source = concat!(
            "start:\n",
            "\tprint(divide(10, 2).toString())\n",
            "\nfunctions:\n",
            "\tinteger divide(integer a, integer b)\n",
            "\t\trequire b != 0\n",
            "\t\tinteger result = a / b\n",
            "\t\tensure result * b == a\n",
            "\t\treturn result\n",
        );
        let hir = build_hir(source);
        let result = HirValidator::validate(&hir);
        assert!(
            result.is_err(),
            "CLASS005: expected error when ensure appears after variable declaration"
        );
        let errors = result.unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.to_string().contains("CLASS005") || e.to_string().contains("ensure")),
            "Expected CLASS005 error, got: {:?}",
            errors
        );
    }

    /// CLASS005 negative: `ensure` before any logic is valid.
    #[test]
    fn test_class005_ensure_at_top_is_valid() {
        let source = concat!(
            "start:\n",
            "\tprint(\"ok\")\n",
            "\nfunctions:\n",
            "\tinteger double(integer x)\n",
            "\t\trequire x >= 0\n",
            "\t\tensure result > 0\n",
            "\t\treturn x * 2\n",
        );
        let hir = build_hir(source);
        // ensure comes before the return statement — this is valid ordering
        let result = HirValidator::validate(&hir);
        // The result may fail for other reasons (undefined 'result' variable) but
        // must NOT fail specifically because of CLASS005 ensure ordering.
        if let Err(ref errors) = result {
            for e in errors {
                assert!(
                    !e.to_string().contains("CLASS005"),
                    "Unexpected CLASS005 error when ensure is correctly placed: {:?}",
                    e
                );
            }
        }
    }
}

/// Unit tests for CONC001 (unsynchronised shared-state access from background) and
/// CONC002 (request-context builtins used outside a request handler).
#[cfg(test)]
mod concurrency_tests {
    use super::*;

    fn loc() -> SourceLocation {
        SourceLocation {
            file: "test.cln".to_string(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        }
    }

    /// Build the minimal HirProgram structure shared across tests:
    /// a program with one module-level state variable `counter` of type `integer`.
    fn program_with_state(functions: Vec<HirFunction>, start: Option<HirFunction>) -> HirProgram {
        HirProgram {
            functions,
            classes: vec![],
            start_function: start,
            imports: vec![],
            tests: vec![],
            state: Some(HirStateBlock {
                declarations: vec![HirStateDeclaration {
                    name: "counter".to_string(),
                    state_type: HirType::Integer,
                    initializer: HirExpression::Literal {
                        value: crate::ast::Value::Integer(0),
                        location: loc(),
                    },
                    guard: None,
                    is_private: false,
                    location: loc(),
                }],
                computed: vec![],
                rules: vec![],
                scope: HirStateScope::App,
                location: loc(),
            }),
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: loc(),
        }
    }

    /// CONC001 positive: a `background:` statement that reads a state variable directly
    /// must be rejected with a CONC001 error.
    #[test]
    fn test_conc001_background_reads_state_var() {
        // Simulate: background someFunc(counter)
        // `counter` is a state variable — reading it inside background is CONC001.
        let bg_stmt = HirStatement::Background {
            expression: HirExpression::Call {
                function: "someFunc".to_string(),
                arguments: vec![HirExpression::Variable {
                    name: "counter".to_string(),
                    location: loc(),
                }],
                location: loc(),
            },
            location: loc(),
        };

        // Register someFunc so it is known and doesn't produce FUNC002 noise.
        let some_func = HirFunction {
            name: "someFunc".to_string(),
            parameters: vec![HirParameter {
                name: "x".to_string(),
                param_type: HirType::Integer,
                default_value: None,
                location: loc(),
            }],
            return_type: Some(HirType::Void),
            body: HirBlock {
                statements: vec![],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };

        let start = HirFunction {
            name: "__start".to_string(),
            parameters: vec![],
            return_type: Some(HirType::Void),
            body: HirBlock {
                statements: vec![bg_stmt],
                location: loc(),
            },
            is_start: true,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };

        let program = program_with_state(vec![some_func], Some(start));
        let result = HirValidator::validate(&program);
        assert!(
            result.is_err(),
            "CONC001: background accessing state var should fail"
        );
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| e.to_string().contains("CONC001")),
            "Expected CONC001 error, got: {:?}",
            errors
        );
    }

    /// CONC001 negative: a `background:` statement that does NOT access state variables
    /// must pass without error.
    #[test]
    fn test_conc001_background_no_state_var_is_valid() {
        // Simulate: background someFunc(42) — 42 is a literal, not a state var
        let bg_stmt = HirStatement::Background {
            expression: HirExpression::Call {
                function: "someFunc".to_string(),
                arguments: vec![HirExpression::Literal {
                    value: crate::ast::Value::Integer(42),
                    location: loc(),
                }],
                location: loc(),
            },
            location: loc(),
        };

        let some_func = HirFunction {
            name: "someFunc".to_string(),
            parameters: vec![HirParameter {
                name: "x".to_string(),
                param_type: HirType::Integer,
                default_value: None,
                location: loc(),
            }],
            return_type: Some(HirType::Void),
            body: HirBlock {
                statements: vec![],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };

        let start = HirFunction {
            name: "__start".to_string(),
            parameters: vec![],
            return_type: Some(HirType::Void),
            body: HirBlock {
                statements: vec![bg_stmt],
                location: loc(),
            },
            is_start: true,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };

        let program = program_with_state(vec![some_func], Some(start));
        let result = HirValidator::validate(&program);
        // May have other unrelated warnings but must not have CONC001.
        if let Err(ref errors) = result {
            assert!(
                !errors.iter().any(|e| e.to_string().contains("CONC001")),
                "Unexpected CONC001 when no state var is accessed: {:?}",
                errors
            );
        }
    }

    /// CONC002 positive: a `NamespaceCall` on `req` outside a request handler must
    /// be rejected with a CONC002 error.
    #[test]
    fn test_conc002_req_namespace_call_outside_handler() {
        // Simulate: req.param("id") inside a regular (non-handler) function.
        let req_call = HirExpression::NamespaceCall {
            namespace: "req".to_string(),
            function: "param".to_string(),
            arguments: vec![HirExpression::Literal {
                value: crate::ast::Value::String("id".to_string()),
                location: loc(),
            }],
            location: loc(),
        };

        let utility_fn = HirFunction {
            name: "utilFn".to_string(),
            parameters: vec![],
            return_type: Some(HirType::String),
            body: HirBlock {
                statements: vec![HirStatement::Return {
                    value: Some(req_call),
                    location: loc(),
                }],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };

        let program = HirProgram {
            functions: vec![utility_fn],
            classes: vec![],
            start_function: None,
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: loc(),
        };
        let result = HirValidator::validate(&program);
        assert!(
            result.is_err(),
            "CONC002: req call outside handler should fail"
        );
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| e.to_string().contains("CONC002")),
            "Expected CONC002 error, got: {:?}",
            errors
        );
    }

    /// CONC002 negative: a `NamespaceCall` on `req` inside a function identified as a
    /// request handler (has a parameter named `req`) must NOT produce a CONC002 error.
    #[test]
    fn test_conc002_req_call_inside_handler_is_valid() {
        // A function with a parameter named `req` is a request handler.
        let req_call = HirExpression::NamespaceCall {
            namespace: "req".to_string(),
            function: "param".to_string(),
            arguments: vec![HirExpression::Literal {
                value: crate::ast::Value::String("id".to_string()),
                location: loc(),
            }],
            location: loc(),
        };

        let handler_fn = HirFunction {
            name: "handleRequest".to_string(),
            parameters: vec![HirParameter {
                name: "req".to_string(),
                param_type: HirType::Any,
                default_value: None,
                location: loc(),
            }],
            return_type: Some(HirType::String),
            body: HirBlock {
                statements: vec![HirStatement::Return {
                    value: Some(req_call),
                    location: loc(),
                }],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };

        let program = HirProgram {
            functions: vec![handler_fn],
            classes: vec![],
            start_function: None,
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: loc(),
        };
        let result = HirValidator::validate(&program);
        if let Err(ref errors) = result {
            assert!(
                !errors.iter().any(|e| e.to_string().contains("CONC002")),
                "Unexpected CONC002 inside a legitimate request handler: {:?}",
                errors
            );
        }
    }

    /// CONC002 negative: a function whose name starts with `__route_handler_` is
    /// automatically identified as a request handler.
    #[test]
    fn test_conc002_route_handler_prefix_is_valid() {
        let session_call = HirExpression::NamespaceCall {
            namespace: "session".to_string(),
            function: "get".to_string(),
            arguments: vec![],
            location: loc(),
        };

        let route_handler = HirFunction {
            name: "__route_handler_0".to_string(),
            parameters: vec![],
            return_type: Some(HirType::String),
            body: HirBlock {
                statements: vec![HirStatement::Return {
                    value: Some(session_call),
                    location: loc(),
                }],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };

        let program = HirProgram {
            functions: vec![route_handler],
            classes: vec![],
            start_function: None,
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: loc(),
        };
        let result = HirValidator::validate(&program);
        if let Err(ref errors) = result {
            assert!(
                !errors.iter().any(|e| e.to_string().contains("CONC002")),
                "Unexpected CONC002 inside a route handler: {:?}",
                errors
            );
        }
    }

    /// Verify the helper `function_is_request_handler` for all three detection paths.
    #[test]
    fn test_function_is_request_handler_detection() {
        let route_fn = HirFunction {
            name: "__route_handler_5".to_string(),
            parameters: vec![],
            return_type: None,
            body: HirBlock {
                statements: vec![],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };
        assert!(
            ValidationContext::function_is_request_handler(&route_fn),
            "Route handler prefix must be detected"
        );

        let req_param_fn = HirFunction {
            name: "myHandler".to_string(),
            parameters: vec![HirParameter {
                name: "req".to_string(),
                param_type: HirType::Any,
                default_value: None,
                location: loc(),
            }],
            return_type: None,
            body: HirBlock {
                statements: vec![],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };
        assert!(
            ValidationContext::function_is_request_handler(&req_param_fn),
            "Parameter named 'req' must be detected"
        );

        let request_type_fn = HirFunction {
            name: "anotherHandler".to_string(),
            parameters: vec![HirParameter {
                name: "r".to_string(),
                param_type: HirType::Named {
                    name: "Request".to_string(),
                    location: loc(),
                },
                default_value: None,
                location: loc(),
            }],
            return_type: None,
            body: HirBlock {
                statements: vec![],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };
        assert!(
            ValidationContext::function_is_request_handler(&request_type_fn),
            "Parameter of type 'Request' must be detected"
        );

        let non_handler = HirFunction {
            name: "utilityFn".to_string(),
            parameters: vec![HirParameter {
                name: "x".to_string(),
                param_type: HirType::Integer,
                default_value: None,
                location: loc(),
            }],
            return_type: None,
            body: HirBlock {
                statements: vec![],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };
        assert!(
            !ValidationContext::function_is_request_handler(&non_handler),
            "Regular function must NOT be detected as handler"
        );
    }

    // --- CONC002: plugin-emitted `_http_route` handler recognition ---------
    //
    // These tests cover the false-positive filed against cln 0.33.71: a
    // function registered by a plugin via `_http_route(method, path, handler)`
    // is a legitimate request-context site, so `req.*`, `session.*`, `res.*`
    // calls inside it must not raise CONC002. Prior to the fix,
    // `function_is_request_handler` only looked at parameter names / types /
    // `__route_handler_` prefix, so plugin-emitted handlers (which carry
    // plugin-chosen names like `__debug_capture_handler` and no `req` param)
    // were rejected.

    /// Build a `start:` function that contains a single `_http_route` call
    /// registering `handler_name` for `GET "/path"`.
    fn start_registering_route(handler_name: &str, protected: bool) -> HirFunction {
        let mut args = vec![
            HirExpression::Literal {
                value: crate::ast::Value::String("GET".to_string()),
                location: loc(),
            },
            HirExpression::Literal {
                value: crate::ast::Value::String("/path".to_string()),
                location: loc(),
            },
            HirExpression::Variable {
                name: handler_name.to_string(),
                location: loc(),
            },
        ];
        if protected {
            args.push(HirExpression::Literal {
                value: crate::ast::Value::String("admin".to_string()),
                location: loc(),
            });
        }
        let route_call = HirStatement::Expression {
            expression: HirExpression::Call {
                function: if protected {
                    "_http_route_protected".to_string()
                } else {
                    "_http_route".to_string()
                },
                arguments: args,
                location: loc(),
            },
            location: loc(),
        };
        HirFunction {
            name: "__start".to_string(),
            parameters: vec![],
            return_type: Some(HirType::Void),
            body: HirBlock {
                statements: vec![route_call],
                location: loc(),
            },
            is_start: true,
            is_private: false,
            owner_screen: None,
            location: loc(),
        }
    }

    /// Build a function whose body returns `namespace.method("id")`.
    /// Used to fabricate plugin-emitted handlers with a chosen name.
    fn handler_returning_namespace_call(name: &str, namespace: &str, method: &str) -> HirFunction {
        HirFunction {
            name: name.to_string(),
            parameters: vec![],
            return_type: Some(HirType::String),
            body: HirBlock {
                statements: vec![HirStatement::Return {
                    value: Some(HirExpression::NamespaceCall {
                        namespace: namespace.to_string(),
                        function: method.to_string(),
                        arguments: vec![HirExpression::Literal {
                            value: crate::ast::Value::String("id".to_string()),
                            location: loc(),
                        }],
                        location: loc(),
                    }),
                    location: loc(),
                }],
                location: loc(),
            },
            is_start: false,
            is_private: false,
            owner_screen: None,
            location: loc(),
        }
    }

    /// CONC002 negative: a plugin-emitted handler `__debug_capture_handler`
    /// registered via `_http_route("GET", "/path", __debug_capture_handler)`
    /// is recognised as a request-context site.
    /// `req.param(...)` inside it must NOT raise CONC002.
    #[test]
    fn test_conc002_plugin_registered_handler_variable_form() {
        let handler = handler_returning_namespace_call("__debug_capture_handler", "req", "param");
        let start = start_registering_route("__debug_capture_handler", false);

        let program = HirProgram {
            functions: vec![handler],
            classes: vec![],
            start_function: Some(start),
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: loc(),
        };
        let result = HirValidator::validate(&program);
        if let Err(ref errors) = result {
            assert!(
                !errors.iter().any(|e| e.to_string().contains("CONC002")),
                "Unexpected CONC002 inside a plugin-registered handler: {:?}",
                errors
            );
        }
    }

    /// CONC002 negative: same as above but with `_http_route_protected` and
    /// a handler using `req.body`, `req.header`, and `req.method`.
    #[test]
    fn test_conc002_plugin_registered_handler_protected_various_builtins() {
        // Handler body: return req.body(); ignoring header/method by
        // constructing individual test functions instead of chaining, to keep
        // the HIR simple.
        for method in ["body", "header", "method"] {
            let handler = handler_returning_namespace_call("__protected_handler", "req", method);
            let start = start_registering_route("__protected_handler", true);
            let program = HirProgram {
                functions: vec![handler],
                classes: vec![],
                start_function: Some(start),
                imports: vec![],
                tests: vec![],
                state: None,
                watch_blocks: vec![],
                externals: vec![],
                screen_blocks: vec![],
                location: loc(),
            };
            let result = HirValidator::validate(&program);
            if let Err(ref errors) = result {
                assert!(
                    !errors.iter().any(|e| e.to_string().contains("CONC002")),
                    "Unexpected CONC002 for req.{} inside protected handler: {:?}",
                    method,
                    errors
                );
            }
        }
    }

    /// CONC002 negative: the string-literal form used by textual assemblers
    /// (`_http_route("GET", "/path", "handlerName")`) also registers the
    /// handler correctly.
    #[test]
    fn test_conc002_plugin_registered_handler_string_literal_form() {
        let handler = handler_returning_namespace_call("__page_handler_home", "req", "param");

        // start: registers the handler by string-literal name
        let route_call = HirStatement::Expression {
            expression: HirExpression::Call {
                function: "_http_route".to_string(),
                arguments: vec![
                    HirExpression::Literal {
                        value: crate::ast::Value::String("GET".to_string()),
                        location: loc(),
                    },
                    HirExpression::Literal {
                        value: crate::ast::Value::String("/".to_string()),
                        location: loc(),
                    },
                    HirExpression::Literal {
                        value: crate::ast::Value::String("__page_handler_home".to_string()),
                        location: loc(),
                    },
                ],
                location: loc(),
            },
            location: loc(),
        };
        let start = HirFunction {
            name: "__start".to_string(),
            parameters: vec![],
            return_type: Some(HirType::Void),
            body: HirBlock {
                statements: vec![route_call],
                location: loc(),
            },
            is_start: true,
            is_private: false,
            owner_screen: None,
            location: loc(),
        };

        let program = HirProgram {
            functions: vec![handler],
            classes: vec![],
            start_function: Some(start),
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: loc(),
        };
        let result = HirValidator::validate(&program);
        if let Err(ref errors) = result {
            assert!(
                !errors.iter().any(|e| e.to_string().contains("CONC002")),
                "Unexpected CONC002 inside string-literal-registered handler: {:?}",
                errors
            );
        }
    }

    /// CONC002 positive regression: an unrelated function that uses
    /// `req.param` but is NOT registered by any `_http_route` call must still
    /// raise CONC002. This protects against turning the check off wholesale.
    #[test]
    fn test_conc002_unregistered_function_still_rejected() {
        let bogus = handler_returning_namespace_call("randomUtility", "req", "param");
        // A `_http_route` call registers a DIFFERENT function; `randomUtility`
        // is not the third arg anywhere.
        let start = start_registering_route("__actual_handler", false);
        let real_handler = handler_returning_namespace_call("__actual_handler", "req", "param");
        let program = HirProgram {
            functions: vec![bogus, real_handler],
            classes: vec![],
            start_function: Some(start),
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![],
            location: loc(),
        };
        let result = HirValidator::validate(&program);
        assert!(
            result.is_err(),
            "CONC002 must still reject req.param inside an unregistered function"
        );
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| e.to_string().contains("CONC002")),
            "Expected CONC002 error on the unregistered function, got: {:?}",
            errors
        );
    }
}
