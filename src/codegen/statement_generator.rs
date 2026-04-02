//! Statement generation for Clean Language WebAssembly compiler.

use wasm_encoder::{BlockType, Instruction, MemArg};
use crate::ast::{Expression, Statement, Type, Value, SourceLocation};
use crate::error::CompilerError;
use crate::types::WasmType;
use super::{CodeGenerator, LocalVarInfo};

impl CodeGenerator {
    /// Generate WASM instructions for a statement
    pub fn generate_statement(
        &mut self,
        statement: &Statement,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        tracing::trace!("DEBUG: generate_statement called with: {:?}", std::mem::discriminant(statement));
        match statement {
            Statement::VariableDecl { name, type_, initializer, .. } => {
                self.generate_variable_declaration(name, &Some(type_.clone()), initializer, instructions)
            },
            Statement::Assignment { target, value, .. } => {
                self.generate_assignment(target, value, instructions)
            },
            Statement::PropertyAssignment { object, property, value, .. } => {
                self.generate_property_assignment_stmt(object, property, value, instructions)
            },
            Statement::Print { expression, .. } => {
                self.generate_print_statement(expression, instructions)
            },
            Statement::PrintBlock { expressions, .. } => {
                self.generate_print_block_statement(expressions, instructions)
            },
            Statement::Return { value, .. } => {
                self.generate_return_statement(value, instructions)
            },
            Statement::If { condition, then_branch, else_branch, .. } => {
                self.generate_if_statement(condition, then_branch, else_branch, instructions)
            },
            // For loops and try-catch are handled by other statement types
            Statement::Test { name, body, .. } => {
                self.generate_test_statement(name, body, instructions)
            },
            Statement::TestsBlock { tests, .. } => {
                self.generate_tests_block_statement(tests, instructions)
            },
            Statement::Background { expression, .. } => {
                // For now, treat background as a regular expression statement
                let mut temp_instructions = Vec::new();
                let result_type = self.generate_expression(expression, &mut temp_instructions)?;
                instructions.extend(temp_instructions);
                // Drop the result if the expression produced a value (to keep stack balanced)
                if result_type != WasmType::Unit {
                    instructions.push(Instruction::Drop);
                }
                Ok(())
            },
            Statement::LaterAssignment { variable, expression, .. } => {
                self.generate_later_assignment_statement(variable, expression, instructions)
            },
            Statement::Error { message, .. } => {
                self.generate_error_statement(message, instructions)
            },
            Statement::RangeIterate { iterator, start, end, step, body, .. } => {
                self.generate_range_iterate_with_bounds(iterator, start, end, step.as_ref(), body, instructions)
            },
            Statement::Iterate { iterator, collection, body, .. } => {
                self.generate_iterate_statement(&iterator, collection, body, instructions)
            },
            Statement::Expression { expr, .. } => {
                tracing::trace!("DEBUG: Generating Statement::Expression");
                self.generate_expression_statement(expr, instructions)
            },
            Statement::TypeApplyBlock { type_, assignments, .. } => {
                // Handle type apply-blocks: integer: count = 0, maxSize = 100
                for assignment in assignments {
                    self.generate_variable_declaration(&assignment.name, &Some(type_.clone()), &assignment.initializer, instructions)?;
                }
                Ok(())
            },
            Statement::FunctionApplyBlock { function_name, expressions, .. } => {
                // Handle function apply-blocks: print: "Hello", "World"
                for expression in expressions {
                    // Special case for print functions - treat them as print statements
                    if function_name == "print" || function_name == "printl" {
                        self.generate_print_statement(expression, instructions)?;
                    } else {
                        // Generate a function call for each expression
                        let call_expr = Expression::Call(function_name.clone(), vec![expression.clone()]);
                        let result_type = self.generate_expression(&call_expr, instructions)?;
                        // Drop the result if the function call produced a value (to keep stack balanced)
                        if result_type != WasmType::Unit {
                            instructions.push(Instruction::Drop);
                        }
                    }
                }
                Ok(())
            },
            Statement::MethodApplyBlock { object_name, method_chain, expressions, .. } => {
                // Handle method apply-blocks: array.push: item1, item2, item3
                for expression in expressions {
                    if let Some(method_name) = method_chain.last() {
                        // Generate a method call for each expression
                        let object_expr = Expression::Variable(object_name.clone());
                        let method_expr = Expression::MethodCall {
                            object: Box::new(object_expr),
                            method: method_name.clone(),
                            arguments: vec![expression.clone()],
                            location: SourceLocation { line: 0, column: 0, file: "apply-block".to_string() },
                        };
                        let result_type = self.generate_expression(&method_expr, instructions)?;
                        // Drop the result if the method call produced a value (to keep stack balanced)
                        if result_type != WasmType::Unit {
                            instructions.push(Instruction::Drop);
                        }
                    }
                }
                Ok(())
            },
            Statement::ConstantApplyBlock { constants, .. } => {
                // Handle constant apply-blocks: constant: integer MAX_SIZE = 100
                for constant in constants {
                    // Generate as immutable variable declaration
                    self.generate_variable_declaration(&constant.name, &Some(constant.type_.clone()), &Some(constant.value.clone()), instructions)?;
                }
                Ok(())
            },
            _ => {
                Err(CompilerError::codegen_error(
                    format!("Unsupported statement type: {:?}", statement),
                    None,
                    None
                ))
            }
        }
    }

