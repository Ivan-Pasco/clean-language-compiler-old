//! Type inference engine for Clean Language
//!
//! Generates type constraints from resolved HIR and performs type inference
//! using constraint-based approach with Hindley-Milner algorithm.

use super::tast::{
    ConcreteType, TypeConstraint, TastProgram, TastFunction, TastClass, TastBlock,
    TastStatement, TastExpression, TastExpressionKind, TastLiteral, TastParameter,
    TastField, BinaryOperator, UnaryOperator, Visibility
};
use super::constraint_solver::{ConstraintSolver, TypeVarId, SolverResult};
use crate::resolver::{ResolvedHirProgram, ResolvedHirFunction, ResolvedHirMethod, ResolvedHirClass, ResolvedHirExpression, ResolvedHirStatement, ResolvedHirBlock, SymbolId, GlobalSymbolTable};
use crate::hir::{HirType, HirBinaryOp, HirUnaryOp};
use crate::error::CompilerError;
use crate::ast::SourceLocation;
use std::collections::HashMap;

/// Type inference engine
#[derive(Debug)]
pub struct TypeInference {
    /// Current type environment mapping symbols to types
    type_env: HashMap<SymbolId, ConcreteType>,
    
    /// Generated type constraints
    constraints: Vec<TypeConstraint>,
    
    /// Type variable generator
    constraint_solver: ConstraintSolver,
    
    /// Symbol table from resolution phase
    symbol_table: GlobalSymbolTable,
    
    /// Built-in types and their methods
    builtins: BuiltinTypes,
    
    /// Current context for inference
    current_function: Option<SymbolId>,
    current_class: Option<SymbolId>,
    current_return_type: Option<ConcreteType>,
    
    /// Errors encountered during inference
    errors: Vec<CompilerError>,
    warnings: Vec<CompilerError>,
}

/// Built-in types and their method signatures
#[derive(Debug, Clone)]
pub struct BuiltinTypes {
    pub integer_methods: HashMap<String, ConcreteType>,
    pub number_methods: HashMap<String, ConcreteType>,
    pub string_methods: HashMap<String, ConcreteType>,
    pub boolean_methods: HashMap<String, ConcreteType>,
    pub array_methods: HashMap<String, ConcreteType>,
}

/// Result of type inference
#[derive(Debug)]
pub struct InferenceResult {
    pub tast: TastProgram,
    pub type_env: HashMap<SymbolId, ConcreteType>,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<CompilerError>,
}

