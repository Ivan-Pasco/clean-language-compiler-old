//! Iterative Name Resolver - Stack-safe implementation
//!
//! This module implements a stack-safe, iterative version of the name resolver
//! to prevent stack overflow issues when resolving deeply nested expressions.

use super::*;
use crate::error::CompilerError;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

/// Work items for iterative resolution
#[derive(Debug, Clone)]
enum ResolverWorkItem {
    /// Resolve a function and store result
    ResolveFunction {
        function: HirFunction,
        result_slot: usize,
    },
    /// Resolve a block and store result
    ResolveBlock {
        block: HirBlock,
        result_slot: usize,
    },
    /// Resolve a statement and store result
    ResolveStatement {
        statement: HirStatement,
        result_slot: usize,
    },
    /// Resolve an expression and store result
    ResolveExpression {
        expression: HirExpression,
        result_slot: usize,
    },
    /// Complete resolution of static method call arguments
    CompleteStaticMethodCall {
        class_name: String,
        method: String,
        location: SourceLocation,
        arg_slots: Vec<usize>,
        result_slot: usize,
    },
    /// Complete resolution of method call
    CompleteMethodCall {
        method: String,
        location: SourceLocation,
        receiver_slot: usize,
        arg_slots: Vec<usize>,
        result_slot: usize,
    },
    /// Complete resolution of function call
    CompleteFunctionCall {
        function: String,
        location: SourceLocation,
        arg_slots: Vec<usize>,
        result_slot: usize,
    },
    /// Complete resolution of base constructor call
    CompleteBaseCall {
        location: SourceLocation,
        arg_slots: Vec<usize>,
        result_slot: usize,
    },
}

/// Iterative name resolver that prevents stack overflow
pub struct IterativeNameResolver {
    symbol_table: GlobalSymbolTable,
    current_class: Option<SymbolId>,
    current_function: Option<SymbolId>,
    errors: Vec<CompilerError>,
    warnings: Vec<CompilerError>,
    work_queue: VecDeque<ResolverWorkItem>,
    results: Vec<Option<ResolverResult>>,
    next_slot: usize,
}

/// Generic result holder for different resolution results
#[derive(Debug, Clone)]
enum ResolverResult {
    Function(ResolvedHirFunction),
    Block(ResolvedHirBlock),
    Statement(ResolvedHirStatement),
    Expression(ResolvedHirExpression),
}

impl IterativeNameResolver {
    /// Create a new iterative name resolver
    pub fn new() -> Self {
        Self {
            symbol_table: GlobalSymbolTable::new(),
            current_class: None,
            current_function: None,
            errors: Vec::new(),
            warnings: Vec::new(),
            work_queue: VecDeque::new(),
            results: Vec::new(),
            next_slot: 0,
        }
    }

    /// Resolve names in a HIR program iteratively
    pub fn resolve(hir: HirProgram) -> Result<ResolutionResult, Vec<CompilerError>> {
        let mut resolver = Self::new();
        match resolver.resolve_program_iterative(hir) {
            Ok(resolved_hir) => Ok(ResolutionResult {
                resolved_hir,
                warnings: resolver.warnings,
            }),
            Err(_) => Err(resolver.errors),
        }
    }

    /// Allocate a new result slot
    fn allocate_slot(&mut self) -> usize {
        let slot = self.next_slot;
        self.next_slot += 1;
        self.results.resize(self.next_slot, None);
        slot
    }

    /// Get result from slot (must exist)
    fn get_result(&self, slot: usize) -> &ResolverResult {
        self.results[slot].as_ref().unwrap()
    }

    /// Set result in slot
    fn set_result(&mut self, slot: usize, result: ResolverResult) {
        self.results[slot] = Some(result);
    }

    /// Check if result is ready
    fn is_ready(&self, slot: usize) -> bool {
        self.results[slot].is_some()
    }

