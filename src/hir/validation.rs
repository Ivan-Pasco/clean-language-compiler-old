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

        // First pass: collect all function and class definitions
        Self::collect_definitions(&mut context, hir);

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
                context.error(
                    &format!("Function '{}' is already defined", function.name),
                    function.location.clone(),
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
                context.error(
                    &format!(
                        "Function '{}' conflicts with start function",
                        start_func.name
                    ),
                    start_func.location.clone(),
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
                context.error(
                    &format!("Class '{}' is already defined", class.name),
                    class.location.clone(),
                );
            } else {
                context.classes.insert(class.name.clone(), class.clone());
            }
        }

        // Collect top-level state variables into global scope so expressions
        // referencing them do not produce spurious "Undefined variable" errors.
        // (SCOPE005 enforcement is handled later by the resolver.)
        if let Some(ref state_block) = hir.state {
            for decl in &state_block.declarations {
                context.declare_variable(decl.name.clone(), decl.state_type.clone());
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
            }
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

            // Check for circular inheritance (SEM008: InheritanceCycle)
            if Self::has_circular_inheritance(&context.classes, &class.name, parent_name) {
                context.error(
                    &format!("Circular inheritance detected for class '{}'", class.name),
                    class.location.clone(),
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
                // Validate the postcondition expression.
                // `result` is a special identifier in postconditions that refers to the
                // function's return value. It is resolved during MIR lowering and does not
                // need to be in the current variable scope for HIR validation.
                Self::validate_expression(context, condition);
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
                // Check if it's a variable or a function reference.
                // Builtin namespaces (string, list, math, etc.) are valid as receiver
                // expressions in method calls even though they are not declared as local
                // variables.  The resolver converts these to qualified namespace calls in
                // stage 4; at the HIR validation stage we simply skip the undefined-variable
                // check for known namespace names.
                const BUILTIN_NAMESPACES: &[&str] = &[
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
                ];
                // Inside a class constructor or method, unqualified field names are valid
                // as expressions (the resolver adds implicit `this.` in stage 4).
                let is_class_field = if let Some(ref class_name) = context.current_class {
                    if let Some(class_def) = context.classes.get(class_name) {
                        class_def.fields.iter().any(|f| f.name == *name)
                    } else {
                        false
                    }
                } else {
                    false
                };
                if context.lookup_variable(name).is_none()
                    && !context.functions.contains_key(name)
                    && !BUILTIN_NAMESPACES.contains(&name.as_str())
                    && !context.classes.contains_key(name)
                    && !is_class_field
                {
                    context.error(&format!("Undefined variable '{}'", name), location.clone());
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
                if let Some(func_def) = context.functions.get(function) {
                    // FUNC002: argument count must match the declared parameter count
                    let expected = func_def.parameters.len();
                    let actual = arguments.len();
                    if actual != expected {
                        context.error_with_code(
                            &format!(
                                "Function '{}' expects {} argument(s) but {} were provided",
                                function, expected, actual
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

            HirExpression::Assignment { target, value, .. } => {
                Self::validate_lvalue(context, target);
                Self::validate_expression(context, value);
            }

            HirExpression::NamespaceCall { arguments, .. } => {
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
                let is_class_field = if let Some(ref class_name) = context.current_class {
                    if let Some(class_def) = context.classes.get(class_name) {
                        class_def.fields.iter().any(|f| f.name == *name)
                    } else {
                        false
                    }
                } else {
                    false
                };

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
            HirType::Named { name, .. } => {
                if !context.classes.contains_key(name) {
                    context.error(&format!("Undefined type '{}'", name), location.clone());
                }
            }

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
                    else_branch,
                    ..
                } => {
                    if Self::block_has_return(then_branch) {
                        if let Some(else_block) = else_branch {
                            if Self::block_has_return(else_block) {
                                return true;
                            }
                        }
                    }
                }

                _ => {}
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
                    location: test_location(),
                },
                HirClass {
                    name: "Child".to_string(),
                    parent: Some("Parent".to_string()),
                    fields: vec![],
                    constructor: None,
                    methods: vec![],
                    invariants: vec![],
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
                    location: test_location(),
                },
                HirClass {
                    name: "B".to_string(),
                    parent: Some("A".to_string()),
                    fields: vec![],
                    constructor: None,
                    methods: vec![],
                    invariants: vec![],
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
