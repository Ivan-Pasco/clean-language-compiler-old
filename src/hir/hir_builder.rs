//! HIR Builder - converts AST to HIR (High-level Intermediate Representation)
//!
//! This module implements the transformation from AST to HIR, which includes:
//! - Desugaring syntactic constructs into normalized forms
//! - Validating semantic consistency (but not type checking)
//! - Converting implicit operations to explicit ones
//! - Maintaining source location information for error reporting

use crate::ast::{Program, Statement, Expression, Function, Class, Type, Value, BinaryOperator, UnaryOperator, Parameter, Constructor};
use crate::ast::SourceLocation;
use crate::hir::*;
use crate::error::CompilerError;

/// HIR Builder - constructs HIR from AST
pub struct HirBuilder {
    type_inference_counter: usize,
    warnings: Vec<CompilerError>,
}

impl HirBuilder {
    /// Create a new HIR builder
    pub fn new() -> Self {
        Self {
            type_inference_counter: 0,
            warnings: Vec::new(),
        }
    }

    /// Build HIR from an AST program
    pub fn build_hir(&mut self, program: Program) -> Result<HirValidationResult, CompilerError> {
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut start_function = None;
        let mut imports = Vec::new();
        let mut tests = Vec::new();

        // Process top-level statements
        for statement in &program.statements {
            match statement {
                Statement::FunctionsBlock { functions: func_list, .. } => {
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
                Statement::Import { imports: import_list, .. } => {
                    for import_item in import_list {
                        imports.push(HirImport {
                            module_name: import_item.name.clone(),
                            items: if let Some(alias) = &import_item.alias {
                                Some(vec![alias.clone()])
                            } else {
                                None
                            },
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
        let parameters = func.parameters.iter()
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
        let fields = class.fields.iter()
            .map(|field| self.build_field(field))
            .collect::<Result<Vec<_>, _>>()?;

        let constructor = if let Some(ctor) = &class.constructor {
            Some(self.build_constructor(ctor)?)
        } else {
            None
        };

        let methods = class.methods.iter()
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
    fn build_constructor(&mut self, ctor: &Constructor) -> Result<HirConstructor, CompilerError> {
        let parameters = ctor.parameters.iter()
            .map(|param| self.build_parameter(param))
            .collect::<Result<Vec<_>, _>>()?;

        let body = self.build_block(&ctor.body)?;

        Ok(HirConstructor {
            parameters,
            body,
            location: ctor.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST method to HIR method  
    fn build_method(&mut self, method: &Function) -> Result<HirMethod, CompilerError> {
        let parameters = method.parameters.iter()
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

        Ok(HirParameter {
            name: param.name.clone(),
            param_type,
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
            Type::IntegerSized { bits, unsigned } => {
                match (bits, unsigned) {
                    (8, false) => Ok(HirType::Integer8),
                    (8, true) => Ok(HirType::Integer8u),
                    (16, false) => Ok(HirType::Integer16),
                    (16, true) => Ok(HirType::Integer16u),
                    (32, false) => Ok(HirType::Integer32),
                    (64, false) => Ok(HirType::Integer64),
                    _ => Err(CompilerError::syntax_error(
                        format!("Unsupported integer size: {bits} bits, unsigned: {unsigned}"),
                        Some("Only 8, 16, 32, and 64 bit integers are supported".to_string()),
                        None,
                    )),
                }
            }
            Type::NumberSized { bits } => {
                match bits {
                    32 => Ok(HirType::Number32),
                    64 => Ok(HirType::Number64),
                    _ => Err(CompilerError::syntax_error(
                        format!("Unsupported number size: {bits} bits"),
                        Some("Only 32 and 64 bit numbers are supported".to_string()),
                        None,
                    )),
                }
            }
            Type::List(inner) => {
                let inner_type = self.build_type(inner)?;
                Ok(HirType::List(Box::new(inner_type)))
            }
            Type::Matrix(inner) => {
                let inner_type = self.build_type(inner)?;
                Ok(HirType::Matrix(Box::new(inner_type)))
            }
            Type::Object(name) | Type::Class { name, .. } => {
                Ok(HirType::Named {
                    name: name.clone(),
                    location: SourceLocation::default(),
                })
            }
            Type::Any => {
                self.type_inference_counter += 1;
                Ok(HirType::Inferred {
                    id: self.type_inference_counter,
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
        let hir_statements = statements.iter()
            .map(|stmt| self.build_statement(stmt))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(HirBlock {
            statements: hir_statements,
            location: SourceLocation::default(),
        })
    }

    /// Convert AST statement to HIR statement
    fn build_statement(&mut self, stmt: &Statement) -> Result<HirStatement, CompilerError> {
        match stmt {
            Statement::VariableDecl { name, type_, initializer, location } => {
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
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Assignment { target, value, location } => {
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

            Statement::Print { expression, newline, location } => {
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

            Statement::If { condition, then_branch, else_branch, location } => {
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

            Statement::While { condition, body, location } => {
                let hir_condition = self.build_expression(condition)?;
                let hir_body = self.build_block(body)?;

                Ok(HirStatement::While {
                    condition: hir_condition,
                    body: hir_body,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Iterate { iterator, collection, body, location } => {
                let hir_iterable = self.build_expression(collection)?;
                let hir_body = self.build_block(body)?;

                Ok(HirStatement::For {
                    variable: iterator.clone(),
                    iterable: hir_iterable,
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
                Ok(HirExpression::Literal {
                    value: value.clone(),
                    location: SourceLocation::default(),
                })
            }

            Expression::Variable(name) => {
                Ok(HirExpression::Variable {
                    name: name.clone(),
                    location: SourceLocation::default(),
                })
            }

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
                let hir_args = args.iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::Call {
                    function: name.clone(),
                    arguments: hir_args,
                    location: SourceLocation::default(),
                })
            }

            Expression::MethodCall { object, method, arguments, location } => {
                let hir_object = self.build_expression(object)?;
                let hir_args = arguments.iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::MethodCall {
                    receiver: Box::new(hir_object),
                    method: method.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::PropertyAccess { object, property, location } => {
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

            Expression::ObjectCreation { class_name, arguments, location } => {
                let hir_args = arguments.iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::Constructor {
                    class_name: class_name.clone(),
                    arguments: hir_args,
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
            BinaryOperator::And => HirBinaryOp::And,
            BinaryOperator::Or => HirBinaryOp::Or,
            _ => HirBinaryOp::Add, // Default fallback
        }
    }

    /// Convert AST unary operator to HIR unary operator
    fn convert_unary_op(&self, op: &UnaryOperator) -> HirUnaryOp {
        match op {
            UnaryOperator::Negate => HirUnaryOp::Negate,
            UnaryOperator::Not => HirUnaryOp::Not,
        }
    }
}

impl Default for HirBuilder {
    fn default() -> Self {
        Self::new()
    }
}