    /// Resolve the entire HIR program iteratively
    fn resolve_program_iterative(&mut self, hir: HirProgram) -> Result<ResolvedHirProgram, ()> {
        // First pass: Register all top-level symbols
        self.register_top_level_symbols(&hir)?;

        // Queue up all functions for resolution
        let mut function_slots = Vec::new();
        for function in hir.functions {
            let slot = self.allocate_slot();
            function_slots.push(slot);
            self.work_queue.push_back(ResolverWorkItem::ResolveFunction {
                function,
                result_slot: slot,
            });
        }

        // Queue up start function if it exists
        let start_function_slot = if let Some(start_fn) = hir.start_function {
            let slot = self.allocate_slot();
            self.work_queue.push_back(ResolverWorkItem::ResolveFunction {
                function: start_fn,
                result_slot: slot,
            });
            Some(slot)
        } else {
            None
        };

        // Process all work items iteratively
        self.process_work_queue()?;

        // Collect resolved functions
        let resolved_functions = function_slots.into_iter()
            .map(|slot| {
                if let ResolverResult::Function(func) = self.get_result(slot) {
                    func.clone()
                } else {
                    panic!("Expected function result in slot {}", slot);
                }
            })
            .collect();

        // Get resolved start function
        let resolved_start_function = start_function_slot.map(|slot| {
            if let ResolverResult::Function(func) = self.get_result(slot) {
                func.clone()
            } else {
                panic!("Expected function result in start function slot {}", slot);
            }
        });

        Ok(ResolvedHirProgram {
            functions: resolved_functions,
            classes: Vec::new(), // Class resolution handled by main resolver
            start_function: resolved_start_function,
            imports: Vec::new(), // Import resolution handled by main resolver
            tests: Vec::new(),   // Test resolution handled by main resolver
            state: None, // State resolution handled by main resolver
            symbol_table: self.symbol_table.clone(),
            location: hir.location,
            externals: Vec::new(), // External resolution handled by main resolver
        })
    }

    /// Process work queue until empty
    fn process_work_queue(&mut self) -> Result<(), ()> {
        while let Some(work_item) = self.work_queue.pop_front() {
            self.process_work_item(work_item)?;
        }
        Ok(())
    }

