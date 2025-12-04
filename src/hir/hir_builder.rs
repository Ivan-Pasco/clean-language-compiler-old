//! HIR Builder - converts AST to HIR (High-level Intermediate Representation)
//!
//! This module implements the transformation from AST to HIR, which includes:
//! - Desugaring syntactic constructs into normalized forms
//! - Validating semantic consistency (but not type checking)
//! - Converting implicit operations to explicit ones
//! - Maintaining source location information for error reporting

use crate::ast::SourceLocation;
use crate::ast::{
    BinaryOperator, Class, Constructor, Expression, Function, Parameter, Program, Statement, Type,
    UnaryOperator, Value,
};
use crate::error::CompilerError;
use crate::hir::*;

/// HIR Builder - constructs HIR from AST
pub struct HirBuilder {
    type_inference_counter: usize,
    warnings: Vec<CompilerError>,
    constant_bindings: std::collections::HashSet<String>,
}

impl HirBuilder {
    /// Create a new HIR builder
    pub fn new() -> Self {
        Self {
            type_inference_counter: 0,
            warnings: Vec::new(),
            constant_bindings: std::collections::HashSet::new(),
        }
    }

    /// Build HIR from an AST program
    pub fn build_hir(&mut self, program: Program) -> Result<HirValidationResult, CompilerError> {
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut start_function = None;
        let mut imports = Vec::new();
        let tests = Vec::new();

        // Process top-level statements
        for statement in &program.statements {
            match statement {
                Statement::FunctionsBlock {
                    functions: func_list,
                    ..
                } => {
                    for func in func_list {
                        let hir_func = self.build_function(func)?;
                        if func.name == "start" {
                            start_function = Some(hir_func);
                        } else {
                            functions.push(hir_func);
                        }
                    }
                }
                Statement::ClassDefinition { class, .. } => {
                    classes.push(self.build_class(class)?);
                }
                Statement::Import {
                    imports: import_list,
                    ..
                } => {
                    for import_item in import_list {
                        // Parse import name to separate module and symbol
                        // Examples:
                        //   "Math" → module: "Math", items: None (whole module)
                        //   "math.sqrt" → module: "math", items: Some(["sqrt"]) (specific symbol)
                        //   "Utils as U" → module: "Utils", items: None, (alias handled separately)
                        //   "Json.decode as jd" → module: "Json", items: Some(["decode"]), (alias handled separately)

                        let (module_name, symbol_items) =
                            if let Some(dot_pos) = import_item.name.find('.') {
                                // Contains dot - import specific symbol(s)
                                let module = &import_item.name[..dot_pos];
                                let symbol = &import_item.name[dot_pos + 1..];
                                (module.to_string(), Some(vec![symbol.to_string()]))
                            } else {
                                // No dot - import whole module
                                (import_item.name.clone(), None)
                            };

                        imports.push(HirImport {
                            module_name,
                            items: symbol_items,
                            location: SourceLocation::default(),
                        });
                    }
                }
                _ => {
                    // Handle other top-level statements if needed
                }
            }
        }

        // Process standalone functions from program.functions
        for func in &program.functions {
            let hir_func = self.build_function(func)?;
            if func.name == "start" {
                start_function = Some(hir_func);
            } else {
                functions.push(hir_func);
            }
        }

        // Process classes from program.classes
        for class in &program.classes {
            classes.push(self.build_class(class)?);
        }

        // Handle the start function if it exists
        if let Some(start_func) = &program.start_function {
            start_function = Some(self.build_function(start_func)?);
        }

        let hir_program = HirProgram {
            functions,
            classes,
            start_function,
            imports,
            tests,
            location: program.location.unwrap_or_default(),
        };

        Ok(HirValidationResult {
            hir: hir_program,
            warnings: self.warnings.clone(),
            type_inference_count: self.type_inference_counter,
        })
    }

