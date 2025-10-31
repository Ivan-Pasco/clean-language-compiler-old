use crate::ast::{
    BinaryOperator, Expression, Function, Program, SourceLocation, Statement, Type, UnaryOperator,
    Value,
};
use crate::semantic::constraints::{Constraint, ConstraintSet, ConstraintType, TypeProperty};
use crate::semantic::type_variables::TypeVariableManager;
use std::collections::HashMap;

/// Generator for type constraints from Clean Language AST
pub struct ConstraintGenerator {
    /// Type variable manager
    type_vars: TypeVariableManager,
    /// Current constraint set being built
    constraints: ConstraintSet,
    /// Current function return type (for return statements)
    current_function_return: Option<ConstraintType>,
    /// Variable type environment
    variable_types: HashMap<String, ConstraintType>,
    /// Function type environment  
    function_types: HashMap<String, ConstraintType>,
    /// Class type environment
    class_types: HashMap<String, ClassTypeInfo>,
    /// Current class context (for method calls)
    current_class: Option<String>,
}

/// Information about a class type
#[derive(Debug, Clone)]
pub struct ClassTypeInfo {
    pub name: String,
    pub fields: HashMap<String, ConstraintType>,
    pub methods: HashMap<String, ConstraintType>,
    pub constructor: Option<ConstraintType>,
    pub base_class: Option<String>,
}

impl ConstraintGenerator {
    pub fn new() -> Self {
        let mut generator = Self {
            type_vars: TypeVariableManager::new(),
            constraints: ConstraintSet::new(),
            current_function_return: None,
            variable_types: HashMap::new(),
            function_types: HashMap::new(),
            class_types: HashMap::new(),
            current_class: None,
        };

        // Add built-in types and functions
        generator.add_builtin_types();
        generator
    }

    /// Generate constraints for a complete program
    pub fn generate_program_constraints(
        &mut self,
        program: &Program,
    ) -> Result<ConstraintSet, String> {
        // First pass: collect class and function signatures
        self.collect_declarations(program)?;

        // Second pass: generate constraints for function bodies
        for function in &program.functions {
            self.generate_function_constraints(function)?;
        }

        // Handle start function specially
        if let Some(start_function) = &program.start_function {
            let old_return = self.current_function_return.clone();
            self.current_function_return = Some(ConstraintType::Concrete(Type::Void));

            for statement in &start_function.body {
                self.generate_statement_constraints(statement)?;
            }

            self.current_function_return = old_return;
        }

        // Handle program-level statements
        for statement in &program.statements {
            self.generate_statement_constraints(statement)?;
        }

        Ok(self.constraints.clone())
    }

    /// Collect type signatures from declarations (first pass)
    fn collect_declarations(&mut self, program: &Program) -> Result<(), String> {
        // Collect function signatures
        for function in &program.functions {
            let param_types: Vec<ConstraintType> = function
                .parameters
                .iter()
                .map(|param| ConstraintType::from(param.type_.clone()))
                .collect();

            let return_type = ConstraintType::from(function.return_type.clone());

            let function_type = ConstraintType::Function {
                params: param_types,
                return_type: Box::new(return_type),
            };

            self.function_types
                .insert(function.name.clone(), function_type);
        }

        // Collect class signatures
        for class in &program.classes {
            let mut class_info = ClassTypeInfo {
                name: class.name.clone(),
                fields: HashMap::new(),
                methods: HashMap::new(),
                constructor: None,
                base_class: class.base_class.clone(),
            };

            // Collect field types
            for field in &class.fields {
                class_info.fields.insert(
                    field.name.clone(),
                    ConstraintType::from(field.type_.clone()),
                );
            }

            // Collect method types
            for method in &class.methods {
                let param_types: Vec<ConstraintType> = method
                    .parameters
                    .iter()
                    .map(|param| ConstraintType::from(param.type_.clone()))
                    .collect();

                let return_type = ConstraintType::from(method.return_type.clone());

                let method_type = ConstraintType::Function {
                    params: param_types,
                    return_type: Box::new(return_type),
                };

                class_info.methods.insert(method.name.clone(), method_type);
            }

            // Constructor type
            if let Some(constructor) = &class.constructor {
                let param_types: Vec<ConstraintType> = constructor
                    .parameters
                    .iter()
                    .map(|param| ConstraintType::from(param.type_.clone()))
                    .collect();

                let constructor_type = ConstraintType::Function {
                    params: param_types,
                    return_type: Box::new(ConstraintType::Concrete(Type::Object(
                        class.name.clone(),
                    ))),
                };

                class_info.constructor = Some(constructor_type);
            }

            self.class_types.insert(class.name.clone(), class_info);
        }

        Ok(())
    }