    fn generate_variable_declaration(
        &mut self,
        name: &str,
        var_type: &Option<Type>,
        initializer: &Option<Expression>,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        let _wasm_type = if let Some(init_expr) = initializer {
            // Generate the initializer expression
            let expr_type = self.generate_expression(init_expr, instructions)?;
            
            // Store the result in a new local variable
            let local_index = self.add_local(expr_type);
            instructions.push(Instruction::LocalSet(local_index));
            
            // Register the variable
            self.variable_map.insert(name.to_string(), super::LocalVarInfo {
                index: local_index,
                type_: expr_type.into(),
            });
            
            expr_type
        } else if let Some(ast_type) = var_type {
            // Convert AST type to WASM type
            let wasm_type = self.ast_type_to_wasm_type(ast_type)?;

            // Create local with default value
            // Special handling for list types - must allocate an empty list
            match ast_type {
                Type::List(_) => {
                    // Create an empty list with default capacity of 8
                    // Using capacity=0 causes issues when .add() is called without capacity checking
                    instructions.push(Instruction::I32Const(8)); // capacity = 8 for empty list (size will be 0)
                    if let Some(func_index) = self.get_function_index("list.allocate") {
                        instructions.push(Instruction::Call(func_index));
                    } else {
                        return Err(CompilerError::codegen_error(
                            "list.allocate function not found".to_string(),
                            None,
                            None
                        ));
                    }
                },
                _ => {
                    // For non-list types, use default values
                    match wasm_type {
                        WasmType::I32 => instructions.push(Instruction::I32Const(0)),
                        WasmType::F64 => instructions.push(Instruction::F64Const(0.0)),
                        _ => return Err(CompilerError::codegen_error(
                            format!("Unsupported type for uninitialized variable: {:?}", wasm_type),
                            None,
                            None
                        )),
                    }
                }
            }

            let local_index = self.add_local(wasm_type);
            instructions.push(Instruction::LocalSet(local_index));

            // Register the variable
            self.variable_map.insert(name.to_string(), super::LocalVarInfo {
                index: local_index,
                type_: wasm_type.into(),
            });

            wasm_type
        } else {
            return Err(CompilerError::codegen_error(
                "Variable declaration must have either type or initializer".to_string(),
                None,
                None
            ));
        };

        // Track the variable type for automatic conversions
        if let Some(ast_type) = var_type {
            self.variable_types.insert(name.to_string(), ast_type.clone());
        } else if let Some(init_expr) = initializer {
            // Infer type from initializer
            let inferred_type = self.infer_expression_type(init_expr)?;
            self.variable_types.insert(name.to_string(), inferred_type);
        }

        Ok(())
    }