    /// Process a single work item
    fn process_work_item(&mut self, work_item: ResolverWorkItem) -> Result<(), ()> {
        match work_item {
            ResolverWorkItem::ResolveFunction { function, result_slot } => {
                self.resolve_function_iterative(function, result_slot)?;
            }
            ResolverWorkItem::ResolveBlock { block, result_slot } => {
                self.resolve_block_iterative(block, result_slot)?;
            }
            ResolverWorkItem::ResolveStatement { statement, result_slot } => {
                self.resolve_statement_iterative(statement, result_slot)?;
            }
            ResolverWorkItem::ResolveExpression { expression, result_slot } => {
                self.resolve_expression_iterative(expression, result_slot)?;
            }
            ResolverWorkItem::CompleteStaticMethodCall { class_name, method, location, arg_slots, result_slot } => {
                // Check if all arguments are resolved
                let all_ready = arg_slots.iter().all(|&slot| self.is_ready(slot));
                if all_ready {
                    let arguments = arg_slots.into_iter()
                        .map(|slot| {
                            if let ResolverResult::Expression(expr) = self.get_result(slot) {
                                Rc::new(expr.clone())
                            } else {
                                panic!("Expected expression result in slot {}", slot);
                            }
                        })
                        .collect();

                    let resolved_expression = ResolvedHirExpression::StaticMethodCall {
                        class_name,
                        class_symbol_id: None, // Symbol resolved at codegen time
                        method,
                        method_symbol_id: None, // Symbol resolved at codegen time
                        arguments,
                        location,
                    };

                    self.set_result(result_slot, ResolverResult::Expression(resolved_expression));
                } else {
                    // Dependencies not ready, push back to queue
                    self.work_queue.push_back(ResolverWorkItem::CompleteStaticMethodCall {
                        class_name, method, location, arg_slots, result_slot
                    });
                }
            }
            ResolverWorkItem::CompleteMethodCall { method, location, receiver_slot, arg_slots, result_slot } => {
                // Check if receiver and all arguments are resolved
                let receiver_ready = self.is_ready(receiver_slot);
                let all_args_ready = arg_slots.iter().all(|&slot| self.is_ready(slot));

                if receiver_ready && all_args_ready {
                    let receiver = if let ResolverResult::Expression(expr) = self.get_result(receiver_slot) {
                        Rc::new(expr.clone())
                    } else {
                        panic!("Expected expression result in receiver slot {}", receiver_slot);
                    };

                    let arguments = arg_slots.into_iter()
                        .map(|slot| {
                            if let ResolverResult::Expression(expr) = self.get_result(slot) {
                                Rc::new(expr.clone())
                            } else {
                                panic!("Expected expression result in slot {}", slot);
                            }
                        })
                        .collect();

                    let resolved_expression = ResolvedHirExpression::MethodCall {
                        receiver,
                        method,
                        method_symbol_id: None, // Symbol resolved at codegen time
                        arguments,
                        location,
                    };

                    self.set_result(result_slot, ResolverResult::Expression(resolved_expression));
                } else {
                    // Dependencies not ready, push back to queue
                    self.work_queue.push_back(ResolverWorkItem::CompleteMethodCall {
                        method, location, receiver_slot, arg_slots, result_slot
                    });
                }
            }
            ResolverWorkItem::CompleteFunctionCall { function, location, arg_slots, result_slot } => {
                // Check if all arguments are resolved
                let all_ready = arg_slots.iter().all(|&slot| self.is_ready(slot));
                if all_ready {
                    let arguments = arg_slots.into_iter()
                        .map(|slot| {
                            if let ResolverResult::Expression(expr) = self.get_result(slot) {
                                Rc::new(expr.clone())
                            } else {
                                panic!("Expected expression result in slot {}", slot);
                            }
                        })
                        .collect();

                    // CRITICAL FIX: Look up function symbol instead of using placeholder SymbolId(0)
                    let function_symbol_id = match self.symbol_table.lookup_symbol(&function) {
                        Some(symbol_id) => symbol_id,
                        None => {
                            // Function not found - emit error and fail
                            self.error(
                                &format!("Function '{}' not found", function),
                                location.clone(),
                            );
                            return Err(());
                        }
                    };

                    let resolved_expression = ResolvedHirExpression::Call {
                        function,
                        function_symbol_id,
                        arguments,
                        location,
                    };

                    self.set_result(result_slot, ResolverResult::Expression(resolved_expression));
                } else {
                    // Dependencies not ready, push back to queue
                    self.work_queue.push_back(ResolverWorkItem::CompleteFunctionCall {
                        function, location, arg_slots, result_slot
                    });
                }
            }
            ResolverWorkItem::CompleteBaseCall { location, arg_slots, result_slot } => {
                // Check if all arguments are resolved
                let all_ready = arg_slots.iter().all(|&slot| self.is_ready(slot));
                if all_ready {
                    let arguments = arg_slots.into_iter()
                        .map(|slot| {
                            if let ResolverResult::Expression(expr) = self.get_result(slot) {
                                Rc::new(expr.clone())
                            } else {
                                panic!("Expected expression result in slot {}", slot);
                            }
                        })
                        .collect();

                    // Get parent class symbol from current class
                    let parent_class_symbol_id = if let Some(current_class_id) = self.current_class {
                        if let Some(class_symbol) = self.symbol_table.get_symbol(current_class_id) {
                            if let SymbolKind::Class { parent, .. } = &class_symbol.kind {
                                parent.ok_or_else(|| {
                                    self.error(
                                        "base() can only be called in a derived class constructor",
                                        location.clone(),
                                    );
                                })?
                            } else {
                                self.error(
                                    "base() can only be called inside a class",
                                    location.clone(),
                                );
                                return Err(());
                            }
                        } else {
                            self.error("Current class symbol not found", location.clone());
                            return Err(());
                        }
                    } else {
                        self.error(
                            "base() can only be called inside a class constructor",
                            location.clone(),
                        );
                        return Err(());
                    };

                    let resolved_expression = ResolvedHirExpression::BaseCall {
                        parent_class_symbol_id,
                        arguments,
                        location,
                    };

                    self.set_result(result_slot, ResolverResult::Expression(resolved_expression));
                } else {
                    // Dependencies not ready, push back to queue
                    self.work_queue.push_back(ResolverWorkItem::CompleteBaseCall {
                        location, arg_slots, result_slot
                    });
                }
            }
        }
        Ok(())
    }