    /// Generate constraints for a function
    fn generate_function_constraints(&mut self, function: &Function) -> Result<(), String> {
        // Enter function scope
        self.type_vars.enter_scope();
        let old_return = self.current_function_return.clone();
        let old_class = self.current_class.clone();

        // Set up function context
        self.current_function_return = Some(ConstraintType::from(function.return_type.clone()));

        // Add parameter types to environment
        for param in &function.parameters {
            self.variable_types.insert(
                param.name.clone(),
                ConstraintType::from(param.type_.clone()),
            );
        }

        // Generate constraints for function body
        for statement in &function.body {
            self.generate_statement_constraints(statement)?;
        }

        // Restore context
        self.current_function_return = old_return;
        self.current_class = old_class;
        self.type_vars.exit_scope();

        Ok(())
    }

    /// Generate constraints for a statement
    fn generate_statement_constraints(&mut self, statement: &Statement) -> Result<(), String> {
        match statement {
            Statement::VariableDecl {
                name,
                type_,
                initializer,
                location,
            } => {
                let value_type = if let Some(init) = initializer {
                    self.generate_expression_constraints(init)?
                } else {
                    // For declarations without initializers, create a fresh type variable
                    let var = self
                        .type_vars
                        .fresh_var(format!("uninitialized_{}", name), location.clone());
                    ConstraintType::Variable(var)
                };

                let declared_type = if *type_ != Type::Void {
                    ConstraintType::from(type_.clone())
                } else {
                    // Create fresh type variable for inference when type is not specified
                    let var = self
                        .type_vars
                        .fresh_var(format!("variable_{}", name), location.clone());
                    ConstraintType::Variable(var)
                };

                // Add equality constraint
                self.constraints.add_equality(
                    value_type,
                    declared_type.clone(),
                    location.clone(),
                    format!("Variable declaration for '{}'", name),
                );

                // Add to environment
                self.variable_types.insert(name.clone(), declared_type);
            }

            Statement::Assignment {
                target,
                value,
                location,
            } => {
                // For assignments, target should be a variable name
                let target_type = if let Some(typ) = self.variable_types.get(target) {
                    typ.clone()
                } else {
                    return Err(format!("Undefined variable '{}' in assignment", target));
                };
                let value_type = self.generate_expression_constraints(value)?;

                self.constraints.add_equality(
                    target_type,
                    value_type,
                    location.clone(),
                    "Assignment compatibility".to_string(),
                );
            }

            Statement::Expression { expr, .. } => {
                self.generate_expression_constraints(expr)?;
            }

            Statement::Return { value, location } => {
                if let Some(return_type) = self.current_function_return.clone() {
                    if let Some(value_expr) = value {
                        let value_type = self.generate_expression_constraints(value_expr)?;
                        self.constraints.add_equality(
                            value_type,
                            return_type,
                            location.clone(),
                            "Return statement type".to_string(),
                        );
                    } else {
                        // Return with no value should match Void
                        self.constraints.add_equality(
                            ConstraintType::Concrete(Type::Void),
                            return_type,
                            location.clone(),
                            "Return statement (no value)".to_string(),
                        );
                    }
                }
            }

            Statement::If {
                condition,
                then_branch,
                else_branch,
                location,
            } => {
                // Condition must be boolean
                let condition_type = self.generate_expression_constraints(condition)?;
                self.constraints.add_equality(
                    condition_type,
                    ConstraintType::Concrete(Type::Boolean),
                    location.clone(),
                    "If condition type".to_string(),
                );

                // Generate constraints for branches
                for stmt in then_branch {
                    self.generate_statement_constraints(stmt)?;
                }
                if let Some(else_stmts) = else_branch {
                    for stmt in else_stmts {
                        self.generate_statement_constraints(stmt)?;
                    }
                }
            }

            Statement::While {
                condition,
                body,
                location,
            } => {
                // Condition must be boolean
                let condition_type = self.generate_expression_constraints(condition)?;
                self.constraints.add_equality(
                    condition_type,
                    ConstraintType::Concrete(Type::Boolean),
                    location.clone(),
                    "While condition type".to_string(),
                );

                for stmt in body {
                    self.generate_statement_constraints(stmt)?;
                }
            }

            Statement::Print {
                expression,
                location,
                ..
            } => {
                // Print statements should accept any stringifiable type
                let expr_type = self.generate_expression_constraints(expression)?;
                self.constraints.add_property(
                    expr_type,
                    TypeProperty::Stringifiable,
                    location.clone(),
                    "Print statement expression".to_string(),
                );
            }

            _ => {
                // Handle other statement types as needed
            }
        }

        Ok(())
    }