    fn generate_assignment(
        &mut self,
        target: &str,
        value: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Generate the value expression
        let value_type = self.generate_expression(value, instructions)?;
        
        // Find the target variable
        if let Some(local_info) = self.find_local(target) {
            let local_wasm_type = match local_info.type_ {
                wasm_encoder::ValType::I32 => WasmType::I32,
                wasm_encoder::ValType::I64 => WasmType::I64,
                wasm_encoder::ValType::F32 => WasmType::F32,
                wasm_encoder::ValType::F64 => WasmType::F64,
                wasm_encoder::ValType::V128 => WasmType::V128,
                _ => WasmType::I32,
            };
            
            // Type compatibility check
            if !self.types_compatible(&value_type, &local_wasm_type) {
                return Err(CompilerError::codegen_error(
                    format!("Type mismatch in assignment to {}: expected {:?}, got {:?}",
                           target, local_wasm_type, value_type),
                    None,
                    None
                ));
            }
            
            // Convert if necessary
            if value_type != local_wasm_type {
                self.generate_conversion(&mut vec![], value_type, local_wasm_type, instructions)?;
            }
            
            instructions.push(Instruction::LocalSet(local_info.index));
        } else {
            return Err(CompilerError::codegen_error(
                format!("Undefined variable in assignment: {}", target),
                None,
                None
            ));
        }

        Ok(())
    }

    fn generate_property_assignment_stmt(
        &mut self,
        object: &str,
        property: &str,
        value: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Generate the value expression first
        let value_type = self.generate_expression(value, instructions)?;
        
        // Find the object variable
        if let Some(object_info) = self.find_local(object) {
            // Check if the object is a class instance
            if let Some(object_type) = self.variable_types.get(object) {
                match object_type {
                    Type::Object(class_name) => {
                        // Find the field in the class hierarchy
                        if let Some((field_type, field_offset)) = self.find_class_field_with_inheritance(class_name, property) {
                            // Load object reference
                            instructions.push(Instruction::LocalGet(object_info.index));
                            
                            // Convert value type if necessary
                            let target_wasm_type = self.ast_type_to_wasm_type(&field_type)?;
                            if value_type != target_wasm_type {
                                self.generate_conversion(&mut vec![], value_type, target_wasm_type, instructions)?;
                            }
                            
                            // Store field value at calculated offset
                            let byte_offset = field_offset * 4; // Assuming 4 bytes per field
                            match target_wasm_type {
                                WasmType::I32 => {
                                    instructions.push(Instruction::I32Store(MemArg {
                                        offset: byte_offset as u64,
                                        align: 2, // 4-byte alignment
                                        memory_index: 0,
                                    }));
                                },
                                WasmType::F64 => {
                                    instructions.push(Instruction::F64Store(MemArg {
                                        offset: byte_offset as u64,
                                        align: 3, // 8-byte alignment
                                        memory_index: 0,
                                    }));
                                },
                                _ => return Err(CompilerError::codegen_error(
                                    format!("Unsupported field type for assignment: {:?}", target_wasm_type),
                                    None,
                                    None
                                ))
                            }
                            
                            Ok(())
                        } else {
                            // Handle special properties like list.type for behavior modifiers
                            match (object_type, property) {
                                (Type::List(_), "type") => {
                                    // For list behavior assignment, we could store this as metadata
                                    // For now, just accept the assignment without error
                                    instructions.push(Instruction::Drop); // Drop the value
                                    Ok(())
                                },
                                _ => Err(CompilerError::codegen_error(
                                    format!("Property '{}' not found in class '{}'", property, class_name),
                                    None,
                                    None
                                ))
                            }
                        }
                    },
                    Type::List(_) => {
                        // Handle list property assignment (like type = "line")
                        if property == "type" {
                            // Store list behavior type as metadata - simplified implementation
                            instructions.push(Instruction::Drop); // Drop the value for now
                            Ok(())
                        } else {
                            Err(CompilerError::codegen_error(
                                format!("Property '{}' not supported on list type", property),
                                None,
                                None
                            ))
                        }
                    },
                    _ => Err(CompilerError::codegen_error(
                        format!("Property assignment not supported on type: {:?}", object_type),
                        None,
                        None
                    ))
                }
            } else {
                Err(CompilerError::codegen_error(
                    format!("Cannot determine type of object '{}'", object),
                    None,
                    None
                ))
            }
        } else {
            Err(CompilerError::codegen_error(
                format!("Object '{}' not found", object),
                None,
                None
            ))
        }
    }