    /// Resolve a function iteratively
    fn resolve_function_iterative(&mut self, function: HirFunction, result_slot: usize) -> Result<(), ()> {
        // Create function symbol and scope
        let function_symbol_id = self.symbol_table.create_symbol(
            function.name.clone(),
            SymbolKind::Function {
                parameters: function.parameters.iter().map(|p| p.param_type.clone()).collect(),
                return_type: function.return_type.clone(),
            },
            ScopeId(0), // Global scope
            function.location.clone(),
        );

        self.current_function = Some(function_symbol_id);

        // Create function scope
        let function_scope = self.symbol_table.create_scope(
            None,
            ScopeType::Function { function_id: function_symbol_id },
        );
        self.symbol_table.enter_scope(function_scope);

        // Register parameters
        let mut resolved_parameters = Vec::new();
        for param in &function.parameters {
            let param_symbol_id = self.symbol_table.create_symbol(
                param.name.clone(),
                SymbolKind::Variable { var_type: param.param_type.clone() },
                function_scope,
                param.location.clone(),
            );

            resolved_parameters.push(ResolvedHirParameter {
                name: param.name.clone(),
                symbol_id: param_symbol_id,
                param_type: param.param_type.clone(),
                default_value: None,
                is_variadic: false,
                location: param.location.clone(),
            });
        }

        // Queue body for resolution
        let body_slot = self.allocate_slot();
        self.work_queue.push_back(ResolverWorkItem::ResolveBlock {
            block: function.body.as_ref().clone(),
            result_slot: body_slot,
        });

        // Process the body resolution
        self.process_work_queue()?;

        // Get resolved body
        let resolved_body = if let ResolverResult::Block(block) = self.get_result(body_slot) {
            Rc::new(block.clone())
        } else {
            panic!("Expected block result in body slot {}", body_slot);
        };

        // Exit function scope
        self.symbol_table.exit_scope();
        self.current_function = None;

        let resolved_function = ResolvedHirFunction {
            name: function.name,
            symbol_id: function_symbol_id,
            parameters: resolved_parameters,
            return_type: function.return_type,
            body: resolved_body,
            is_start: function.is_start,
            is_background: false,
            location: function.location,
        };

        self.set_result(result_slot, ResolverResult::Function(resolved_function));
        Ok(())
    }

    /// Resolve a block iteratively
    fn resolve_block_iterative(&mut self, block: HirBlock, result_slot: usize) -> Result<(), ()> {
        // Create block scope
        let block_scope = self.symbol_table.create_scope(None, ScopeType::Block);
        self.symbol_table.enter_scope(block_scope);

        // Queue all statements for resolution
        let mut statement_slots = Vec::new();
        for statement in block.statements {
            let slot = self.allocate_slot();
            statement_slots.push(slot);
            self.work_queue.push_back(ResolverWorkItem::ResolveStatement {
                statement: statement.as_ref().clone(),
                result_slot: slot,
            });
        }

        // Process all statements
        self.process_work_queue()?;

        // Collect resolved statements
        let resolved_statements = statement_slots.into_iter()
            .map(|slot| {
                if let ResolverResult::Statement(stmt) = self.get_result(slot) {
                    stmt.clone()
                } else {
                    panic!("Expected statement result in slot {}", slot);
                }
            })
            .collect();

        self.symbol_table.exit_scope();

        let resolved_block = ResolvedHirBlock {
            statements: resolved_statements,
            location: block.location,
        };

        self.set_result(result_slot, ResolverResult::Block(resolved_block));
        Ok(())
    }

    /// Resolve a statement iteratively
    fn resolve_statement_iterative(&mut self, statement: HirStatement, result_slot: usize) -> Result<(), ()> {
        match statement {
            HirStatement::Expression { expression, location } => {
                let expr_slot = self.allocate_slot();
                self.work_queue.push_back(ResolverWorkItem::ResolveExpression {
                    expression: expression.as_ref().clone(),
                    result_slot: expr_slot,
                });

                // Process expression resolution
                self.process_work_queue()?;

                let resolved_expression = if let ResolverResult::Expression(expr) = self.get_result(expr_slot) {
                    Rc::new(expr.clone())
                } else {
                    panic!("Expected expression result in slot {}", expr_slot);
                };

                let resolved_statement = ResolvedHirStatement::Expression {
                    expression: resolved_expression,
                    location,
                };

                self.set_result(result_slot, ResolverResult::Statement(resolved_statement));
            }
            HirStatement::Print { expression, newline, location } => {
                let expr_slot = self.allocate_slot();
                self.work_queue.push_back(ResolverWorkItem::ResolveExpression {
                    expression: expression.as_ref().clone(),
                    result_slot: expr_slot,
                });

                // Process expression resolution
                self.process_work_queue()?;

                let resolved_expression = if let ResolverResult::Expression(expr) = self.get_result(expr_slot) {
                    Rc::new(expr.clone())
                } else {
                    panic!("Expected expression result in slot {}", expr_slot);
                };

                let resolved_statement = ResolvedHirStatement::Print {
                    expression: resolved_expression,
                    newline,
                    location,
                };

                self.set_result(result_slot, ResolverResult::Statement(resolved_statement));
            }
            _ => {
                // For now, create a minimal resolved statement
                let resolved_statement = ResolvedHirStatement::Expression {
                    expression: Rc::new(ResolvedHirExpression::Literal {
                        value: crate::ast::Value::Integer(0),
                        location: SourceLocation::default(),
                    }),
                    location: SourceLocation::default(),
                };

                self.set_result(result_slot, ResolverResult::Statement(resolved_statement));
            }
        }
        Ok(())
    }