    /// Generate constraints for an expression and return its type
    fn generate_expression_constraints(
        &mut self,
        expression: &Expression,
    ) -> Result<ConstraintType, String> {
        match expression {
            Expression::Literal(value) => Ok(self.literal_type(value)),

            Expression::Variable(name) => {
                if let Some(typ) = self.variable_types.get(name) {
                    Ok(typ.clone())
                } else {
                    Err(format!("Undefined variable '{}'", name))
                }
            }

            Expression::Binary(left, operator, right) => {
                let left_type = self.generate_expression_constraints(left)?;
                let right_type = self.generate_expression_constraints(right)?;

                self.generate_binary_op_constraints(left_type, operator, right_type, None)
            }

            Expression::Unary(operator, operand) => {
                let operand_type = self.generate_expression_constraints(operand)?;
                self.generate_unary_op_constraints(operator, operand_type, None)
            }

            Expression::Call(name, args) => {
                self.generate_function_call_constraints(name, args, None)
            }

            Expression::MethodCall {
                object,
                method,
                arguments,
                location,
            } => {
                let object_type = self.generate_expression_constraints(object)?;
                self.generate_method_call_constraints(
                    object_type,
                    method,
                    arguments,
                    Some(location.clone()),
                )
            }

            Expression::ListAccess(array, index) => {
                let array_type = self.generate_expression_constraints(array)?;
                let index_type = self.generate_expression_constraints(index)?;

                // Index must be integer
                self.constraints.add_equality(
                    index_type,
                    ConstraintType::Concrete(Type::Integer),
                    None,
                    "List index type".to_string(),
                );

                // Array must be List type, return element type
                let element_var = self.type_vars.fresh_var("list_element".to_string(), None);
                let element_type = ConstraintType::Variable(element_var);

                self.constraints.add_constraint(Constraint::ArrayElement {
                    array_type,
                    element_type: element_type.clone(),
                    location: None,
                    reason: "List access".to_string(),
                });

                Ok(element_type)
            }

            Expression::ObjectCreation {
                class_name,
                arguments,
                location,
            } => {
                if let Some(class_info) = self.class_types.get(class_name).cloned() {
                    if let Some(constructor_type) = class_info.constructor {
                        // Check constructor arguments
                        if let ConstraintType::Function { params, .. } = constructor_type {
                            if arguments.len() != params.len() {
                                return Err(format!(
                                    "Constructor for '{}' expects {} arguments, got {}",
                                    class_name,
                                    params.len(),
                                    arguments.len()
                                ));
                            }

                            // Check argument types
                            for (i, (arg, expected_type)) in
                                arguments.iter().zip(params.iter()).enumerate()
                            {
                                let arg_type = self.generate_expression_constraints(arg)?;
                                self.constraints.add_equality(
                                    arg_type,
                                    expected_type.clone(),
                                    Some(location.clone()),
                                    format!("Constructor argument {} for '{}'", i, class_name),
                                );
                            }
                        }
                    }

                    Ok(ConstraintType::Concrete(Type::Object(class_name.clone())))
                } else {
                    Err(format!("Unknown class '{}'", class_name))
                }
            }

            Expression::PropertyAccess {
                object,
                property,
                location,
            } => {
                let object_type = self.generate_expression_constraints(object)?;
                self.generate_field_access_constraints(
                    object_type,
                    property,
                    Some(location.clone()),
                )
            }

            _ => {
                // Handle other expression types as needed
                let var = self.type_vars.fresh_var("unknown_expr".to_string(), None);
                Ok(ConstraintType::Variable(var))
            }
        }
    }