    fn generate_expression_statement(
        &mut self,
        expr: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Special handling for list mutating methods like .add(), .remove(), etc.
        // These should update the variable in-place: `list.add(item)` becomes `list = list.add(item)`
        if let Expression::MethodCall { object, method, arguments, .. } = expr {
            if matches!(method.as_str(), "add" | "remove" | "insert" | "pop" | "push" | "clear" | "removeLast") {
                // Check if object is a simple variable (not a complex expression)
                if let Expression::Variable(var_name) = object.as_ref() {
                    // Check if this variable exists and is a list type
                    if let Some(_var_type) = self.variable_types.get(var_name) {
                        // This is a mutating method call on a list variable
                        // Generate: var = var.method(args)
                        // First, generate the method call expression
                        let result_type = self.generate_expression(expr, instructions)?;

                        // Then store the result back into the variable
                        if let Some(var_info) = self.variable_map.get(var_name).cloned() {
                            instructions.push(Instruction::LocalSet(var_info.index));
                            return Ok(());
                        }

                        // If we couldn't find the variable, fall through to normal handling
                        if result_type != WasmType::Unit {
                            instructions.push(Instruction::Drop);
                        }
                        return Ok(());
                    }
                }
            }
        }

        // Normal expression statement handling
        let result_type = self.generate_expression(expr, instructions)?;

        // Only drop if the expression actually produces a value
        // Use the computed result type to determine this reliably
        if result_type != WasmType::Unit {
            tracing::trace!("DEBUG: Expression statement returning {:?}, adding Drop", result_type);
            instructions.push(Instruction::Drop);
        } else {
            tracing::trace!("DEBUG: Expression statement returning Unit, no Drop needed");
        }

        Ok(())
    }

    fn generate_print_statement(
        &mut self,
        expr: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Generate the expression
        let expr_type = self.generate_expression(expr, instructions)?;
        
        // Determine appropriate print function based on expression type and context
        let print_function = match expr_type {
            WasmType::I32 => {
                // Check if this is a string result (e.g., from toString() method call)
                if self.is_string_expression(expr) {
                    "print_string"
                } else {
                    "print_integer"
                }
            },
            WasmType::F64 => "print_float", 
            _ => "print_string",
        };
        
        let func_index = self.get_function_index_or_error(print_function)?;
        instructions.push(Instruction::Call(func_index));
        
        Ok(())
    }

