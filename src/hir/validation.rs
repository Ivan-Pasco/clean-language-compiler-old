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

    /// Add a warning
    pub fn warning(&mut self, message: &str, location: SourceLocation) {
        self.warnings
            .push(CompilerError::validation_warning(message, location));
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
                    context.error(
                        &format!("Duplicate import item '{}'", item),
                        import.location.clone(),
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
            context.error(
                "Start function cannot have parameters",
                function.location.clone(),
            );
        }

        // Start function return type should be void or None
        if let Some(return_type) = &function.return_type {
            if *return_type != HirType::Void {
                context.warning(
                    "Start function should return void",
                    function.location.clone(),
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
                context.error(
                    &format!("Parent class '{}' is not defined", parent_name),
                    class.location.clone(),
                );
            }

            // Check for circular inheritance
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
                context.error(
                    &format!("Duplicate field '{}' in class '{}'", field.name, class.name),
                    field.location.clone(),
                );
            }

            Self::validate_field(context, field);
        }

        // Validate constructor
        if let Some(constructor) = &class.constructor {
            Self::validate_constructor(context, constructor, &class.name);
        }

        // Validate methods
        let mut method_names = HashSet::new();
        for method in &class.methods {
            if !method_names.insert(&method.name) {
                context.error(
                    &format!(
                        "Duplicate method '{}' in class '{}'",
                        method.name, class.name
                    ),
                    method.location.clone(),
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
                        context.warning("Empty return in non-void function", location.clone());
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

            HirStatement::While {
                condition, body, ..
            } => {
                Self::validate_expression(context, condition);

                context.push_scope();
                Self::validate_block(context, body);
                context.pop_scope();
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
        }
    }

    /// Validate an expression
    fn validate_expression(context: &mut ValidationContext, expression: &HirExpression) {
        match expression {
            HirExpression::Literal { .. } => {
                // Literals are always valid
            }

            HirExpression::Variable { name, location } => {
                if context.lookup_variable(name).is_none() {
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
                if !context.functions.contains_key(function) {
                    context.error(
                        &format!("Undefined function '{}'", function),
                        location.clone(),
                    );
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

            HirExpression::This { location } => {
                if context.current_class.is_none() {
                    context.error(
                        "'this' can only be used inside a class method or constructor",
                        location.clone(),
                    );
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
        }
    }

    /// Validate an L-value (assignment target)
    fn validate_lvalue(context: &mut ValidationContext, lvalue: &HirLValue) {
        match lvalue {
            HirLValue::Variable { name, location } => {
                if context.lookup_variable(name).is_none() {
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
                location: test_location(),
            }],
            classes: vec![],
            start_function: None,
            imports: vec![],
            tests: vec![],
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
                location: test_location(),
            }],
            classes: vec![],
            start_function: None,
            imports: vec![],
            tests: vec![],
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
                    location: test_location(),
                },
            ],
            classes: vec![],
            start_function: None,
            imports: vec![],
            tests: vec![],
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
                    location: test_location(),
                },
                HirClass {
                    name: "Child".to_string(),
                    parent: Some("Parent".to_string()),
                    fields: vec![],
                    constructor: None,
                    methods: vec![],
                    location: test_location(),
                },
            ],
            start_function: None,
            imports: vec![],
            tests: vec![],
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
                    location: test_location(),
                },
                HirClass {
                    name: "B".to_string(),
                    parent: Some("A".to_string()),
                    fields: vec![],
                    constructor: None,
                    methods: vec![],
                    location: test_location(),
                },
            ],
            start_function: None,
            imports: vec![],
            tests: vec![],
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