    /// Generate constraints for binary operations
    fn generate_binary_op_constraints(
        &mut self,
        left_type: ConstraintType,
        operator: &BinaryOperator,
        right_type: ConstraintType,
        location: Option<SourceLocation>,
    ) -> Result<ConstraintType, String> {
        match operator {
            BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide => {
                // Both operands must be numeric
                self.constraints.add_property(
                    left_type.clone(),
                    TypeProperty::Numeric,
                    location.clone(),
                    format!("Left operand of {:?}", operator),
                );
                self.constraints.add_property(
                    right_type.clone(),
                    TypeProperty::Numeric,
                    location.clone(),
                    format!("Right operand of {:?}", operator),
                );

                // Result type is the wider of the two operands
                let result_var = self
                    .type_vars
                    .fresh_var(format!("{:?}_result", operator), location.clone());
                let result_type = ConstraintType::Variable(result_var);

                // Add promotion constraints
                self.constraints.add_subtype(
                    left_type,
                    result_type.clone(),
                    location.clone(),
                    "Numeric promotion (left)".to_string(),
                );
                self.constraints.add_subtype(
                    right_type,
                    result_type.clone(),
                    location.clone(),
                    "Numeric promotion (right)".to_string(),
                );

                Ok(result_type)
            }

            BinaryOperator::Equal | BinaryOperator::NotEqual => {
                // Types must be comparable
                self.constraints.add_property(
                    left_type.clone(),
                    TypeProperty::Comparable,
                    location.clone(),
                    "Comparison left operand".to_string(),
                );
                self.constraints.add_property(
                    right_type.clone(),
                    TypeProperty::Comparable,
                    location.clone(),
                    "Comparison right operand".to_string(),
                );

                // Types should be compatible for comparison
                self.constraints.add_equality(
                    left_type,
                    right_type,
                    location.clone(),
                    "Comparison operand compatibility".to_string(),
                );

                Ok(ConstraintType::Concrete(Type::Boolean))
            }

            BinaryOperator::Less
            | BinaryOperator::Greater
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterEqual => {
                // Both operands must be comparable and same type
                self.constraints.add_property(
                    left_type.clone(),
                    TypeProperty::Comparable,
                    location.clone(),
                    "Ordering comparison left operand".to_string(),
                );
                self.constraints.add_property(
                    right_type.clone(),
                    TypeProperty::Comparable,
                    location.clone(),
                    "Ordering comparison right operand".to_string(),
                );

                self.constraints.add_equality(
                    left_type,
                    right_type,
                    location.clone(),
                    "Ordering comparison type compatibility".to_string(),
                );

                Ok(ConstraintType::Concrete(Type::Boolean))
            }

            BinaryOperator::And | BinaryOperator::Or => {
                // Both operands must be boolean
                self.constraints.add_equality(
                    left_type,
                    ConstraintType::Concrete(Type::Boolean),
                    location.clone(),
                    "Logical operation left operand".to_string(),
                );
                self.constraints.add_equality(
                    right_type,
                    ConstraintType::Concrete(Type::Boolean),
                    location.clone(),
                    "Logical operation right operand".to_string(),
                );

                Ok(ConstraintType::Concrete(Type::Boolean))
            }

            _ => {
                // Handle other operators as needed
                let result_var = self
                    .type_vars
                    .fresh_var(format!("{:?}_result", operator), location);
                Ok(ConstraintType::Variable(result_var))
            }
        }
    }

    /// Generate constraints for unary operations
    fn generate_unary_op_constraints(
        &mut self,
        operator: &UnaryOperator,
        operand_type: ConstraintType,
        location: Option<SourceLocation>,
    ) -> Result<ConstraintType, String> {
        match operator {
            UnaryOperator::Negate => {
                self.constraints.add_property(
                    operand_type.clone(),
                    TypeProperty::Numeric,
                    location,
                    "Unary minus operand".to_string(),
                );
                Ok(operand_type)
            }

            UnaryOperator::Not => {
                self.constraints.add_equality(
                    operand_type,
                    ConstraintType::Concrete(Type::Boolean),
                    location,
                    "Logical not operand".to_string(),
                );
                Ok(ConstraintType::Concrete(Type::Boolean))
            }
        }
    }

    /// Generate constraints for function calls
    fn generate_function_call_constraints(
        &mut self,
        name: &str,
        args: &[Expression],
        location: Option<SourceLocation>,
    ) -> Result<ConstraintType, String> {
        let function_type = self.function_types.get(name).cloned();
        if let Some(function_type) = function_type {
            if let ConstraintType::Function {
                params,
                return_type,
            } = function_type
            {
                if args.len() != params.len() {
                    return Err(format!(
                        "Function '{}' expects {} arguments, got {}",
                        name,
                        params.len(),
                        args.len()
                    ));
                }

                // Check argument types
                for (i, (arg, expected_type)) in args.iter().zip(params.iter()).enumerate() {
                    let arg_type = self.generate_expression_constraints(arg)?;
                    self.constraints.add_equality(
                        arg_type,
                        expected_type.clone(),
                        location.clone(),
                        format!("Function '{}' argument {}", name, i),
                    );
                }

                Ok(*return_type)
            } else {
                Err(format!("'{}' is not a function", name))
            }
        } else {
            Err(format!("Unknown function '{}'", name))
        }
    }