impl TypeInference {
    /// Create a new type inference engine
    pub fn new(symbol_table: GlobalSymbolTable) -> Self {
        Self {
            type_env: HashMap::new(),
            constraints: Vec::new(),
            constraint_solver: ConstraintSolver::new(),
            symbol_table,
            builtins: BuiltinTypes::new(),
            current_function: None,
            current_class: None,
            current_return_type: None,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
    
    /// Perform type inference on a resolved HIR program
    pub fn infer_types(mut self, program: ResolvedHirProgram) -> InferenceResult {
        // Initialize built-in types in type environment
        self.initialize_builtins();
        
        // Infer types for all program elements
        let tast_program = self.infer_program(&program);
        
        // Solve generated constraints
        let mut solver = std::mem::replace(&mut self.constraint_solver, ConstraintSolver::new());
        solver.add_constraints(std::mem::take(&mut self.constraints));
        let solver_result = solver.solve();
        
        // Apply final substitutions to type environment
        self.apply_substitutions(&solver_result);
        
        // Collect errors
        self.errors.extend(solver_result.errors);
        
        InferenceResult {
            tast: tast_program,
            type_env: self.type_env,
            errors: self.errors,
            warnings: self.warnings,
        }
    }
    
    /// Initialize built-in types and add them to type environment
    fn initialize_builtins(&mut self) {
        // Built-in functions would be added here
        // For now, we'll add them as we encounter them during inference
    }
    
    /// Infer types for the entire program
    fn infer_program(&mut self, program: &ResolvedHirProgram) -> TastProgram {
        let mut tast_functions = Vec::new();
        let mut tast_classes = Vec::new();
        
        // First pass: Register all function and class signatures
        for function in &program.functions {
            self.register_function_signature(function);
        }
        
        for class in &program.classes {
            self.register_class_signature(class);
        }
        
        // Second pass: Infer function bodies
        for function in &program.functions {
            if let Ok(tast_function) = self.infer_function(function) {
                tast_functions.push(tast_function);
            }
        }
        
        // Third pass: Infer class method bodies
        for class in &program.classes {
            if let Ok(tast_class) = self.infer_class(class) {
                tast_classes.push(tast_class);
            }
        }
        
        // Handle start function
        let tast_start_function = if let Some(start_fn) = &program.start_function {
            self.infer_function(start_fn).ok()
        } else {
            None
        };
        
        TastProgram {
            functions: tast_functions,
            classes: tast_classes,
            start_function: tast_start_function,
            imports: Vec::new(), // Would convert imports here
            tests: Vec::new(),   // Would convert tests here
            type_env: self.type_env.clone(),
            location: program.location.clone(),
        }
    }
    
    /// Register function signature in type environment
    fn register_function_signature(&mut self, function: &ResolvedHirFunction) {
        let param_types: Vec<ConcreteType> = function.parameters.iter()
            .map(|p| self.hir_type_to_concrete(&p.param_type))
            .collect();
        
        let return_type = if let Some(ref rt) = function.return_type {
            self.hir_type_to_concrete(rt)
        } else {
            ConcreteType::Undefined
        };
        
        let function_type = ConcreteType::Function {
            parameters: param_types,
            return_type: Box::new(return_type),
            is_async: function.is_async,
        };
        
        self.type_env.insert(function.symbol_id, function_type);
    }
    
    /// Register method signature in type environment
    fn register_method_signature(&mut self, method: &ResolvedHirMethod) {
        let param_types: Vec<ConcreteType> = method.parameters.iter()
            .map(|p| self.hir_type_to_concrete(&p.param_type))
            .collect();
        
        let return_type = self.hir_type_to_concrete(&method.return_type);
        
        let function_type = ConcreteType::Function {
            parameters: param_types,
            return_type: Box::new(return_type),
            is_async: false, // Methods are not async by default
        };
        
        self.type_env.insert(method.symbol_id, function_type);
    }
    
    /// Register class signature in type environment
    fn register_class_signature(&mut self, class: &ResolvedHirClass) {
        let class_type = ConcreteType::Class {
            symbol_id: class.symbol_id,
            type_args: Vec::new(), // Would handle generics here
        };
        
        self.type_env.insert(class.symbol_id, class_type);
        
        // Register field types
        for field in &class.fields {
            let field_type = self.hir_type_to_concrete(&field.field_type);
            self.type_env.insert(field.symbol_id, field_type);
        }
        
        // Register method signatures
        for method in &class.methods {
            self.register_method_signature(method);
        }
    }
    
    /// Infer types for a function
    fn infer_function(&mut self, function: &ResolvedHirFunction) -> Result<TastFunction, CompilerError> {
        self.current_function = Some(function.symbol_id);
        self.current_return_type = Some(if let Some(ref return_type) = function.return_type {
            self.hir_type_to_concrete(return_type)
        } else {
            ConcreteType::Undefined
        });
        
        // Add parameters to type environment
        let mut tast_parameters = Vec::new();
        for param in &function.parameters {
            let param_type = self.hir_type_to_concrete(&param.param_type);
            self.type_env.insert(param.symbol_id, param_type.clone());
            
            let default_value = if let Some(default_expr) = &param.default_value {
                Some(self.infer_expression(default_expr)?)
            } else {
                None
            };
            
            tast_parameters.push(TastParameter {
                symbol_id: param.symbol_id,
                name: param.name.clone(),
                param_type,
                default_value,
                is_variadic: param.is_variadic,
                location: param.location.clone(),
            });
        }
        
        // Infer function body
        let tast_body = self.infer_block(&function.body)?;
        
        // Check return type consistency
        let inferred_return_type = tast_body.return_type.clone();
        let declared_return_type = if let Some(ref return_type) = function.return_type {
            self.hir_type_to_concrete(return_type)
        } else {
            ConcreteType::Undefined
        };
        
        self.add_constraint(TypeConstraint::Equality {
            left: inferred_return_type,
            right: declared_return_type.clone(),
            location: function.location.clone(),
        });
        
        self.current_function = None;
        self.current_return_type = None;
        
        Ok(TastFunction {
            symbol_id: function.symbol_id,
            name: function.name.clone(),
            parameters: tast_parameters,
            return_type: declared_return_type,
            body: tast_body,
            generic_params: Vec::new(), // Would handle generics here
            constraints: Vec::new(),
            is_async: function.is_async,
            visibility: Visibility::Public, // Would get from HIR
            location: function.location.clone(),
        })
    }
    
    /// Infer types for a method (similar to function but handles methods)
    fn infer_method(&mut self, method: &ResolvedHirMethod) -> Result<TastFunction, CompilerError> {
        self.current_function = Some(method.symbol_id);
        self.current_return_type = Some(self.hir_type_to_concrete(&method.return_type));
        
        // Add parameters to type environment
        let mut tast_parameters = Vec::new();
        for param in &method.parameters {
            let param_type = self.hir_type_to_concrete(&param.param_type);
            self.type_env.insert(param.symbol_id, param_type.clone());
            
            tast_parameters.push(TastParameter {
                symbol_id: param.symbol_id,
                name: param.name.clone(),
                param_type: param_type,
                default_value: None, // Would convert from HIR
                is_variadic: param.is_variadic,
                location: param.location.clone(),
            });
        }
        
        // Infer body
        let tast_body = self.infer_block(&method.body)?;
        
        let declared_return_type = self.hir_type_to_concrete(&method.return_type);
        
        Ok(TastFunction {
            symbol_id: method.symbol_id,
            name: method.name.clone(),
            parameters: tast_parameters,
            return_type: declared_return_type,
            body: tast_body,
            generic_params: Vec::new(), // Would handle generics here
            constraints: Vec::new(),
            is_async: false, // Methods are typically not async
            visibility: Visibility::Public, // Would get from HIR
            location: method.location.clone(),
        })
    }
    
    /// Infer types for a class
    fn infer_class(&mut self, class: &ResolvedHirClass) -> Result<TastClass, CompilerError> {
        self.current_class = Some(class.symbol_id);
        
        // Convert fields
        let mut tast_fields = Vec::new();
        for field in &class.fields {
            let field_type = self.hir_type_to_concrete(&field.field_type);
            
            let default_value = if let Some(init_expr) = &field.initializer {
                Some(self.infer_expression(init_expr)?)
            } else {
                None
            };
            
            tast_fields.push(TastField {
                symbol_id: field.symbol_id,
                name: field.name.clone(),
                field_type,
                default_value,
                is_static: false, // ResolvedHirField doesn't have is_static field
                is_readonly: false, // Would get from HIR
                visibility: Visibility::Public, // Would get from HIR
                location: field.location.clone(),
            });
        }
        
        // Convert methods
        let mut tast_methods = Vec::new();
        for method in &class.methods {
            if let Ok(tast_method) = self.infer_method(method) {
                tast_methods.push(tast_method);
            }
        }
        
        self.current_class = None;
        
        Ok(TastClass {
            symbol_id: class.symbol_id,
            name: class.name.clone(),
            fields: tast_fields,
            methods: tast_methods,
            constructors: Vec::new(), // Would handle constructors
            parent_class: class.parent,
            interfaces: Vec::new(), // Would handle interfaces
            generic_params: Vec::new(), // Would handle generics
            is_abstract: false, // Would get from HIR
            visibility: Visibility::Public, // Would get from HIR
            location: class.location.clone(),
        })
    }
    
    /// Infer types for a block
    fn infer_block(&mut self, block: &ResolvedHirBlock) -> Result<TastBlock, CompilerError> {
        let mut tast_statements = Vec::new();
        let mut block_return_type = ConcreteType::Null;
        
        for statement in &block.statements {
            let tast_statement = self.infer_statement(statement)?;
            
            // Update block return type based on statement
            match &tast_statement {
                TastStatement::Return { return_type, .. } => {
                    block_return_type = return_type.clone();
                }
                TastStatement::Expression { expression, .. } => {
                    // Last expression in block becomes return type
                    block_return_type = expression.expr_type.clone();
                }
                _ => {}
            }
            
            tast_statements.push(tast_statement);
        }
        
        Ok(TastBlock {
            statements: tast_statements,
            scope_id: crate::resolver::symbol_table::ScopeId(0), // ResolvedHirBlock doesn't track scope_id
            return_type: block_return_type,
            location: block.location.clone(),
        })
    }
    
    /// Infer types for a statement
    fn infer_statement(&mut self, statement: &ResolvedHirStatement) -> Result<TastStatement, CompilerError> {
        match statement {
            ResolvedHirStatement::Expression { expression, location } => {
                let tast_expression = self.infer_expression(expression)?;
                Ok(TastStatement::Expression {
                    expression: tast_expression,
                    location: location.clone(),
                })
            }
            
            ResolvedHirStatement::VariableDeclaration { symbol_id, name, var_type, initializer, location } => {
                let declared_type = self.hir_type_to_concrete(var_type);
                
                let tast_initializer = if let Some(init_expr) = initializer {
                    let tast_init = self.infer_expression(init_expr)?;
                    
                    // Add constraint that initializer type matches declared type
                    self.add_constraint(TypeConstraint::Equality {
                        left: tast_init.expr_type.clone(),
                        right: declared_type.clone(),
                        location: location.clone(),
                    });
                    
                    Some(tast_init)
                } else {
                    None
                };
                
                // Add variable to type environment
                self.type_env.insert(*symbol_id, declared_type.clone());
                
                Ok(TastStatement::VariableDeclaration {
                    symbol_id: *symbol_id,
                    name: name.clone(),
                    var_type: declared_type,
                    initializer: tast_initializer,
                    is_mutable: false, // ResolvedHirStatement doesn't track mutability
                    location: location.clone(),
                })
            }
            
            ResolvedHirStatement::Return { value, location } => {
                let return_type = if let Some(return_expr) = value {
                    let tast_expr = self.infer_expression(return_expr)?;
                    let expr_type = tast_expr.expr_type.clone();
                    
                    // Check against function return type
                    if let Some(expected_return_type) = &self.current_return_type {
                        self.add_constraint(TypeConstraint::Equality {
                            left: expr_type.clone(),
                            right: expected_return_type.clone(),
                            location: location.clone(),
                        });
                    }
                    
                    Some(tast_expr)
                } else {
                    // Void return
                    if let Some(expected_return_type) = &self.current_return_type {
                        self.add_constraint(TypeConstraint::Equality {
                            left: ConcreteType::Null,
                            right: expected_return_type.clone(),
                            location: location.clone(),
                        });
                    }
                    None
                };
                
                let return_expr_type = return_type.as_ref()
                    .map(|e| e.expr_type.clone())
                    .unwrap_or(ConcreteType::Null);
                
                Ok(TastStatement::Return {
                    value: return_type,
                    return_type: return_expr_type,
                    location: location.clone(),
                })
            }
            
            ResolvedHirStatement::Print { expression, newline, location } => {
                let tast_expression = self.infer_expression(expression)?;
                
                // Print statements should work with any type
                Ok(TastStatement::Print {
                    expression: tast_expression,
                    newline: *newline,
                    location: location.clone(),
                })
            }
            
            ResolvedHirStatement::If { condition, then_branch, else_branch, location } => {
                // Infer condition type and ensure it's boolean
                let tast_condition = self.infer_expression(condition)?;
                self.add_constraint(TypeConstraint::Equality {
                    left: tast_condition.expr_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });

                // Infer then branch
                let tast_then_block = self.infer_block(then_branch)?;
                
                // Infer else branch if present
                let tast_else_block = if let Some(else_block) = else_branch {
                    Some(self.infer_block(else_block)?)
                } else {
                    None
                };

                // Determine result type - if both branches exist, find common type
                let result_type = if let Some(else_tast) = &tast_else_block {
                    // Both branches exist, find common type
                    self.find_common_type(&tast_then_block.return_type, &else_tast.return_type)
                } else {
                    // Only then branch, result is unit (void)
                    ConcreteType::Null
                };

                Ok(TastStatement::If {
                    condition: tast_condition,
                    then_block: tast_then_block,
                    else_block: tast_else_block,
                    result_type,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::While { condition, body, location } => {
                // Infer condition type and ensure it's boolean
                let tast_condition = self.infer_expression(condition)?;
                self.add_constraint(TypeConstraint::Equality {
                    left: tast_condition.expr_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });

                // Infer body
                let tast_body = self.infer_block(body)?;

                Ok(TastStatement::While {
                    condition: tast_condition,
                    body: tast_body,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::For { variable, variable_symbol_id, iterable, body, location } => {
                // Infer iterable type
                let tast_iterable = self.infer_expression(iterable)?;
                
                // Extract element type from iterable (Array<T> -> T)
                let element_type = match &tast_iterable.expr_type {
                    ConcreteType::Array(element_type) => (**element_type).clone(),
                    _ => {
                        // Add constraint that iterable must be an array
                        let element_var = self.create_type_variable();
                        self.add_constraint(TypeConstraint::Equality {
                            left: tast_iterable.expr_type.clone(),
                            right: ConcreteType::Array(Box::new(element_var.clone())),
                            location: location.clone(),
                        });
                        element_var
                    }
                };

                // Add loop variable to type environment
                self.type_env.insert(*variable_symbol_id, element_type);

                // Infer body
                let tast_body = self.infer_block(body)?;

                Ok(TastStatement::For {
                    iterator: *variable_symbol_id,
                    iterable: tast_iterable,
                    body: tast_body,
                    location: location.clone(),
                })
            }

            // Handle any remaining unimplemented statement types
            _ => {
                self.errors.push(CompilerError::type_error(
                    "Statement type not yet implemented in type inference",
                    None,
                    Some(statement.location().clone()),
                ));
                
                Ok(TastStatement::Expression {
                    expression: TastExpression {
                        kind: TastExpressionKind::Literal {
                            value: TastLiteral::Null,
                        },
                        expr_type: ConcreteType::Unknown,
                        location: statement.location().clone(),
                    },
                    location: statement.location().clone(),
                })
            }
        }
    }
    
    /// Infer types for an expression
    fn infer_expression(&mut self, expression: &ResolvedHirExpression) -> Result<TastExpression, CompilerError> {
        let (kind, expr_type, location) = match expression {
            ResolvedHirExpression::Literal { value, location } => {
                let (tast_literal, literal_type) = self.infer_literal(value);
                (TastExpressionKind::Literal { value: tast_literal }, literal_type, location.clone())
            }
            
            ResolvedHirExpression::Variable { symbol_id, name, location } => {
                let var_type = self.type_env.get(symbol_id)
                    .cloned()
                    .unwrap_or_else(|| {
                        self.errors.push(CompilerError::type_error(
                            &format!("Variable {} not found in type environment", name),
                            None,
                            Some(location.clone()),
                        ));
                        ConcreteType::Unknown
                    });
                
                (TastExpressionKind::Variable {
                    symbol_id: *symbol_id,
                    name: name.clone(),
                }, var_type, location.clone())
            }
            
            ResolvedHirExpression::BinaryOp { left, op, right, location } => {
                let tast_left = self.infer_expression(left)?;
                let tast_right = self.infer_expression(right)?;
                
                let result_type = self.infer_binary_operation(op, &tast_left.expr_type, &tast_right.expr_type, location)?;
                
                (TastExpressionKind::BinaryOperation {
                    operator: self.convert_binary_operator(op),
                    left: Box::new(tast_left),
                    right: Box::new(tast_right),
                }, result_type, location.clone())
            }
            
            ResolvedHirExpression::Call { function, function_symbol_id, arguments, location } => {
                let mut tast_arguments = Vec::new();
                
                for arg in arguments {
                    tast_arguments.push(self.infer_expression(arg)?);
                }
                
                // Look up function type and determine return type
                let return_type = self.infer_function_return_type(*function_symbol_id, &tast_arguments)?;
                
                (TastExpressionKind::FunctionCall {
                    function: Box::new(TastExpression {
                        kind: TastExpressionKind::Variable {
                            symbol_id: *function_symbol_id,
                            name: function.clone(),
                        },
                        expr_type: ConcreteType::Function {
                            parameters: tast_arguments.iter().map(|a| a.expr_type.clone()).collect(),
                            return_type: Box::new(return_type.clone()),
                            is_async: false,
                        },
                        location: location.clone(),
                    }),
                    arguments: tast_arguments,
                    type_args: Vec::new(),
                }, return_type, location.clone())
            }
            
            ResolvedHirExpression::Index { array, index, location } => {
                let tast_array = self.infer_expression(array)?;
                let tast_index = self.infer_expression(index)?;
                
                // Verify index type is integer
                if !matches!(tast_index.expr_type, ConcreteType::Integer) {
                    self.errors.push(CompilerError::type_error(
                        &format!("Array index must be integer, found {:?}", tast_index.expr_type),
                        None,
                        Some(location.clone()),
                    ));
                }
                
                // Extract element type from array type
                let element_type = match &tast_array.expr_type {
                    ConcreteType::Array(element_type) => (**element_type).clone(),
                    other_type => {
                        self.errors.push(CompilerError::type_error(
                            &format!("Cannot index into non-array type: {:?}", other_type),
                            None,
                            Some(location.clone()),
                        ));
                        ConcreteType::Unknown
                    }
                };
                
                (TastExpressionKind::ArrayAccess {
                    array: Box::new(tast_array),
                    index: Box::new(tast_index),
                }, element_type, location.clone())
            }
            
            ResolvedHirExpression::MethodCall { receiver, method, method_symbol_id, arguments, location } => {
                let tast_receiver = self.infer_expression(receiver)?;
                
                let mut tast_arguments = Vec::new();
                for arg in arguments {
                    tast_arguments.push(self.infer_expression(arg)?);
                }
                
                // For now, use simple method resolution based on receiver type
                let return_type = self.infer_method_return_type(method, &tast_receiver.expr_type, &tast_arguments)?;
                
                (TastExpressionKind::MethodCall {
                    receiver: Box::new(tast_receiver),
                    method_name: method.clone(),
                    method_symbol: method_symbol_id.unwrap_or(crate::resolver::symbol_table::SymbolId(0)), // Use dummy SymbolId for built-in methods
                    arguments: tast_arguments,
                    type_args: Vec::new(),
                }, return_type, location.clone())
            }
            
            // Would implement other expression types here
            _ => {
                let location = match expression {
                    ResolvedHirExpression::Literal { location, .. } => location,
                    ResolvedHirExpression::Variable { location, .. } => location,
                    ResolvedHirExpression::BinaryOp { location, .. } => location,
                    ResolvedHirExpression::UnaryOp { location, .. } => location,
                    ResolvedHirExpression::Call { location, .. } => location,
                    ResolvedHirExpression::MethodCall { location, .. } => location,
                    ResolvedHirExpression::FieldAccess { location, .. } => location,
                    ResolvedHirExpression::Index { location, .. } => location,
                    ResolvedHirExpression::Array { location, .. } => location,
                    ResolvedHirExpression::Constructor { location, .. } => location,
                    ResolvedHirExpression::This { location, .. } => location,
                    ResolvedHirExpression::Cast { location, .. } => location,
                    ResolvedHirExpression::Assignment { location, .. } => location,
                };
                
                self.errors.push(CompilerError::type_error(
                    "Expression type not yet implemented in type inference",
                    None,
                    Some(location.clone()),
                ));
                
                (TastExpressionKind::Literal {
                    value: TastLiteral::Null,
                }, ConcreteType::Unknown, location.clone())
            }
        };
        
        Ok(TastExpression {
            kind,
            expr_type,
            location,
        })
    }
    
    /// Infer type for a literal
    fn infer_literal(&self, literal: &crate::ast::Value) -> (TastLiteral, ConcreteType) {
        match literal {
            crate::ast::Value::Integer(value) => (TastLiteral::Integer(*value), ConcreteType::Integer),
            crate::ast::Value::Number(value) => (TastLiteral::Number(*value), ConcreteType::Number),
            crate::ast::Value::String(value) => (TastLiteral::String(value.clone()), ConcreteType::String),
            crate::ast::Value::Boolean(value) => (TastLiteral::Boolean(*value), ConcreteType::Boolean),
            crate::ast::Value::Void => (TastLiteral::Null, ConcreteType::Null),
            _ => (TastLiteral::Null, ConcreteType::Unknown), // Handle other value types
        }
    }
    
    /// Infer result type of binary operation
    fn infer_binary_operation(&mut self, operator: &HirBinaryOp, left_type: &ConcreteType, right_type: &ConcreteType, location: &SourceLocation) -> Result<ConcreteType, CompilerError> {
        match operator {
            HirBinaryOp::Add | HirBinaryOp::Subtract | HirBinaryOp::Multiply | HirBinaryOp::Divide | 
            HirBinaryOp::Modulo | HirBinaryOp::Power => {
                // Arithmetic operations require numeric types
                self.add_constraint(TypeConstraint::Subtype {
                    subtype: left_type.clone(),
                    supertype: ConcreteType::Number,
                    location: location.clone(),
                });
                
                self.add_constraint(TypeConstraint::Subtype {
                    subtype: right_type.clone(),
                    supertype: ConcreteType::Number,
                    location: location.clone(),
                });
                
                // Result is the common supertype of operands
                Ok(left_type.common_supertype(right_type))
            }
            
            HirBinaryOp::Equal | HirBinaryOp::NotEqual => {
                // Equality can compare any types
                Ok(ConcreteType::Boolean)
            }
            
            HirBinaryOp::Less | HirBinaryOp::LessEqual | 
            HirBinaryOp::Greater | HirBinaryOp::GreaterEqual => {
                // Comparison requires compatible types
                self.add_constraint(TypeConstraint::Equality {
                    left: left_type.clone(),
                    right: right_type.clone(),
                    location: location.clone(),
                });
                
                Ok(ConcreteType::Boolean)
            }
            
            HirBinaryOp::And | HirBinaryOp::Or => {
                // Logical operations require boolean types
                self.add_constraint(TypeConstraint::Equality {
                    left: left_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });
                
                self.add_constraint(TypeConstraint::Equality {
                    left: right_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });
                
                Ok(ConcreteType::Boolean)
            }
            
            HirBinaryOp::StringConcat => {
                // String concatenation
                self.add_constraint(TypeConstraint::Equality {
                    left: left_type.clone(),
                    right: ConcreteType::String,
                    location: location.clone(),
                });
                
                self.add_constraint(TypeConstraint::Equality {
                    left: right_type.clone(),
                    right: ConcreteType::String,
                    location: location.clone(),
                });
                
                Ok(ConcreteType::String)
            }
        }
    }
    
    /// Infer return type of function call
    fn infer_function_call(&mut self, function_type: &ConcreteType, arguments: &[TastExpression], location: &SourceLocation) -> Result<ConcreteType, CompilerError> {
        match function_type {
            ConcreteType::Function { parameters, return_type, .. } => {
                if parameters.len() != arguments.len() {
                    return Err(CompilerError::type_error(
                        &format!("Function expects {} arguments, got {}", parameters.len(), arguments.len()),
                        None,
                        Some(location.clone()),
                    ));
                }
                
                // Check argument types match parameters
                for (param_type, arg) in parameters.iter().zip(arguments.iter()) {
                    self.add_constraint(TypeConstraint::Equality {
                        left: arg.expr_type.clone(),
                        right: param_type.clone(),
                        location: location.clone(),
                    });
                }
                
                Ok((**return_type).clone())
            }
            
            _ => Err(CompilerError::type_error(
                &format!("Cannot call non-function type: {}", function_type),
                None,
                Some(location.clone()),
            ))
        }
    }
    
    /// Get function return type from symbol table
    fn infer_function_return_type(&self, function_symbol_id: SymbolId, _arguments: &[TastExpression]) -> Result<ConcreteType, CompilerError> {
        // Look up function type from symbol table
        if let Some(function_type) = self.type_env.get(&function_symbol_id) {
            match function_type {
                ConcreteType::Function { return_type, .. } => {
                    Ok((**return_type).clone())
                }
                _ => Err(CompilerError::type_error(
                    "Symbol is not a function",
                    None,
                    None,
                ))
            }
        } else {
            Err(CompilerError::type_error(
                &format!("Function symbol {:?} not found in type environment", function_symbol_id),
                None,
                None,
            ))
        }
    }
    
    /// Infer return type of method call
    fn infer_method_return_type(&self, method_name: &str, receiver_type: &ConcreteType, _arguments: &[TastExpression]) -> Result<ConcreteType, CompilerError> {
        // For now, implement basic built-in method type inference
        match (receiver_type, method_name) {
            // Integer methods
            (ConcreteType::Integer, "toString") => Ok(ConcreteType::String),
            (ConcreteType::Integer, "abs") => Ok(ConcreteType::Integer),
            
            // Number methods
            (ConcreteType::Number, "toString") => Ok(ConcreteType::String),
            (ConcreteType::Number, "abs") => Ok(ConcreteType::Number),
            (ConcreteType::Number, "floor") => Ok(ConcreteType::Integer),
            (ConcreteType::Number, "ceil") => Ok(ConcreteType::Integer),
            (ConcreteType::Number, "round") => Ok(ConcreteType::Integer),
            
            // String methods
            (ConcreteType::String, "length") => Ok(ConcreteType::Integer),
            (ConcreteType::String, "toUpperCase") => Ok(ConcreteType::String),
            (ConcreteType::String, "toLowerCase") => Ok(ConcreteType::String),
            (ConcreteType::String, "trim") => Ok(ConcreteType::String),
            
            // Array methods
            (ConcreteType::Array(element_type), "length") => Ok(ConcreteType::Integer),
            (ConcreteType::Array(element_type), "push") => Ok(ConcreteType::Undefined), // void return
            (ConcreteType::Array(element_type), "pop") => Ok((**element_type).clone()),
            (ConcreteType::Array(_), "toString") => Ok(ConcreteType::String),
            
            // Boolean methods
            (ConcreteType::Boolean, "toString") => Ok(ConcreteType::String),
            
            // For unknown method/type combinations, return Unknown
            _ => {
                Ok(ConcreteType::Unknown)
            }
        }
    }
    
    /// Convert HIR type to concrete type
    fn hir_type_to_concrete(&self, hir_type: &HirType) -> ConcreteType {
        match hir_type {
            HirType::Integer => ConcreteType::Integer,
            HirType::Number => ConcreteType::Number,
            HirType::String => ConcreteType::String,
            HirType::Boolean => ConcreteType::Boolean,
            HirType::Void => ConcreteType::Undefined,
            HirType::Integer8 => ConcreteType::Integer,
            HirType::Integer8u => ConcreteType::Integer,
            HirType::Integer16 => ConcreteType::Integer,
            HirType::Integer16u => ConcreteType::Integer,
            HirType::Integer32 => ConcreteType::Integer,
            HirType::Integer64 => ConcreteType::Integer,
            HirType::Number32 => ConcreteType::Number,
            HirType::Number64 => ConcreteType::Number,
            HirType::List(element_type) => {
                ConcreteType::Array(Box::new(self.hir_type_to_concrete(element_type)))
            }
            HirType::Matrix(element_type) => {
                // Matrix is a specialized form of nested arrays
                ConcreteType::Array(Box::new(ConcreteType::Array(Box::new(self.hir_type_to_concrete(element_type)))))
            }
            HirType::Named { name, .. } => {
                // For user-defined types, we need to look them up in the symbol table
                // For now, we'll treat them as unknown types that will be resolved later
                ConcreteType::Undefined
            }
            HirType::Inferred { .. } => {
                // Type inference placeholders are handled by the constraint solver
                ConcreteType::Undefined
            }
        }
    }
    
    /// Convert HIR binary operator to TAST binary operator
    fn convert_binary_operator(&self, operator: &HirBinaryOp) -> BinaryOperator {
        match operator {
            HirBinaryOp::Add => BinaryOperator::Add,
            HirBinaryOp::Subtract => BinaryOperator::Subtract,
            HirBinaryOp::Multiply => BinaryOperator::Multiply,
            HirBinaryOp::Divide => BinaryOperator::Divide,
            HirBinaryOp::Modulo => BinaryOperator::Modulo,
            HirBinaryOp::Power => BinaryOperator::Power,
            HirBinaryOp::Equal => BinaryOperator::Equal,
            HirBinaryOp::NotEqual => BinaryOperator::NotEqual,
            HirBinaryOp::Less => BinaryOperator::LessThan,
            HirBinaryOp::LessEqual => BinaryOperator::LessThanOrEqual,
            HirBinaryOp::Greater => BinaryOperator::GreaterThan,
            HirBinaryOp::GreaterEqual => BinaryOperator::GreaterThanOrEqual,
            HirBinaryOp::And => BinaryOperator::And,
            HirBinaryOp::Or => BinaryOperator::Or,
            HirBinaryOp::StringConcat => BinaryOperator::Concatenate,
        }
    }
    
    /// Add a constraint to be solved
    fn add_constraint(&mut self, constraint: TypeConstraint) {
        self.constraints.push(constraint);
    }
    
    /// Apply solved substitutions to type environment
    fn apply_substitutions(&mut self, solver_result: &SolverResult) {
        // Would apply substitutions to finalize types
        // For now, types in type_env are already concrete
    }

    /// Find a common type between two types for control flow branches
    fn find_common_type(&self, left: &ConcreteType, right: &ConcreteType) -> ConcreteType {
        if left == right {
            left.clone()
        } else {
            match (left, right) {
                // If one type is assignable to the other, use the more general type
                (l, r) if l.is_assignable_to(r) => r.clone(),
                (l, r) if r.is_assignable_to(l) => l.clone(),
                
                // Both numeric types -> Number (more general)
                (ConcreteType::Integer, ConcreteType::Number) | 
                (ConcreteType::Number, ConcreteType::Integer) => ConcreteType::Number,
                
                // Array types with compatible elements
                (ConcreteType::Array(left_elem), 
                 ConcreteType::Array(right_elem)) => {
                    let common_elem = self.find_common_type(left_elem, right_elem);
                    ConcreteType::Array(Box::new(common_elem))
                }
                
                // Otherwise, fall back to Unknown for error recovery
                _ => ConcreteType::Unknown,
            }
        }
    }

    /// Create a fresh type variable for type inference
    fn create_type_variable(&self) -> ConcreteType {
        // For now, use Unknown as a type variable placeholder
        // A full implementation would generate unique type variable IDs
        ConcreteType::Unknown
    }
}

impl BuiltinTypes {
    /// Initialize built-in type method signatures
    fn new() -> Self {
        let mut string_methods = HashMap::new();
        string_methods.insert("length".to_string(), ConcreteType::Integer);
        string_methods.insert("substring".to_string(), ConcreteType::Function {
            parameters: vec![ConcreteType::Integer, ConcreteType::Integer],
            return_type: Box::new(ConcreteType::String),
            is_async: false,
        });
        
        let mut array_methods = HashMap::new();
        array_methods.insert("length".to_string(), ConcreteType::Integer);
        array_methods.insert("push".to_string(), ConcreteType::Function {
            parameters: vec![ConcreteType::Generic { name: "T".to_string(), bounds: vec![] }],
            return_type: Box::new(ConcreteType::Integer),
            is_async: false,
        });
        
        Self {
            integer_methods: HashMap::new(),
            number_methods: HashMap::new(),
            string_methods,
            boolean_methods: HashMap::new(),
            array_methods,
        }
    }
}

// Extension trait to get location from HIR statements
trait StatementLocation {
    fn location(&self) -> &SourceLocation;
}

impl StatementLocation for ResolvedHirStatement {
    fn location(&self) -> &SourceLocation {
        match self {
            ResolvedHirStatement::Expression { location, .. } => location,
            ResolvedHirStatement::VariableDeclaration { location, .. } => location,
            ResolvedHirStatement::Return { location, .. } => location,
            ResolvedHirStatement::If { location, .. } => location,
            ResolvedHirStatement::While { location, .. } => location,
            ResolvedHirStatement::Assignment { location, .. } => location,
            ResolvedHirStatement::For { location, .. } => location,
            ResolvedHirStatement::Print { location, .. } => location,
        }
    }
}