    fn generate_print_block_statement(
        &mut self,
        exprs: &[Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        for expr in exprs {
            self.generate_print_statement(expr, instructions)?;
        }
        Ok(())
    }

    fn generate_return_statement(
        &mut self,
        expr: &Option<Expression>,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        if let Some(return_expr) = expr {
            self.generate_expression(return_expr, instructions)?;
        }
        instructions.push(Instruction::Return);
        Ok(())
    }

    fn generate_if_statement(
        &mut self,
        condition: &Expression,
        then_branch: &[Statement],
        else_branch: &Option<Vec<Statement>>,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Generate condition
        self.generate_expression(condition, instructions)?;
        
        // Create if block
        instructions.push(Instruction::If(BlockType::Empty));
        
        // Generate then branch
        for stmt in then_branch {
            self.generate_statement(stmt, instructions)?;
        }
        
        // Generate else branch if present
        if let Some(else_stmts) = else_branch {
            instructions.push(Instruction::Else);
            for stmt in else_stmts {
                self.generate_statement(stmt, instructions)?;
            }
        }
        
        instructions.push(Instruction::End);
        Ok(())
    }

    fn generate_test_statement(
        &mut self,
        _name: &str,
        _body: &[Statement],
        _instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        Err(CompilerError::codegen_error(
            "Test statements not yet implemented".to_string(),
            None,
            None
        ))
    }

    fn generate_tests_block_statement(
        &mut self,
        _tests: &[crate::ast::TestCase],
        _instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        Err(CompilerError::codegen_error(
            "Tests block not yet implemented".to_string(),
            None,
            None
        ))
    }

    fn generate_background_statement(
        &mut self,
        statements: &[Statement],
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // In single-threaded WASM, background statements execute synchronously
        for stmt in statements {
            self.generate_statement(stmt, instructions)?;
        }
        Ok(())
    }

    fn generate_later_assignment_statement(
        &mut self,
        target: &str,
        value: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // In single-threaded WASM, later assignments execute synchronously
        // Generate the expression value
        let expr_type = self.generate_expression(value, instructions)?;

        // Find or create the target variable
        if let Some(local_info) = self.find_local(target) {
            instructions.push(Instruction::LocalSet(local_info.index));
        } else {
            // Create new local for this variable
            let local_index = self.add_local(expr_type);
            instructions.push(Instruction::LocalSet(local_index));
            self.variable_map.insert(target.to_string(), super::LocalVarInfo {
                index: local_index,
                type_: expr_type.into(),
            });
        }
        Ok(())
    }

    fn generate_error_statement(
        &mut self,
        message: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Generate trap instruction for error statements
        // Note: Message handling requires WASM exception handling - using unreachable for now
        let _ = message;
        instructions.push(Instruction::Unreachable);
        Ok(())
    }

    fn generate_range_iterate_with_bounds(
        &mut self,
        iterator: &str,
        start: &Expression,
        end: &Expression,
        step: Option<&Expression>,
        body: &[Statement],
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Allocate locals for counter, end value, and step
        let counter_index = self.add_local(WasmType::I32);
        let end_index = self.add_local(WasmType::I32);
        let step_index = self.add_local(WasmType::I32);

        // Register the iterator variable
        self.variable_map.insert(
            iterator.to_string(),
            super::LocalVarInfo {
                index: counter_index,
                type_: wasm_encoder::ValType::I32,
            },
        );

        // Initialize counter = start
        self.generate_expression(start, instructions)?;
        instructions.push(Instruction::LocalSet(counter_index));

        // Initialize end value
        self.generate_expression(end, instructions)?;
        instructions.push(Instruction::LocalSet(end_index));

        // Initialize step (default = 1)
        if let Some(step_expr) = step {
            self.generate_expression(step_expr, instructions)?;
        } else {
            instructions.push(Instruction::I32Const(1));
        }
        instructions.push(Instruction::LocalSet(step_index));

        // Block (exit target) > Loop (continue target)
        instructions.push(Instruction::Block(BlockType::Empty));
        instructions.push(Instruction::Loop(BlockType::Empty));

        // Check: counter < end (for positive step) — exit if false
        instructions.push(Instruction::LocalGet(counter_index));
        instructions.push(Instruction::LocalGet(end_index));
        instructions.push(Instruction::I32LtS);
        instructions.push(Instruction::I32Eqz);
        instructions.push(Instruction::BrIf(1)); // Exit if counter >= end

        // Generate body
        for stmt in body {
            self.generate_statement(stmt, instructions)?;
        }

        // counter += step
        instructions.push(Instruction::LocalGet(counter_index));
        instructions.push(Instruction::LocalGet(step_index));
        instructions.push(Instruction::I32Add);
        instructions.push(Instruction::LocalSet(counter_index));

        // Branch back to loop
        instructions.push(Instruction::Br(0));

        // End loop, end block
        instructions.push(Instruction::End);
        instructions.push(Instruction::End);

        // Clean up iterator variable
        self.variable_map.remove(iterator);
        Ok(())
    }

    fn generate_iterate_statement(
        &mut self,
        variable: &str,
        iterable: &Expression,
        body: &[Statement],
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Implement iteration over lists
        // Note: Matrices and strings use different iteration patterns in MIR codegen

        // Get the length of the list - call the length method
        // For lists, we need to call the list.length() method
        match iterable {
            Expression::Variable(list_var_name) => {
                // Get the list reference
                if let Some(local_info) = self.find_local(list_var_name) {
                    instructions.push(Instruction::LocalGet(local_info.index));
                } else {
                    return Err(CompilerError::codegen_error(
                        format!("Variable '{}' not found", list_var_name),
                        None,
                        None
                    ));
                }

                // Call the list's length method
                // Create a method call expression for list.length()
                let length_call = Expression::MethodCall {
                    object: Box::new(Expression::Variable(list_var_name.clone())),
                    method: "length".to_string(),
                    arguments: vec![],
                    location: SourceLocation::new(0, 0, "<generated>"),
                };

                // Generate the method call to get the list length
                self.generate_expression(&length_call, instructions)?;
                
                // Store length in a local variable
                let length_local = self.add_local(WasmType::I32);
                instructions.push(Instruction::LocalSet(length_local));
                
                // Initialize loop counter
                let counter_local = self.add_local(WasmType::I32);
                instructions.push(Instruction::I32Const(0));
                instructions.push(Instruction::LocalSet(counter_local));
                
                // Start loop
                let _loop_label = self.next_label();
                let _break_label = self.next_label();
                
                instructions.push(Instruction::Block(wasm_encoder::BlockType::Empty));
                instructions.push(Instruction::Loop(wasm_encoder::BlockType::Empty));
                
                // Check loop condition: counter < length
                instructions.push(Instruction::LocalGet(counter_local));
                instructions.push(Instruction::LocalGet(length_local));
                instructions.push(Instruction::I32LtS);
                instructions.push(Instruction::I32Eqz);
                instructions.push(Instruction::BrIf(1)); // Break if counter >= length
                
                // Get current element: list[counter]
                // Load the list reference again
                if let Some(local_info) = self.find_local(list_var_name) {
                    instructions.push(Instruction::LocalGet(local_info.index));
                } else {
                    return Err(CompilerError::codegen_error(
                        format!("Variable '{}' not found in element access", list_var_name),
                        None,
                        None
                    ));
                }
                
                // Load the counter (index)
                instructions.push(Instruction::LocalGet(counter_local));
                
                // Call list_get function to get the element at current index
                let list_get_func_index = self.get_function_index_or_error("list.get")?;
                instructions.push(Instruction::Call(list_get_func_index));
                
                // Store the element in the loop variable
                let variable_local = self.add_local(WasmType::I32);
                instructions.push(Instruction::LocalSet(variable_local));
                
                // Add loop variable to variable map for the body
                let var_info = LocalVarInfo {
                    index: variable_local,
                    type_: wasm_encoder::ValType::I32,
                };
                self.variable_map.insert(variable.to_string(), var_info);
                
                // Generate loop body
                for stmt in body {
                    self.generate_statement(stmt, instructions)?;
                }
                
                // Increment counter
                instructions.push(Instruction::LocalGet(counter_local));
                instructions.push(Instruction::I32Const(1));
                instructions.push(Instruction::I32Add);
                instructions.push(Instruction::LocalSet(counter_local));
                
                // Continue loop
                instructions.push(Instruction::Br(0));
                
                // End loop and block
                instructions.push(Instruction::End);
                instructions.push(Instruction::End);
                
                Ok(())
            },
            _ => {
                Err(CompilerError::codegen_error(
                    "Complex iterable expressions not yet supported".to_string(),
                    None,
                    None
                ))
            }
        }
    }

    // Helper method to check if an expression produces a string result
    fn is_string_expression(&self, expr: &Expression) -> bool {
        match expr {
            // String literals are obviously strings
            Expression::Literal(Value::String(_)) => true,
            
            // toString() method calls always return strings
            Expression::MethodCall { method, .. } if method == "toString" => true,
            
            // String variables
            Expression::Variable(var_name) => {
                if let Some(var_type) = self.variable_types.get(var_name) {
                    matches!(var_type, Type::String)
                } else {
                    false
                }
            },
            
            // String operations (concatenation, etc.) - would need to check operators
            // Binary operations that result in strings
            Expression::Binary(left, op, right) => {
                match op {
                    // String concatenation
                    crate::ast::BinaryOperator::Add => {
                        self.is_string_expression(left) || self.is_string_expression(right)
                    },
                    _ => false
                }
            },
            
            // String interpolation always returns strings
            Expression::StringInterpolation(_) => true,
            
            // Method calls on string-returning functions
            Expression::Call(func_name, _) => {
                // Check if this is a function that returns a string
                // For now, we'll be conservative and return false
                // This could be improved with better type tracking
                func_name.ends_with("_to_string") || func_name.ends_with(".toString")
            },
            
            _ => false
        }
    }

    // Helper method to infer type from expression
    fn infer_expression_type(&self, expr: &Expression) -> Result<Type, CompilerError> {
        match expr {
            Expression::Literal(Value::Integer(_)) => Ok(Type::Integer),
            Expression::Literal(Value::Number(_)) => Ok(Type::Number),
            Expression::Literal(Value::String(_)) => Ok(Type::String),
            Expression::Literal(Value::Boolean(_)) => Ok(Type::Boolean),
            Expression::Literal(Value::List(_)) => Ok(Type::List(Box::new(Type::Integer))), // Default to integer list
            Expression::Variable(name) => {
                if let Some(ast_type) = self.variable_types.get(name) {
                    Ok(ast_type.clone())
                } else {
                    Err(CompilerError::codegen_error(
                        format!("Cannot infer type of variable: {}", name),
                        None,
                        None
                    ))
                }
            },
            _ => Err(CompilerError::codegen_error(
                "Cannot infer type of complex expression".to_string(),
                None,
                None
            )),
        }
    }
}