    /// Generate constraints for method calls
    fn generate_method_call_constraints(
        &mut self,
        object_type: ConstraintType,
        method: &str,
        args: &[Expression],
        location: Option<SourceLocation>,
    ) -> Result<ConstraintType, String> {
        // Create constraint that object type has the method
        let method_var = self.type_vars.fresh_var(
            format!(
                "method_{}_{}",
                method,
                location.as_ref().map(|l| l.line).unwrap_or(0)
            ),
            location.clone(),
        );
        let method_type = ConstraintType::Variable(method_var);

        self.constraints.add_constraint(Constraint::ClassMember {
            class_type: object_type,
            member_name: method.to_string(),
            member_type: method_type.clone(),
            location: location.clone(),
            reason: format!("Method call '{}'", method),
        });

        // Generate constraints for method arguments
        let arg_types: Result<Vec<_>, _> = args
            .iter()
            .map(|arg| self.generate_expression_constraints(arg))
            .collect();
        let arg_types = arg_types?;

        // Create return type variable
        let return_var = self
            .type_vars
            .fresh_var(format!("method_{}_return", method), location.clone());
        let return_type = ConstraintType::Variable(return_var);

        // Method type must be function type
        self.constraints.add_equality(
            method_type,
            ConstraintType::Function {
                params: arg_types,
                return_type: Box::new(return_type.clone()),
            },
            location,
            format!("Method '{}' signature", method),
        );

        Ok(return_type)
    }

    /// Generate constraints for field access
    fn generate_field_access_constraints(
        &mut self,
        object_type: ConstraintType,
        field: &str,
        location: Option<SourceLocation>,
    ) -> Result<ConstraintType, String> {
        let field_var = self.type_vars.fresh_var(
            format!(
                "field_{}_{}",
                field,
                location.as_ref().map(|l| l.line).unwrap_or(0)
            ),
            location.clone(),
        );
        let field_type = ConstraintType::Variable(field_var);

        self.constraints.add_constraint(Constraint::ClassMember {
            class_type: object_type,
            member_name: field.to_string(),
            member_type: field_type.clone(),
            location: location.clone(),
            reason: format!("Field access '{}'", field),
        });

        Ok(field_type)
    }

    /// Get the type of a literal value
    fn literal_type(&mut self, value: &Value) -> ConstraintType {
        match value {
            Value::Integer(_) => ConstraintType::Concrete(Type::Integer),
            Value::Number(_) => ConstraintType::Concrete(Type::Number),
            Value::String(_) => ConstraintType::Concrete(Type::String),
            Value::Boolean(_) => ConstraintType::Concrete(Type::Boolean),
            Value::Null => ConstraintType::Concrete(Type::Void), // Null maps to void semantics
            Value::Void => ConstraintType::Concrete(Type::Void),
            Value::List(items) => {
                if items.is_empty() {
                    // Empty list - create generic list type
                    let element_var = self
                        .type_vars
                        .fresh_var("empty_list_element".to_string(), None);
                    ConstraintType::Generic {
                        name: "List".to_string(),
                        params: vec![ConstraintType::Variable(element_var)],
                    }
                } else {
                    // Infer element type from first element
                    let first_type = match &items[0] {
                        Value::Integer(_) => ConstraintType::Concrete(Type::Integer),
                        Value::Number(_) => ConstraintType::Concrete(Type::Number),
                        Value::String(_) => ConstraintType::Concrete(Type::String),
                        Value::Boolean(_) => ConstraintType::Concrete(Type::Boolean),
                        Value::Null => ConstraintType::Concrete(Type::Void), // Null maps to void semantics
                        Value::Void => ConstraintType::Concrete(Type::Void),
                        _ => ConstraintType::Top, // For complex nested types
                    };
                    ConstraintType::Generic {
                        name: "List".to_string(),
                        params: vec![first_type],
                    }
                }
            }
            Value::Matrix(_) => {
                let element_var = self.type_vars.fresh_var("matrix_element".to_string(), None);
                ConstraintType::Generic {
                    name: "Matrix".to_string(),
                    params: vec![ConstraintType::Variable(element_var)],
                }
            }
            Value::Pairs(_) => {
                // Pairs literals
                let key_var = self.type_vars.fresh_var("pairs_key".to_string(), None);
                let value_var = self.type_vars.fresh_var("pairs_value".to_string(), None);
                ConstraintType::Generic {
                    name: "Pairs".to_string(),
                    params: vec![
                        ConstraintType::Variable(key_var),
                        ConstraintType::Variable(value_var),
                    ],
                }
            }
            // Handle sized integer types
            Value::Integer8(_)
            | Value::Integer8u(_)
            | Value::Integer16(_)
            | Value::Integer16u(_)
            | Value::Integer32(_)
            | Value::Integer64(_) => ConstraintType::Concrete(Type::Integer),
            Value::Number32(_) | Value::Number64(_) => ConstraintType::Concrete(Type::Number),
        }
    }