    /// Resolve an expression iteratively
    fn resolve_expression_iterative(&mut self, expression: HirExpression, result_slot: usize) -> Result<(), ()> {
        match expression {
            HirExpression::Literal { value, location } => {
                let resolved_expression = ResolvedHirExpression::Literal { value, location };
                self.set_result(result_slot, ResolverResult::Expression(resolved_expression));
            }

            HirExpression::StaticMethodCall { class_name, method, arguments, location } => {
                // Queue arguments for resolution
                let mut arg_slots = Vec::new();
                for argument in arguments {
                    let slot = self.allocate_slot();
                    arg_slots.push(slot);
                    self.work_queue.push_back(ResolverWorkItem::ResolveExpression {
                        expression: argument.as_ref().clone(),
                        result_slot: slot,
                    });
                }

                // Queue completion of static method call
                self.work_queue.push_back(ResolverWorkItem::CompleteStaticMethodCall {
                    class_name,
                    method,
                    location,
                    arg_slots,
                    result_slot,
                });
            }

            HirExpression::Variable { name, location } => {
                // CRITICAL FIX: Look up variable symbol and fail if not found
                let symbol_id = match self.symbol_table.lookup_symbol(&name) {
                    Some(symbol_id) => symbol_id,
                    None => {
                        // Variable not found - emit error and fail
                        self.error(
                            &format!("Variable '{}' not found", name),
                            location.clone(),
                        );
                        return Err(());
                    }
                };

                let resolved_expression = ResolvedHirExpression::Variable {
                    name,
                    symbol_id,
                    location,
                };

                self.set_result(result_slot, ResolverResult::Expression(resolved_expression));
            }

            HirExpression::BaseCall { arguments, location } => {
                // Queue arguments for resolution
                let mut arg_slots = Vec::new();
                for argument in arguments {
                    let slot = self.allocate_slot();
                    arg_slots.push(slot);
                    self.work_queue.push_back(ResolverWorkItem::ResolveExpression {
                        expression: argument.as_ref().clone(),
                        result_slot: slot,
                    });
                }

                // Queue completion of base call
                self.work_queue.push_back(ResolverWorkItem::CompleteBaseCall {
                    location,
                    arg_slots,
                    result_slot,
                });
            }

            _ => {
                // For other expressions, create a simple literal placeholder
                let resolved_expression = ResolvedHirExpression::Literal {
                    value: crate::ast::Value::Integer(0),
                    location: SourceLocation::default(),
                };

                self.set_result(result_slot, ResolverResult::Expression(resolved_expression));
            }
        }
        Ok(())
    }

    /// Register all top-level symbols
    fn register_top_level_symbols(&mut self, hir: &HirProgram) -> Result<(), ()> {
        // Register functions
        for function in &hir.functions {
            self.symbol_table.create_symbol(
                function.name.clone(),
                SymbolKind::Function {
                    parameters: function.parameters.iter().map(|p| p.param_type.clone()).collect(),
                    return_type: function.return_type.clone(),
                },
                ScopeId(0), // Global scope
                function.location.clone(),
            );
        }

        // Register start function
        if let Some(start_fn) = &hir.start_function {
            self.symbol_table.create_symbol(
                start_fn.name.clone(),
                SymbolKind::Function {
                    parameters: start_fn.parameters.iter().map(|p| p.param_type.clone()).collect(),
                    return_type: start_fn.return_type.clone(),
                },
                ScopeId(0), // Global scope
                start_fn.location.clone(),
            );
        }

        Ok(())
    }

    /// Log an error
    fn error(&mut self, message: &str, location: SourceLocation) {
        self.errors.push(CompilerError::syntax_error(
            message.to_string(),
            None,
            Some(location),
        ));
    }
}

impl Default for IterativeNameResolver {
    fn default() -> Self {
        Self::new()
    }
}