    /// Convert AST function to HIR function
    fn build_function(&mut self, func: &Function) -> Result<HirFunction, CompilerError> {
        let parameters = func
            .parameters
            .iter()
            .map(|param| self.build_parameter(param))
            .collect::<Result<Vec<_>, _>>()?;

        let return_type = if func.return_type == Type::Void {
            None
        } else {
            Some(self.build_type(&func.return_type)?)
        };

        let body = self.build_block(&func.body)?;

        Ok(HirFunction {
            name: func.name.clone(),
            parameters,
            return_type,
            body,
            is_start: func.name == "start",
            location: func.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST class to HIR class
    fn build_class(&mut self, class: &Class) -> Result<HirClass, CompilerError> {
        let fields = class
            .fields
            .iter()
            .map(|field| self.build_field(field))
            .collect::<Result<Vec<_>, _>>()?;

        let constructor = if let Some(ctor) = &class.constructor {
            Some(self.build_constructor(ctor, &class.fields)?)
        } else {
            None
        };

        let methods = class
            .methods
            .iter()
            .map(|method| self.build_method(method))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(HirClass {
            name: class.name.clone(),
            parent: class.base_class.clone(),
            fields,
            constructor,
            methods,
            location: class.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST field to HIR field
    fn build_field(&mut self, field: &crate::ast::Field) -> Result<HirField, CompilerError> {
        let field_type = self.build_type(&field.type_)?;
        let initializer = if let Some(init) = &field.default_value {
            Some(self.build_expression(init)?)
        } else {
            None
        };

        Ok(HirField {
            name: field.name.clone(),
            field_type,
            initializer,
            location: SourceLocation::default(),
        })
    }

    /// Convert AST constructor to HIR constructor
    fn build_constructor(
        &mut self,
        ctor: &Constructor,
        class_fields: &[crate::ast::Field],
    ) -> Result<HirConstructor, CompilerError> {
        let parameters = ctor
            .parameters
            .iter()
            .map(|param| self.build_parameter(param))
            .collect::<Result<Vec<_>, _>>()?;

        let mut body = self.build_block(&ctor.body)?;

        // CRITICAL FIX: Auto-storing fields feature
        // When constructor body is empty and parameter names match field names,
        // automatically generate field assignments: field = parameter
        if body.statements.is_empty() {
            let mut auto_assignments = Vec::new();

            for param in &ctor.parameters {
                // Check if there's a field with matching name
                if let Some(_field) = class_fields.iter().find(|f| f.name == param.name) {
                    // Generate: field = parameter
                    let assignment = HirStatement::Assignment {
                        target: HirLValue::Variable {
                            name: param.name.clone(),
                            location: SourceLocation::default(),
                        },
                        value: HirExpression::Variable {
                            name: param.name.clone(),
                            location: SourceLocation::default(),
                        },
                        location: SourceLocation::default(),
                    };
                    auto_assignments.push(assignment);
                }
            }

            body.statements = auto_assignments;
        }

        Ok(HirConstructor {
            parameters,
            body,
            location: ctor.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST method to HIR method  
    fn build_method(&mut self, method: &Function) -> Result<HirMethod, CompilerError> {
        let parameters = method
            .parameters
            .iter()
            .map(|param| self.build_parameter(param))
            .collect::<Result<Vec<_>, _>>()?;

        let return_type = self.build_type(&method.return_type)?;
        let body = self.build_block(&method.body)?;

        Ok(HirMethod {
            name: method.name.clone(),
            parameters,
            return_type,
            body,
            location: method.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST parameter to HIR parameter
    fn build_parameter(&mut self, param: &Parameter) -> Result<HirParameter, CompilerError> {
        let param_type = self.build_type(&param.type_)?;
        let default_value = if let Some(default_expr) = &param.default_value {
            Some(self.build_expression(default_expr)?)
        } else {
            None
        };

        Ok(HirParameter {
            name: param.name.clone(),
            param_type,
            default_value,
            location: SourceLocation::default(),
        })
    }

    /// Convert AST type to HIR type
    fn build_type(&mut self, ast_type: &Type) -> Result<HirType, CompilerError> {
        match ast_type {
            Type::Boolean => Ok(HirType::Boolean),
            Type::Integer => Ok(HirType::Integer),
            Type::Number => Ok(HirType::Number),
            Type::String => Ok(HirType::String),
            Type::Void => Ok(HirType::Void),
            Type::IntegerSized { bits, unsigned } => match (bits, unsigned) {
                (8, false) => Ok(HirType::Integer8),
                (8, true) => Ok(HirType::Integer8u),
                (16, false) => Ok(HirType::Integer16),
                (16, true) => Ok(HirType::Integer16u),
                (32, false) => Ok(HirType::Integer32),
                (32, true) => Ok(HirType::Integer32u),
                (64, false) => Ok(HirType::Integer64),
                (64, true) => Ok(HirType::Integer64u),
                _ => Err(CompilerError::syntax_error(
                    format!("Unsupported integer size: {bits} bits, unsigned: {unsigned}"),
                    Some("Only 8, 16, 32, and 64 bit integers are supported".to_string()),
                    None,
                )),
            },
            Type::NumberSized { bits } => match bits {
                32 => Ok(HirType::Number32),
                64 => Ok(HirType::Number64),
                _ => Err(CompilerError::syntax_error(
                    format!("Unsupported number size: {bits} bits"),
                    Some("Only 32 and 64 bit numbers are supported".to_string()),
                    None,
                )),
            },
            Type::List(inner) => {
                let inner_type = self.build_type(inner)?;
                Ok(HirType::List(Box::new(inner_type)))
            }
            Type::Matrix(inner) => {
                let inner_type = self.build_type(inner)?;
                Ok(HirType::Matrix(Box::new(inner_type)))
            }
            Type::Pairs(key_type, value_type) => {
                let key_hir_type = self.build_type(key_type)?;
                let value_hir_type = self.build_type(value_type)?;
                Ok(HirType::Pairs(
                    Box::new(key_hir_type),
                    Box::new(value_hir_type),
                ))
            }
            Type::Object(name) | Type::Class { name, .. } => Ok(HirType::Named {
                name: name.clone(),
                location: SourceLocation::default(),
            }),
            Type::Any => {
                // Treat 'any' as a named generic type, not inferred
                // This allows proper type checking with empty literals
                Ok(HirType::Named {
                    name: "any".to_string(),
                    location: SourceLocation::default(),
                })
            }
            _ => {
                // For unsupported types, create an inferred type for now
                self.type_inference_counter += 1;
                Ok(HirType::Inferred {
                    id: self.type_inference_counter,
                    location: SourceLocation::default(),
                })
            }
        }
    }

    /// Convert statements to HIR block
    fn build_block(&mut self, statements: &[Statement]) -> Result<HirBlock, CompilerError> {
        let mut hir_statements = Vec::new();

        for stmt in statements {
            // Special handling for TypeApplyBlock - expand into multiple statements
            if let Statement::TypeApplyBlock {
                type_,
                assignments,
                location,
            } = stmt
            {
                tracing::debug!(
                    type_ = ?type_,
                    assignments_count = assignments.len(),
                    "Expanding TypeApplyBlock into variable declarations"
                );
                // Convert each assignment in the apply block to a variable declaration
                for assignment in assignments {
                    let var_type = self.build_type(type_)?;
                    let init_expr = if let Some(init) = &assignment.initializer {
                        Some(self.build_expression(init)?)
                    } else {
                        None
                    };

                    tracing::debug!(
                        variable_name = %assignment.name,
                        var_type = ?var_type,
                        "Created VariableDeclaration from TypeApplyBlock"
                    );

                    hir_statements.push(HirStatement::VariableDeclaration {
                        name: assignment.name.clone(),
                        var_type,
                        initializer: init_expr,
                        is_mutable: true, // Apply blocks create mutable variables
                        location: location.clone().unwrap_or_default(),
                    });
                }
            } else if let Statement::ConstantApplyBlock {
                constants,
                location,
            } = stmt
            {
                // Convert each constant in the apply block to a variable declaration
                // Constants are treated as immutable variables in HIR
                for constant in constants {
                    let var_type = self.build_type(&constant.type_)?;
                    let init_expr = Some(self.build_expression(&constant.value)?);

                    // Track this as a constant binding for resolver
                    self.constant_bindings.insert(constant.name.clone());

                    hir_statements.push(HirStatement::VariableDeclaration {
                        name: constant.name.clone(),
                        var_type,
                        initializer: init_expr,
                        is_mutable: false, // Constant apply blocks create immutable variables
                        location: location.clone().unwrap_or_default(),
                    });
                }
            } else {
                // Regular statement processing
                hir_statements.push(self.build_statement(stmt)?);
            }
        }

        Ok(HirBlock {
            statements: hir_statements,
            location: SourceLocation::default(),
        })
    }

    /// Convert AST statement to HIR statement
    fn build_statement(&mut self, stmt: &Statement) -> Result<HirStatement, CompilerError> {
        match stmt {
            Statement::VariableDecl {
                name,
                type_,
                initializer,
                location,
            } => {
                let var_type = self.build_type(type_)?;
                let init_expr = if let Some(init) = initializer {
                    Some(self.build_expression(init)?)
                } else {
                    None
                };

                Ok(HirStatement::VariableDeclaration {
                    name: name.clone(),
                    var_type,
                    initializer: init_expr,
                    is_mutable: true, // Regular variable declarations are mutable by default
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Assignment {
                target,
                value,
                location,
            } => {
                let lvalue = HirLValue::Variable {
                    name: target.clone(),
                    location: location.clone().unwrap_or_default(),
                };
                let hir_value = self.build_expression(value)?;

                Ok(HirStatement::Assignment {
                    target: lvalue,
                    value: hir_value,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Print {
                expression,
                newline,
                location,
            } => {
                let hir_expr = self.build_expression(expression)?;
                Ok(HirStatement::Print {
                    expression: hir_expr,
                    newline: *newline,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Return { value, location } => {
                let return_value = if let Some(expr) = value {
                    Some(self.build_expression(expr)?)
                } else {
                    None
                };

                Ok(HirStatement::Return {
                    value: return_value,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::If {
                condition,
                then_branch,
                else_branch,
                location,
            } => {
                let hir_condition = self.build_expression(condition)?;
                let hir_then = self.build_block(then_branch)?;
                let hir_else = if let Some(else_stmts) = else_branch {
                    Some(self.build_block(else_stmts)?)
                } else {
                    None
                };

                Ok(HirStatement::If {
                    condition: hir_condition,
                    then_branch: hir_then,
                    else_branch: hir_else,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Iterate {
                iterator,
                collection,
                body,
                location,
            } => {
                let hir_iterable = self.build_expression(collection)?;
                let hir_body = self.build_block(body)?;

                Ok(HirStatement::For {
                    variable: iterator.clone(),
                    iterable: hir_iterable,
                    body: hir_body,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::RangeIterate {
                iterator,
                start,
                end,
                step,
                body,
                location,
            } => {
                // Convert RangeIterate to For with a Range expression as the iterable
                let hir_start = self.build_expression(start)?;
                let hir_end = self.build_expression(end)?;

                // Step parameter warning (not yet in language specification)
                if step.is_some() {
                    eprintln!("WARNING: Range iteration with step is not yet fully supported");
                }

                let range_expr = HirExpression::Range {
                    start: Box::new(hir_start),
                    end: Box::new(hir_end),
                    inclusive: true, // "iterate i in 0 to 10" is inclusive
                    location: location.clone().unwrap_or_default(),
                };

                let hir_body = self.build_block(body)?;

                Ok(HirStatement::For {
                    variable: iterator.clone(),
                    iterable: range_expr,
                    body: hir_body,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Expression { expr, location } => {
                let hir_expr = self.build_expression(expr)?;
                Ok(HirStatement::Expression {
                    expression: hir_expr,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::LaterAssignment {
                variable,
                expression,
                location,
            } => {
                let hir_expr = self.build_expression(expression)?;
                Ok(HirStatement::LaterAssignment {
                    variable: variable.clone(),
                    expression: hir_expr,
                    location: location.clone().unwrap_or_default(),
                })
            }

            // TypeApplyBlock is handled in build_block() where it can expand into multiple statements
            _ => {
                // For unsupported statements, create a dummy expression statement
                Ok(HirStatement::Expression {
                    expression: HirExpression::Literal {
                        value: Value::Void,
                        location: SourceLocation::default(),
                    },
                    location: SourceLocation::default(),
                })
            }
        }
    }

    /// Convert AST expression to HIR expression
    fn build_expression(&mut self, expr: &Expression) -> Result<HirExpression, CompilerError> {
        match expr {
            Expression::Literal(value) => {
                // CRITICAL FIX: Array literals must be converted to HirExpression::Array
                // not HirExpression::Literal, otherwise they get converted to Null in type inference
                match value {
                    Value::List(elements) => {
                        // Convert list literal to Array expression
                        let mut hir_elements = Vec::new();
                        for elem in elements {
                            hir_elements
                                .push(self.build_expression(&Expression::Literal(elem.clone()))?);
                        }

                        // Infer element type from first element, or use Void for empty lists
                        let element_type = if let Some(first_elem) = elements.first() {
                            self.value_to_hir_type(first_elem)
                        } else {
                            HirType::Void // Will be inferred from context in type checker
                        };

                        Ok(HirExpression::Array {
                            elements: hir_elements,
                            element_type,
                            location: SourceLocation::default(),
                        })
                    }
                    _ => Ok(HirExpression::Literal {
                        value: value.clone(),
                        location: SourceLocation::default(),
                    }),
                }
            }

            Expression::Variable(name) => Ok(HirExpression::Variable {
                name: name.clone(),
                location: SourceLocation::default(),
            }),

            Expression::Binary(left, op, right) => {
                let hir_left = self.build_expression(left)?;
                let hir_right = self.build_expression(right)?;
                let hir_op = self.convert_binary_op(op);

                Ok(HirExpression::BinaryOp {
                    left: Box::new(hir_left),
                    op: hir_op,
                    right: Box::new(hir_right),
                    location: SourceLocation::default(),
                })
            }

            Expression::Unary(op, operand) => {
                let hir_operand = self.build_expression(operand)?;
                let hir_op = self.convert_unary_op(op);

                Ok(HirExpression::UnaryOp {
                    op: hir_op,
                    operand: Box::new(hir_operand),
                    location: SourceLocation::default(),
                })
            }

            Expression::Call(name, args) => {
                let hir_args = args
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                // CRITICAL FIX: Detect base() calls for parent constructor invocation
                // base() is a special function call that invokes the parent class constructor
                if name == "base" {
                    eprintln!(
                        "DEBUG HIR: Detected base() call with {} arguments",
                        hir_args.len()
                    );
                    Ok(HirExpression::BaseCall {
                        arguments: hir_args,
                        location: SourceLocation::default(),
                    })
                } else {
                    Ok(HirExpression::Call {
                        function: name.clone(),
                        arguments: hir_args,
                        location: SourceLocation::default(),
                    })
                }
            }

            Expression::MethodCall {
                object,
                method,
                arguments,
                location,
            } => {
                let hir_object = self.build_expression(object)?;
                let hir_args = arguments
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::MethodCall {
                    receiver: Box::new(hir_object),
                    method: method.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::PropertyAccess {
                object,
                property,
                location,
            } => {
                let hir_object = self.build_expression(object)?;

                Ok(HirExpression::FieldAccess {
                    object: Box::new(hir_object),
                    field: property.clone(),
                    location: location.clone(),
                })
            }

            Expression::ListAccess(array, index) => {
                let hir_array = self.build_expression(array)?;
                let hir_index = self.build_expression(index)?;

                Ok(HirExpression::Index {
                    array: Box::new(hir_array),
                    index: Box::new(hir_index),
                    location: SourceLocation::default(),
                })
            }

            Expression::ObjectCreation {
                class_name,
                arguments,
                location,
            } => {
                let hir_args = arguments
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::Constructor {
                    class_name: class_name.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::NamespaceCall {
                namespace,
                function,
                arguments,
                location,
            } => {
                let hir_args = arguments
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::NamespaceCall {
                    namespace: namespace.clone(),
                    function: function.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::StaticMethodCall {
                namespace,
                class_name,
                method,
                arguments,
                location,
            } => {
                let hir_args = arguments
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::StaticMethodCall {
                    namespace: namespace.clone(),
                    class_name: class_name.clone(),
                    method: method.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::OnError {
                expression,
                fallback,
                location,
            } => {
                let hir_expression = self.build_expression(expression)?;
                let hir_fallback = self.build_expression(fallback)?;

                Ok(HirExpression::OnError {
                    expression: Box::new(hir_expression),
                    fallback: Box::new(hir_fallback),
                    location: location.clone(),
                })
            }

            Expression::Conditional {
                condition,
                then_expr,
                else_expr,
                location,
            } => {
                let hir_condition = self.build_expression(condition)?;
                let hir_then = self.build_expression(then_expr)?;
                let hir_else = self.build_expression(else_expr)?;

                Ok(HirExpression::Conditional {
                    condition: Box::new(hir_condition),
                    then_expr: Box::new(hir_then),
                    else_expr: Box::new(hir_else),
                    location: location.clone(),
                })
            }

            // CRITICAL FIX: Handle base() calls from AST
            // The parser creates Expression::BaseCall, so we must handle it here
            Expression::BaseCall {
                arguments,
                location,
            } => {
                eprintln!(
                    "DEBUG HIR: Handling Expression::BaseCall with {} arguments",
                    arguments.len()
                );
                let hir_args = arguments
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::BaseCall {
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::Range {
                start,
                end,
                inclusive,
                location,
            } => {
                let hir_start = self.build_expression(start)?;
                let hir_end = self.build_expression(end)?;

                Ok(HirExpression::Range {
                    start: Box::new(hir_start),
                    end: Box::new(hir_end),
                    inclusive: *inclusive,
                    location: location.clone(),
                })
            }

            _ => {
                // For unsupported expressions, create a void literal
                Ok(HirExpression::Literal {
                    value: Value::Void,
                    location: SourceLocation::default(),
                })
            }
        }
    }

    /// Convert AST binary operator to HIR binary operator
    fn convert_binary_op(&self, op: &BinaryOperator) -> HirBinaryOp {
        match op {
            BinaryOperator::Add => HirBinaryOp::Add,
            BinaryOperator::Subtract => HirBinaryOp::Subtract,
            BinaryOperator::Multiply => HirBinaryOp::Multiply,
            BinaryOperator::Divide => HirBinaryOp::Divide,
            BinaryOperator::Modulo => HirBinaryOp::Modulo,
            BinaryOperator::Power => HirBinaryOp::Power,
            BinaryOperator::Equal => HirBinaryOp::Equal,
            BinaryOperator::NotEqual => HirBinaryOp::NotEqual,
            BinaryOperator::Less => HirBinaryOp::Less,
            BinaryOperator::Greater => HirBinaryOp::Greater,
            BinaryOperator::LessEqual => HirBinaryOp::LessEqual,
            BinaryOperator::GreaterEqual => HirBinaryOp::GreaterEqual,
            BinaryOperator::Is => HirBinaryOp::Is,
            BinaryOperator::Not => HirBinaryOp::IsNot,
            BinaryOperator::And => HirBinaryOp::And,
            BinaryOperator::Or => HirBinaryOp::Or,
        }
    }

    /// Convert AST unary operator to HIR unary operator
    fn convert_unary_op(&self, op: &UnaryOperator) -> HirUnaryOp {
        match op {
            UnaryOperator::Negate => HirUnaryOp::Negate,
            UnaryOperator::Not => HirUnaryOp::Not,
        }
    }

    fn value_to_hir_type(&self, value: &Value) -> HirType {
        match value {
            Value::Integer(_) => HirType::Integer,
            Value::Number(_) => HirType::Number,
            Value::String(_) => HirType::String,
            Value::Boolean(_) => HirType::Boolean,
            Value::Integer8(_) => HirType::Integer8,
            Value::Integer8u(_) => HirType::Integer8u,
            Value::Integer16(_) => HirType::Integer16,
            Value::Integer16u(_) => HirType::Integer16u,
            Value::Integer32(_) => HirType::Integer32,
            Value::Integer64(_) => HirType::Integer64,
            Value::Number32(_) => HirType::Number32,
            Value::Number64(_) => HirType::Number64,
            Value::List(elements) => {
                let element_type = if let Some(first) = elements.first() {
                    Box::new(self.value_to_hir_type(first))
                } else {
                    // Empty list - use Void as placeholder, will be inferred from context
                    Box::new(HirType::Void)
                };
                HirType::List(element_type)
            }
            Value::Matrix(_) => {
                // Matrix type will be inferred properly in type checker
                HirType::Matrix(Box::new(HirType::Number))
            }
            Value::Pairs(_) => {
                // Pairs type will be inferred properly in type checker
                HirType::Pairs(Box::new(HirType::Void), Box::new(HirType::Void))
            }
            Value::Null | Value::Void => HirType::Void,
        }
    }
}

impl Default for HirBuilder {
    fn default() -> Self {
        Self::new()
    }
}