    /// Add built-in types and functions
    fn add_builtin_types(&mut self) {
        // Add built-in functions like print, etc.
        self.function_types.insert(
            "print".to_string(),
            ConstraintType::Function {
                params: vec![ConstraintType::Concrete(Type::String)],
                return_type: Box::new(ConstraintType::Concrete(Type::Void)),
            },
        );

        self.function_types.insert(
            "toString".to_string(),
            ConstraintType::Function {
                params: vec![ConstraintType::Top], // Accept any type
                return_type: Box::new(ConstraintType::Concrete(Type::String)),
            },
        );

        // Add more built-in functions as needed
    }

    /// Get the final constraint set
    pub fn get_constraints(self) -> ConstraintSet {
        self.constraints
    }

    /// Get the type variable manager
    pub fn get_type_vars(&self) -> &TypeVariableManager {
        &self.type_vars
    }
}

impl Default for ConstraintGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Type;

    #[test]
    fn test_variable_declaration_constraint() {
        let mut generator = ConstraintGenerator::new();

        let expr = Expression::Literal(Value::Integer(42));

        let stmt = Statement::VariableDecl {
            type_: Type::Integer,
            name: "x".to_string(),
            initializer: Some(expr),
            location: None,
        };

        let result = generator.generate_statement_constraints(&stmt);
        assert!(result.is_ok());

        // Should have equality constraint between literal and declared type
        let constraints = generator.get_constraints();
        assert!(!constraints.constraints.is_empty());
    }

    #[test]
    fn test_function_call_constraint() {
        let mut generator = ConstraintGenerator::new();

        // Add a test function
        generator.function_types.insert(
            "add".to_string(),
            ConstraintType::Function {
                params: vec![
                    ConstraintType::Concrete(Type::Integer),
                    ConstraintType::Concrete(Type::Integer),
                ],
                return_type: Box::new(ConstraintType::Concrete(Type::Integer)),
            },
        );

        let args = vec![
            Expression::Literal(Value::Integer(1)),
            Expression::Literal(Value::Integer(2)),
        ];

        let result = generator.generate_function_call_constraints("add", &args, None);
        assert!(result.is_ok());

        if let Ok(return_type) = result {
            assert_eq!(return_type, ConstraintType::Concrete(Type::Integer));
        }
    }

    #[test]
    fn test_binary_operation_constraint() {
        let mut generator = ConstraintGenerator::new();

        let left = Expression::Literal(Value::Integer(1));
        let right = Expression::Literal(Value::Integer(2));

        let expr = Expression::Binary(Box::new(left), BinaryOperator::Add, Box::new(right));

        let result = generator.generate_expression_constraints(&expr);
        assert!(result.is_ok());

        // Should generate numeric property constraints
        let constraints = generator.get_constraints();
        let has_numeric_constraint = constraints.constraints.iter().any(|c| {
            matches!(
                c,
                Constraint::HasProperty {
                    property: TypeProperty::Numeric,
                    ..
                }
            )
        });
        assert!(has_numeric_constraint);
    }

    #[test]
    fn test_array_access_constraint() {
        let mut generator = ConstraintGenerator::new();

        let array = Expression::Variable("arr".to_string());
        let index = Expression::Literal(Value::Integer(0));

        // Add array variable to environment
        generator.variable_types.insert(
            "arr".to_string(),
            ConstraintType::Generic {
                name: "List".to_string(),
                params: vec![ConstraintType::Concrete(Type::String)],
            },
        );

        let expr = Expression::ListAccess(Box::new(array), Box::new(index));

        let result = generator.generate_expression_constraints(&expr);
        assert!(result.is_ok());

        // Should generate array element constraint
        let constraints = generator.get_constraints();
        let has_array_constraint = constraints
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::ArrayElement { .. }));
        assert!(has_array_constraint);
    }
}
