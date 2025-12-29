//! Expression generation for Clean Language WebAssembly compiler.

use wasm_encoder::Instruction;
use crate::ast::{BinaryOperator, Expression, StringPart, UnaryOperator, Value};
use crate::error::CompilerError;
use crate::types::WasmType;
use super::CodeGenerator;

impl CodeGenerator {
    /// Generate WASM instructions for an expression
    pub fn generate_expression(
        &mut self, 
        expr: &Expression, 
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match expr {
            Expression::Literal(value) => self.generate_value_expression(value, instructions),
            Expression::Variable(name) => self.generate_variable_expression(name, instructions),
            Expression::Binary(left, operator, right) => {
                self.generate_binary_expression(left, operator, right, instructions)
            },
            Expression::Unary(operator, operand) => {
                self.generate_unary_expression(operator, operand, instructions)
            },
            Expression::Call(name, arguments) => {
                self.generate_function_call_expression(name, arguments, instructions)
            },
            Expression::MethodCall { object, method, arguments, .. } => {
                self.generate_method_call_expression(object, method, arguments, instructions)
            },
            Expression::PropertyAccess { object, property, .. } => {
                self.generate_property_access_expression(object, property, instructions)
            },
            Expression::ListAccess(array, index) => {
                self.generate_array_access_expression(array, index, instructions)
            },
            Expression::ListAssignment { list, index, value, .. } => {
                self.generate_list_assignment_expression(list, index, value, instructions)
            },
            Expression::OnError { expression, fallback, .. } => {
                self.generate_onerror_expression(expression, fallback, instructions)
            },
            Expression::StaticMethodCall { class_name, method, arguments, .. } => {
                self.generate_static_method_call_expression(class_name, method, arguments, instructions)
            },
            Expression::StringInterpolation(parts) => {
                self.generate_string_interpolation_expression(parts, instructions)
            },
            Expression::NamespaceCall { namespace, function, arguments, .. } => {
                // Handle namespace calls like conditional.integer, logical.and, etc.
                // For namespace calls, the namespace is the "class" and function is the "method"
                self.generate_builtin_static_method_call(namespace, function, arguments, instructions)
            },
            Expression::BaseCall { arguments, .. } => {
                // Handle base constructor calls in inheritance
                self.generate_base_call_expression(arguments, instructions)
            },
            Expression::Conditional { condition, then_expr, else_expr, .. } => {
                self.generate_conditional_expression(condition, then_expr, else_expr, instructions)
            },
            _ => {
                Err(CompilerError::codegen_error(
                    format!("Unsupported expression type: {:?}", expr),
                    None,
                    None
                ))
            }
        }
    }

    fn generate_value_expression(
        &mut self,
        value: &Value,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match value {
            Value::Integer(n) => {
                instructions.push(Instruction::I32Const(*n as i32));
                Ok(WasmType::I32)
            },
            Value::Number(f) => {
                instructions.push(Instruction::F64Const(*f));
                Ok(WasmType::F64)
            },
            Value::String(s) => {
                let string_ptr = self.allocate_string(s)?;
                instructions.push(Instruction::I32Const(string_ptr as i32));
                Ok(WasmType::I32)
            },
            Value::Boolean(b) => {
                instructions.push(Instruction::I32Const(if *b { 1 } else { 0 }));
                Ok(WasmType::I32)
            },
            Value::List(elements) => {
                // Convert Values to Expressions for list literal processing
                let expressions: Vec<Expression> = elements.iter()
                    .map(|val| Expression::Literal(val.clone()))
                    .collect();
                self.generate_list_literal_expression(&expressions, instructions)
            },
            Value::Matrix(rows) => {
                self.generate_matrix_literal_expression(rows, instructions)
            },
            Value::Void => {
                Ok(WasmType::I32) // Void represented as 0
            },
            _ => {
                Err(CompilerError::codegen_error(
                    format!("Unsupported value type: {:?}", value),
                    None,
                    None
                ))
            }
        }
    }

    fn generate_variable_expression(
        &mut self,
        name: &str,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        if let Some(local_info) = self.find_local(name) {
            instructions.push(Instruction::LocalGet(local_info.index));
            Ok(match local_info.type_ {
                wasm_encoder::ValType::I32 => WasmType::I32,
                wasm_encoder::ValType::I64 => WasmType::I64,
                wasm_encoder::ValType::F32 => WasmType::F32,
                wasm_encoder::ValType::F64 => WasmType::F64,
                wasm_encoder::ValType::V128 => WasmType::V128,
                other => {
                    return Err(CompilerError::codegen_error(
                        format!("Unsupported local variable type for '{}': {:?}", name, other),
                        Some("This variable has an unsupported WASM type".to_string()),
                        None
                    ));
                }
            })
        } else if self.is_namespace_variable(name) {
            // Handle namespace variables like compare, conditional, logical
            // These represent namespace objects and don't have actual variable storage
            // Return a special object pointer that represents the namespace
            instructions.push(Instruction::I32Const(0)); // Placeholder value for namespace object
            Ok(WasmType::I32)
        } else if let Some(class_name) = &self.current_class_context {
            // Try to find field in current class or inherited classes
            if let Some(field_info) = self.find_class_field_with_inheritance(class_name, name) {
                // Load 'this' pointer (always the first parameter in method calls)
                instructions.push(Instruction::LocalGet(0));
                
                // Load field value from object memory
                let field_offset = field_info.1 * 4; // Assuming 4 bytes per field
                instructions.push(Instruction::I32Const(field_offset as i32));
                instructions.push(Instruction::I32Add);
                instructions.push(Instruction::I32Load(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                
                Ok(WasmType::I32) // Field access returns i32 (type info available in field_info.0)
            } else {
                Err(CompilerError::codegen_error(
                    format!("Undefined variable: {}", name),
                    None,
                    None
                ))
            }
        } else {
            Err(CompilerError::codegen_error(
                format!("Undefined variable: {}", name),
                None,
                None
            ))
        }
    }

    fn generate_binary_expression(
        &mut self,
        left: &Expression,
        operator: &BinaryOperator,
        right: &Expression,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // BOOK: null-coalescing - Handle default operator specially (needs stack rearrangement)
        if let BinaryOperator::Default = operator {
            return self.generate_default_operation(left, right, instructions);
        }

        let left_type = self.generate_expression(left, instructions)?;
        let right_type = self.generate_expression(right, instructions)?;

        match operator {
            BinaryOperator::Add => self.generate_add_operation(left_type, right_type, instructions),
            BinaryOperator::Subtract => self.generate_subtract_operation(left_type, right_type, instructions),
            BinaryOperator::Multiply => self.generate_multiply_operation(left_type, right_type, instructions),
            BinaryOperator::Divide => self.generate_divide_operation(left_type, right_type, instructions),
            BinaryOperator::Equal => self.generate_equal_operation(left_type, right_type, instructions),
            BinaryOperator::NotEqual => self.generate_not_equal_operation(left_type, right_type, instructions),
            BinaryOperator::Less => self.generate_less_than_operation(left_type, right_type, instructions),
            BinaryOperator::LessEqual => self.generate_less_equal_operation(left_type, right_type, instructions),
            BinaryOperator::Greater => self.generate_greater_than_operation(left_type, right_type, instructions),
            BinaryOperator::GreaterEqual => self.generate_greater_equal_operation(left_type, right_type, instructions),
            BinaryOperator::And => self.generate_logical_and_operation(left_type, right_type, instructions),
            BinaryOperator::Or => self.generate_logical_or_operation(left_type, right_type, instructions),
            BinaryOperator::Modulo => self.generate_modulo_operation(left_type, right_type, instructions),
            BinaryOperator::Power => self.generate_power_operation(left_type, right_type, instructions),
            BinaryOperator::Is => self.generate_is_operation(left_type, right_type, instructions),
            BinaryOperator::Not => self.generate_not_operation(left_type, right_type, instructions),
            BinaryOperator::Default => unreachable!(), // Handled above
        }
    }

    fn generate_unary_expression(
        &mut self,
        operator: &UnaryOperator,
        operand: &Expression,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        let operand_type = self.generate_expression(operand, instructions)?;

        match operator {
            UnaryOperator::Negate => {
                match operand_type {
                    WasmType::I32 => {
                        instructions.push(Instruction::I32Const(-1));
                        instructions.push(Instruction::I32Mul);
                        Ok(WasmType::I32)
                    },
                    WasmType::F64 => {
                        instructions.push(Instruction::F64Neg);
                        Ok(WasmType::F64)
                    },
                    _ => Err(CompilerError::codegen_error(
                        "Cannot negate non-numeric type".to_string(),
                        None,
                        None
                    ))
                }
            },
            UnaryOperator::Not => {
                instructions.push(Instruction::I32Eqz);
                Ok(WasmType::I32)
            },
        }
    }

    fn generate_add_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32Add);
                Ok(WasmType::I32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Add);
                Ok(WasmType::F64)
            },
            (WasmType::I32, WasmType::F64) => {
                // Convert left to F64 and add
                instructions.insert(instructions.len() - 2, Instruction::F64ConvertI32S);
                instructions.push(Instruction::F64Add);
                Ok(WasmType::F64)
            },
            (WasmType::F64, WasmType::I32) => {
                // Convert right to F64 and add
                instructions.push(Instruction::F64ConvertI32S);
                instructions.push(Instruction::F64Add);
                Ok(WasmType::F64)
            },
            _ => Err(CompilerError::codegen_error(
                "Unsupported types for addition".to_string(),
                None,
                None
            ))
        }
    }

    // Additional arithmetic operations...
    fn generate_subtract_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32Sub);
                Ok(WasmType::I32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Sub);
                Ok(WasmType::F64)
            },
            _ => Err(CompilerError::codegen_error(
                "Unsupported types for subtraction".to_string(),
                None,
                None
            ))
        }
    }

    fn generate_multiply_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32Mul);
                Ok(WasmType::I32)
            },
            (WasmType::I64, WasmType::I64) => {
                instructions.push(Instruction::I64Mul);
                Ok(WasmType::I64)
            },
            (WasmType::F32, WasmType::F32) => {
                instructions.push(Instruction::F32Mul);
                Ok(WasmType::F32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Mul);
                Ok(WasmType::F64)
            },
            // Mixed type conversions would need more complex stack manipulation
            // For now, let's keep it simple and only support same-type operations
            _ => Err(CompilerError::codegen_error(
                format!("Unsupported types for multiplication: {:?} * {:?}", left_type, right_type),
                None,
                None
            ))
        }
    }

    fn generate_divide_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32DivS);
                Ok(WasmType::I32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Div);
                Ok(WasmType::F64)
            },
            _ => Err(CompilerError::codegen_error(
                "Unsupported types for division".to_string(),
                None,
                None
            ))
        }
    }

    // Comparison operations...
    fn generate_equal_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32Eq);
                Ok(WasmType::I32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Eq);
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                "Unsupported types for equality comparison".to_string(),
                None,
                None
            ))
        }
    }

    fn generate_not_equal_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        self.generate_equal_operation(left_type, right_type, instructions)?;
        instructions.push(Instruction::I32Eqz);
        Ok(WasmType::I32)
    }

    fn generate_less_than_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32LtS);
                Ok(WasmType::I32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Lt);
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                "Unsupported types for less than comparison".to_string(),
                None,
                None
            ))
        }
    }

    fn generate_less_equal_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32LeS);
                Ok(WasmType::I32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Le);
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                "Unsupported types for less than or equal comparison".to_string(),
                None,
                None
            ))
        }
    }

    fn generate_greater_than_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32GtS);
                Ok(WasmType::I32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Gt);
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                "Unsupported types for greater than comparison".to_string(),
                None,
                None
            ))
        }
    }

    fn generate_greater_equal_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32GeS);
                Ok(WasmType::I32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Ge);
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                "Unsupported types for greater than or equal comparison".to_string(),
                None,
                None
            ))
        }
    }

    fn generate_logical_and_operation(
        &mut self,
        _left_type: WasmType,
        _right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        instructions.push(Instruction::I32And);
        Ok(WasmType::I32)
    }

    fn generate_logical_or_operation(
        &mut self,
        _left_type: WasmType,
        _right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        instructions.push(Instruction::I32Or);
        Ok(WasmType::I32)
    }

    // BOOK: null-coalescing - Generate code for default operator (null coalescing)
    // Semantics: `a default b` returns a if a is not null (not 0), otherwise returns b
    fn generate_default_operation(
        &mut self,
        left: &Expression,
        right: &Expression,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Generate left expression and store in temp local
        let left_type = self.generate_expression(left, instructions)?;
        let left_local = self.add_local(left_type);
        instructions.push(Instruction::LocalSet(left_local));

        // Generate right expression (fallback value)
        let right_type = self.generate_expression(right, instructions)?;

        // Now stack has: [right_value]
        // We need to push: left_value, condition, then select
        // select(val1, val2, cond) returns val1 if cond != 0, else val2
        // Stack order for select: [val2, val1, cond]
        // We want: return left if (left != 0), else return right
        // So: val1 = left, val2 = right, cond = (left != 0)

        // Stack currently: [right_value]
        // Push left_value (this is val1)
        instructions.push(Instruction::LocalGet(left_local));

        // Push condition (left != 0)
        instructions.push(Instruction::LocalGet(left_local));
        instructions.push(Instruction::I32Const(0));
        instructions.push(Instruction::I32Ne);

        // Stack now: [right_value, left_value, condition]
        // select will return left_value if condition != 0, else right_value
        instructions.push(Instruction::Select);

        // Return type is the type of the values (they should match)
        // For now, we return the left type; type checking should ensure compatibility
        Ok(left_type)
    }

    fn generate_modulo_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32RemS);
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                "Modulo operation only supported for integers".to_string(),
                None,
                None
            ))
        }
    }

    // Method and function call implementations...
    fn generate_function_call_expression(
        &mut self,
        name: &str,
        arguments: &[Expression],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Check if this function has default parameters and handle them
        let mut final_arguments = arguments.to_vec();
        
        if let Some(func_def) = self.function_definitions.get(name).cloned() {
            // If we have fewer arguments than parameters, fill in with default values
            while final_arguments.len() < func_def.parameters.len() {
                let param_index = final_arguments.len();
                if let Some(ref default_value) = func_def.parameters[param_index].default_value {
                    final_arguments.push(default_value.clone());
                } else {
                    // Missing required parameter - this should be caught by semantic analysis
                    return Err(CompilerError::codegen_error(
                        format!("Missing required parameter {} for function {}", param_index, name),
                        None,
                        None
                    ));
                }
            }
        }

        // Generate arguments (including defaults)
        for arg in &final_arguments {
            self.generate_expression(arg, instructions)?;
        }

        // Try to get function index, fallback to implicit method call if in class context
        let func_index = match self.get_function_index(name) {
            Some(index) => {
                index
            },
            None => {
                // If function not found and we're in a class context, try as implicit method call
                if let Some(class_name) = &self.current_class_context {
                    let class_name_clone = class_name.clone();
                    return self.generate_implicit_method_call(&class_name_clone, name, arguments, instructions);
                } else {
                    return Err(CompilerError::codegen_error(
                        format!("Function not found: {}", name),
                        None,
                        None
                    ));
                }
            }
        };
        instructions.push(Instruction::Call(func_index));
        
        // Return the function's return type
        self.get_function_return_type(func_index)
    }

    fn generate_method_call_expression(
        &mut self,
        object: &Expression,
        method_name: &str,
        arguments: &[Expression],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Check if this is a namespace method call like compare.integer.greaterThan()
        if let Expression::PropertyAccess { object: namespace_obj, property: namespace_prop, .. } = object {
            if let Expression::Variable(namespace_name) = namespace_obj.as_ref() {
                if self.is_namespace_variable(namespace_name) {
                    // This is a namespace method call: namespace.category.method()
                    let full_namespace = format!("{}.{}", namespace_name, namespace_prop);
                    return self.generate_builtin_static_method_call(&full_namespace, method_name, arguments, instructions);
                }
            }
        }

        // Check if this is a direct namespace method call like conditional.integer()
        if let Expression::Variable(var_name) = object {
            if self.is_namespace_variable(var_name) {
                // This is a namespace method call: namespace.method()
                return self.generate_builtin_static_method_call(var_name, method_name, arguments, instructions);
            }
        }

        // Generate the object expression first
        let object_type = self.generate_expression(object, instructions)?;

        // Handle method calls based on method name and object type
        match method_name {
            "toString" => {
                // Generate arguments if any (should be none for toString)
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // Call appropriate toString function based on object type
                match object_type {
                    WasmType::I32 => {
                        // Check if it's actually a boolean by examining the object
                        if let Expression::Variable(var_name) = object {
                            if let Some(var_type) = self.variable_types.get(var_name) {
                                match var_type {
                                    crate::ast::Type::Boolean => {
                                        let func_index = self.get_function_index_or_error("bool_to_string")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    },
                                    crate::ast::Type::String => {
                                        // String already has correct representation, no conversion needed
                                        return Ok(WasmType::I32);
                                    },
                                    _ => {
                                        // Default to integer conversion
                                        let func_index = self.get_function_index_or_error("int_to_string")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    }
                                }
                            }
                        }
                        
                        // Default I32 treatment as integer
                        let func_index = self.get_function_index_or_error("int_to_string")?;
                        instructions.push(Instruction::Call(func_index));
                        Ok(WasmType::I32)
                    },
                    WasmType::F64 => {
                        let func_index = self.get_function_index_or_error("float_to_string")?;
                        instructions.push(Instruction::Call(func_index));
                        Ok(WasmType::I32)
                    },
                    _ => {
                        // For other types, assume they need string conversion
                        let func_index = self.get_function_index_or_error("int_to_string")?;
                        instructions.push(Instruction::Call(func_index));
                        Ok(WasmType::I32)
                    }
                }
            },
            "length" => {
                // Generate arguments if any (should be none for length)
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // Call appropriate length function based on object type
                match object_type {
                    WasmType::I32 => {
                        // Check if it's a string or treat as number digits
                        if let Expression::Variable(var_name) = object {
                            if let Some(var_type) = self.variable_types.get(var_name) {
                                match var_type {
                                    crate::ast::Type::String => {
                                        let func_index = self.get_function_index_or_error("string_length")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    },
                                    _ => {}
                                }
                            }
                        }
                        
                        // For integers, convert to string first then get length (digit count)
                        let str_func_index = self.get_function_index_or_error("int_to_string")?;
                        instructions.push(Instruction::Call(str_func_index));
                        let len_func_index = self.get_function_index_or_error("string_length")?;
                        instructions.push(Instruction::Call(len_func_index));
                        Ok(WasmType::I32)
                    },
                    WasmType::F64 => {
                        // For numbers, convert to string first then get length
                        let str_func_index = self.get_function_index_or_error("float_to_string")?;
                        instructions.push(Instruction::Call(str_func_index));
                        let len_func_index = self.get_function_index_or_error("string_length")?;
                        instructions.push(Instruction::Call(len_func_index));
                        Ok(WasmType::I32)
                    },
                    _ => {
                        // Check if this is a list type
                        if let Expression::Variable(var_name) = object {
                            if let Some(var_type) = self.variable_types.get(var_name) {
                                match var_type {
                                    crate::ast::Type::List(_) => {
                                        // Call list_length function
                                        let func_index = self.get_function_index_or_error("list_length")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    },
                                    _ => {}
                                }
                            }
                        }
                        
                        Err(CompilerError::codegen_error(
                            format!("length() method not supported for type: {:?}", object_type),
                            None,
                            None
                        ))
                    }
                }
            },
            "toInteger" => {
                // Generate arguments if any (should be none for toInteger)
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // Call appropriate toInteger function based on object type
                match object_type {
                    WasmType::I32 => {
                        // Check the actual variable type
                        if let Expression::Variable(var_name) = object {
                            if let Some(var_type) = self.variable_types.get(var_name) {
                                match var_type {
                                    crate::ast::Type::Boolean => {
                                        let func_index = self.get_function_index_or_error("boolean.toInteger")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    },
                                    crate::ast::Type::String => {
                                        let func_index = self.get_function_index_or_error("string.toInteger")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    },
                                    crate::ast::Type::Integer => {
                                        // Identity conversion - integer is already integer
                                        return Ok(WasmType::I32);
                                    },
                                    _ => {}
                                }
                            }
                        }
                        // Default: assume it's already an integer
                        Ok(WasmType::I32)
                    },
                    WasmType::F64 => {
                        let func_index = self.get_function_index_or_error("number.toInteger")?;
                        instructions.push(Instruction::Call(func_index));
                        Ok(WasmType::I32)
                    },
                    _ => {
                        Err(CompilerError::codegen_error(
                            format!("toInteger() method not supported for type: {:?}", object_type),
                            None,
                            None
                        ))
                    }
                }
            },
            "toNumber" => {
                // Generate arguments if any (should be none for toNumber)  
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // Call appropriate toNumber function based on object type
                match object_type {
                    WasmType::I32 => {
                        // Check the actual variable type
                        if let Expression::Variable(var_name) = object {
                            if let Some(var_type) = self.variable_types.get(var_name) {
                                match var_type {
                                    crate::ast::Type::Boolean => {
                                        let func_index = self.get_function_index_or_error("boolean.toNumber")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::F64);
                                    },
                                    crate::ast::Type::String => {
                                        let func_index = self.get_function_index_or_error("string.toNumber")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::F64);
                                    },
                                    crate::ast::Type::Integer => {
                                        let func_index = self.get_function_index_or_error("integer.toNumber")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::F64);
                                    },
                                    _ => {}
                                }
                            }
                        }
                        // Default: assume it's integer and convert to number
                        let func_index = self.get_function_index_or_error("integer.toNumber")?;
                        instructions.push(Instruction::Call(func_index));
                        Ok(WasmType::F64)
                    },
                    WasmType::F64 => {
                        // Number is already a number - identity conversion
                        Ok(WasmType::F64)
                    },
                    _ => {
                        Err(CompilerError::codegen_error(
                            format!("toNumber() method not supported for type: {:?}", object_type),
                            None,
                            None
                        ))
                    }
                }
            },
            "toBoolean" => {
                // Generate arguments if any (should be none for toBoolean)
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // Call appropriate toBoolean function based on object type
                match object_type {
                    WasmType::I32 => {
                        // Check the actual variable type
                        if let Expression::Variable(var_name) = object {
                            if let Some(var_type) = self.variable_types.get(var_name) {
                                match var_type {
                                    crate::ast::Type::Boolean => {
                                        // Boolean is already boolean - identity conversion
                                        return Ok(WasmType::I32);
                                    },
                                    crate::ast::Type::String => {
                                        let func_index = self.get_function_index_or_error("string.toBoolean")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    },
                                    crate::ast::Type::Integer => {
                                        let func_index = self.get_function_index_or_error("integer.toBoolean")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    },
                                    _ => {}
                                }
                            }
                        }
                        // Default: assume it's integer and convert to boolean
                        let func_index = self.get_function_index_or_error("integer.toBoolean")?;
                        instructions.push(Instruction::Call(func_index));
                        Ok(WasmType::I32)
                    },
                    WasmType::F64 => {
                        let func_index = self.get_function_index_or_error("number.toBoolean")?;
                        instructions.push(Instruction::Call(func_index));
                        Ok(WasmType::I32)
                    },
                    _ => {
                        Err(CompilerError::codegen_error(
                            format!("toBoolean() method not supported for type: {:?}", object_type),
                            None,
                            None
                        ))
                    }
                }
            },
            "toUpperCase" => {
                // Generate arguments if any (should be none for toUpperCase)
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // toUpperCase only works on strings
                match object_type {
                    WasmType::I32 => {
                        // Check if it's actually a string
                        if let Expression::Variable(var_name) = object {
                            if let Some(var_type) = self.variable_types.get(var_name) {
                                match var_type {
                                    crate::ast::Type::String => {
                                        let func_index = self.get_function_index_or_error("string.toUpperCase")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    },
                                    _ => {}
                                }
                            }
                        }
                        Err(CompilerError::codegen_error(
                            "toUpperCase() method is only supported for strings".to_string(),
                            None,
                            None
                        ))
                    },
                    _ => {
                        Err(CompilerError::codegen_error(
                            "toUpperCase() method is only supported for strings".to_string(),
                            None,
                            None
                        ))
                    }
                }
            },
            "toLowerCase" => {
                // Generate arguments if any (should be none for toLowerCase)
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // toLowerCase only works on strings
                match object_type {
                    WasmType::I32 => {
                        // Check if it's actually a string
                        if let Expression::Variable(var_name) = object {
                            if let Some(var_type) = self.variable_types.get(var_name) {
                                match var_type {
                                    crate::ast::Type::String => {
                                        let func_index = self.get_function_index_or_error("string.toLowerCase")?;
                                        instructions.push(Instruction::Call(func_index));
                                        return Ok(WasmType::I32);
                                    },
                                    _ => {}
                                }
                            }
                        }
                        Err(CompilerError::codegen_error(
                            "toLowerCase() method is only supported for strings".to_string(),
                            None,
                            None
                        ))
                    },
                    _ => {
                        Err(CompilerError::codegen_error(
                            "toLowerCase() method is only supported for strings".to_string(),
                            None,
                            None
                        ))
                    }
                }
            },
            _ => {
                // Try to handle as a custom class method call
                self.generate_custom_method_call(object, method_name, arguments, instructions)
            }
        }
    }

    fn generate_custom_method_call(
        &mut self,
        object: &Expression,
        method_name: &str,
        arguments: &[Expression],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Check if this is a method call on a primitive type that should be converted to a static call
        if let Expression::Variable(var_name) = object {
            if let Some(var_type) = self.variable_types.get(var_name) {
                match var_type {
                    crate::ast::Type::List(_) => {
                        // Convert list.method() to list.method(list, args)
                        return self.generate_static_method_call("list", method_name, object, arguments, instructions);
                    },
                    crate::ast::Type::String => {
                        // Convert string.method() to string.method(string, args)
                        return self.generate_static_method_call("string", method_name, object, arguments, instructions);
                    },
                    _ => {} // Continue with class method handling
                }
            }
        }
        
        // First, determine the class of the object
        let object_class = self.get_object_class(object)?;
        
        // Look for the method in the class hierarchy (starting with the object's class, then parent classes)
        let method_function_name = self.find_method_in_hierarchy(&object_class, method_name)?;
        
        // Generate the arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }
        
        // Generate the object reference (this pointer) - need to pass the object as first argument for instance methods
        self.generate_expression(object, instructions)?;
        
        // Get function index and call the method
        let func_index = self.get_function_index_or_error(&method_function_name)?;
        instructions.push(Instruction::Call(func_index));
        
        // Return the method's return type (for now assume void, this should be looked up from method signature)
        Ok(WasmType::I32) // Method return type lookup uses i32 as default
    }

    fn get_object_class(&self, object: &Expression) -> Result<String, CompilerError> {
        match object {
            Expression::Variable(var_name) => {
                // Look up variable type in variable_types table
                if let Some(var_type) = self.variable_types.get(var_name) {
                    match var_type {
                        crate::ast::Type::Class { name, .. } => Ok(name.clone()),
                        crate::ast::Type::Object(name) => Ok(name.clone()),
                        _ => Err(CompilerError::codegen_error(
                            format!("Variable '{}' is not a class instance", var_name),
                            None,
                            None
                        ))
                    }
                } else {
                    Err(CompilerError::codegen_error(
                        format!("Variable '{}' not found", var_name),
                        None,
                        None
                    ))
                }
            }
            _ => Err(CompilerError::codegen_error(
                "Complex object expressions not yet supported for method calls".to_string(),
                None,
                None
            ))
        }
    }

    fn find_method_in_hierarchy(&self, class_name: &str, method_name: &str) -> Result<String, CompilerError> {
        let mut current_class = class_name;
        
        loop {
            // Try to find the method in current class
            let method_function_name = format!("{}_{}", current_class, method_name);
            if self.get_function_index(&method_function_name).is_some() {
                return Ok(method_function_name);
            }
            
            // Method not found in current class, check parent class
            if let Some(class_info) = self.class_table.get(current_class) {
                if let Some(parent_name) = &class_info.base_class {
                    current_class = parent_name;
                } else {
                    // No parent class, method not found
                    break;
                }
            } else {
                // Class not found, method not found
                break;
            }
        }
        
        Err(CompilerError::codegen_error(
            format!("Method '{}' not found in class '{}' or its parent classes", method_name, class_name),
            None,
            None
        ))
    }

    fn generate_implicit_method_call(
        &mut self,
        class_name: &str,
        method_name: &str,
        arguments: &[Expression],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Look for the method in the class hierarchy (starting with the current class, then parent classes)
        let method_function_name = self.find_method_in_hierarchy(class_name, method_name)?;
        
        // Generate the arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }
        
        // For implicit method calls, we need to generate a reference to 'this'
        // In WASM, 'this' is typically the first parameter (local 0)
        instructions.push(Instruction::LocalGet(0));
        
        // Get function index and call the method
        let func_index = self.get_function_index_or_error(&method_function_name)?;
        instructions.push(Instruction::Call(func_index));
        
        // Return the method's return type (for now assume I32, should be looked up from method signature)
        Ok(WasmType::I32) // Method return type lookup uses i32 as default
    }

    fn generate_property_access_expression(
        &mut self,
        object: &Expression,
        property: &str,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Check if this is a namespace property access like compare.integer, conditional.string, etc.
        if let Expression::Variable(namespace_name) = object {
            if self.is_namespace_variable(namespace_name) {
                // This is a namespace property access - return a function pointer/reference
                // We don't need to generate any actual code here, just return a valid type
                // The actual function call will be handled by the method call expression
                Ok(WasmType::I32) // Function reference
            } else if self.is_any_type_variable(namespace_name) {
                // Property access on Any type (JSON object)
                // Generate: __json_get_field(any_ptr, key_ptr, key_len)
                self.generate_any_property_access(object, property, instructions)
            } else {
                Err(CompilerError::codegen_error(
                    format!("Property access '{}' not supported on variable '{}' (not namespace or any type)", property, namespace_name),
                    None,
                    None
                ))
            }
        } else if let Expression::PropertyAccess { .. } = object {
            // Chained property access on Any type (e.g., data.section.subsection)
            // The inner property access result should be Any type
            self.generate_any_property_access(object, property, instructions)
        } else if let Expression::IndexAccess(..) | Expression::ListAccess(..) = object {
            // Index access on Any type (e.g., data.items[0].name)
            self.generate_any_property_access(object, property, instructions)
        } else if let Expression::FunctionCall { .. } = object {
            // Function call that returns Any type (e.g., json.textToData(text).field)
            self.generate_any_property_access(object, property, instructions)
        } else {
            Err(CompilerError::codegen_error(
                format!("Property access '{}' not supported for this object type", property),
                None,
                None
            ))
        }
    }

    /// Check if a variable is of type Any (dynamic/JSON type)
    fn is_any_type_variable(&self, name: &str) -> bool {
        if let Some(var_type) = self.variable_types.get(name) {
            matches!(var_type, Type::Any)
        } else {
            false
        }
    }

    /// Generate property access on Any type (JSON object)
    /// Translates data.field into __json_get_field(data, "field", field_len)
    fn generate_any_property_access(
        &mut self,
        object: &Expression,
        property: &str,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // 1. Generate the object expression (puts any_ptr on stack)
        self.generate_expression(object, instructions)?;

        // 2. Add property name to string pool and push pointer
        // add_string_to_pool returns pointer to [i32 length][bytes...]
        // __json_get_field will handle the length prefix internally
        let key_ptr = self.add_string_to_pool(property);
        instructions.push(Instruction::I32Const(key_ptr as i32));

        // 3. Push property name length
        let key_len = property.len() as i32;
        instructions.push(Instruction::I32Const(key_len));

        // 4. Call __json_get_field(any_ptr, key_ptr, key_len) -> any_ptr
        let func_index = self.get_function_index_or_error("__json_get_field")?;
        instructions.push(Instruction::Call(func_index));

        Ok(WasmType::I32) // Returns Any pointer
    }

    fn generate_array_access_expression(
        &mut self,
        array: &Expression,
        index: &Expression,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Normal array access
        // Generate the array expression (should yield array pointer)
        let array_type = self.generate_expression(array, instructions)?;

        // Ensure we have an array pointer (I32 in WebAssembly)
        if array_type != WasmType::I32 {
            return Err(CompilerError::codegen_error(
                format!("Array expression must evaluate to pointer (I32), got: {:?}", array_type),
                None,
                None
            ));
        }

        // Generate the index expression (should yield integer)
        let index_type = self.generate_expression(index, instructions)?;

        // Ensure index is integer
        if index_type != WasmType::I32 {
            return Err(CompilerError::codegen_error(
                format!("Array index must be integer (I32), got: {:?}", index_type),
                None,
                None
            ));
        }

        // Call runtime function for safe array access with bounds checking
        // Stack: [array_ptr, index] -> [element_value]
        let func_index = self.get_function_index_or_error("array_get")?;
        instructions.push(Instruction::Call(func_index));

        // Determine return type based on the array's element type
        let element_type = self.get_array_element_type(array)?;
        Ok(element_type)
    }

    fn generate_list_assignment_expression(
        &mut self,
        list: &Expression,
        index: &Expression,
        value: &Expression,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Generate the list expression (should yield list pointer)
        let list_type = self.generate_expression(list, instructions)?;

        // Ensure we have a list pointer (I32 in WebAssembly)
        if list_type != WasmType::I32 {
            return Err(CompilerError::codegen_error(
                format!("List expression must evaluate to pointer (I32), got: {:?}", list_type),
                None,
                None
            ));
        }

        // Generate the index expression (should yield integer)
        let index_type = self.generate_expression(index, instructions)?;

        // Ensure index is integer
        if index_type != WasmType::I32 {
            return Err(CompilerError::codegen_error(
                format!("List index must be integer (I32), got: {:?}", index_type),
                None,
                None
            ));
        }

        // Generate the value expression
        let value_type = self.generate_expression(value, instructions)?;

        // Call runtime function for safe list assignment with bounds checking
        // Stack: [list_ptr, index, value] -> []
        let func_index = self.get_function_index_or_error("list_set")?;
        instructions.push(Instruction::Call(func_index));

        // List assignment returns void
        Ok(WasmType::Void)
    }

    fn generate_list_literal_expression(
        &mut self,
        elements: &[Expression],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        if elements.is_empty() {
            // Empty list - allocate empty array
            let empty_values: Vec<Value> = Vec::new();
            let array_ptr = self.memory_utils.allocate_array(&empty_values)?;
            instructions.push(Instruction::I32Const(array_ptr as i32));
            return Ok(WasmType::I32);
        }
        
        // For now, only handle literal values - non-literal expressions require runtime array construction
        let mut values = Vec::new();
        
        for element_expr in elements {
            match element_expr {
                Expression::Literal(value) => {
                    values.push(value.clone());
                },
                Expression::Variable(_) => {
                    // For variables, we need to generate runtime array creation
                    return self.generate_runtime_list_literal(elements, instructions);
                },
                _ => {
                    // For complex expressions, we need to generate runtime array creation
                    return self.generate_runtime_list_literal(elements, instructions);
                }
            }
        }
        
        // All elements are literals - we can create the array at compile time
        let array_ptr = self.memory_utils.allocate_array(&values)?;
        instructions.push(Instruction::I32Const(array_ptr as i32));
        
        Ok(WasmType::I32) // Return pointer to list
    }
    
    fn generate_runtime_list_literal(
        &mut self,
        elements: &[Expression],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // For runtime list construction, we need to:
        // 1. Create an empty array with the correct size
        // 2. Generate each element and store it in the array
        
        // First, create an empty array with the required size
        instructions.push(Instruction::I32Const(elements.len() as i32));
        // Note: Uses simplified array creation (runtime list_new handles actual allocation)
        
        // Generate code for each element
        for (index, element_expr) in elements.iter().enumerate() {
            // Duplicate the array pointer for the set operation
            instructions.push(Instruction::LocalGet(0)); // Assume array is in local 0
            instructions.push(Instruction::I32Const(index as i32));
            
            // Generate the element value
            self.generate_expression(element_expr, instructions)?;
            
            // Call array_set function
            let func_index = self.get_function_index_or_error("array_set")?;
            instructions.push(Instruction::Call(func_index));
            
            // array_set returns void, but we need the array pointer for the next iteration
            instructions.push(Instruction::LocalGet(0));
        }
        
        // Return array pointer
        instructions.push(Instruction::LocalGet(0));
        
        Ok(WasmType::I32)
    }

    fn generate_matrix_literal_expression(
        &mut self,
        rows: &[Vec<f64>],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        if rows.is_empty() {
            return Err(CompilerError::codegen_error(
                "Empty matrix literals are not supported".to_string(),
                None,
                None
            ));
        }

        // Validate matrix dimensions (all rows must have same length)
        let num_cols = rows[0].len();
        for row in rows {
            if row.len() != num_cols {
                return Err(CompilerError::codegen_error(
                    "Matrix literal has inconsistent row lengths".to_string(),
                    None,
                    None
                ));
            }
        }

        let num_rows = rows.len();
        
        // Create flattened array of all elements (row-major order)
        let mut flat_values = Vec::new();
        for row in rows {
            for &value in row {
                flat_values.push(Value::Number(value));
            }
        }

        // Allocate the flattened data array first
        let data_array_ptr = self.memory_utils.allocate_array(&flat_values)?;

        // Generate instructions to create the matrix
        // Push rows count
        instructions.push(Instruction::I32Const(num_rows as i32));
        
        // Push columns count  
        instructions.push(Instruction::I32Const(num_cols as i32));
        
        // Push data array pointer
        instructions.push(Instruction::I32Const(data_array_ptr as i32));

        // Call matrix.createNumber function (since parser converts all numbers to f64)
        let create_func_index = self.get_function_index_or_error("matrix.createNumber")?;
        instructions.push(Instruction::Call(create_func_index));

        Ok(WasmType::I32) // Return matrix pointer
    }

    fn generate_onerror_expression(
        &mut self,
        main_expr: &Expression,
        _fallback_expr: &Expression,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Simplified error handling - evaluates main expression (WASM exception handling pending spec stabilization)
        self.generate_expression(main_expr, instructions)
    }

    #[allow(dead_code)]
    fn generate_chained_method_call_expression(
        &mut self,
        _object: &Expression,
        _calls: &[(String, Vec<Expression>)],
        _instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        Err(CompilerError::codegen_error(
            "Chained method calls not yet implemented".to_string(),
            None,
            None
        ))
    }

    fn generate_static_method_call_expression(
        &mut self,
        class_name: &str,
        method_name: &str,
        arguments: &[Expression],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Handle lowercase namespace calls (e.g., math.max)
        let normalized_class_name = if class_name.chars().all(|c| c.is_lowercase() || c == '_') {
            match class_name {
                "math" => "Math",
                "string" => "String",
                "array" | "list" => "Array",
                "console" => "Console",
                "file" => "File",
                "http" => "Http",
                _ => class_name,
            }
        } else {
            class_name
        };

        // Use the existing builtin static method call implementation
        self.generate_builtin_static_method_call(normalized_class_name, method_name, arguments, instructions)
    }

    fn generate_power_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::F64, WasmType::F64) => {
                // Both operands are already F64, directly call math_pow
                let pow_func = self.get_function_index_or_error("math_pow")?;
                instructions.push(Instruction::Call(pow_func));
                Ok(WasmType::F64)
            },
            (WasmType::I32, WasmType::I32) => {
                // Convert both I32 operands to F64, call math_pow, result stays F64
                // Stack: [left_i32, right_i32] -> need to manage conversion order properly
                
                // Store right operand temporarily since we need to convert left first
                let temp_local = self.add_local(WasmType::I32);
                instructions.push(Instruction::LocalSet(temp_local)); // Stack: [left_i32]
                
                // Convert left operand to F64
                instructions.push(Instruction::F64ConvertI32S); // Stack: [left_f64]
                
                // Get and convert right operand to F64
                instructions.push(Instruction::LocalGet(temp_local)); // Stack: [left_f64, right_i32]
                instructions.push(Instruction::F64ConvertI32S); // Stack: [left_f64, right_f64]
                
                let pow_func = self.get_function_index_or_error("math_pow")?;
                instructions.push(Instruction::Call(pow_func)); // Stack: [result_f64]
                Ok(WasmType::F64) // Power operations always return F64 (number type)
            },
            (WasmType::I32, WasmType::F64) => {
                // Convert left I32 to F64, right is already F64
                // Stack: [left_i32, right_f64] -> [left_f64, right_f64] -> [result_f64]
                let temp_local = self.add_local(WasmType::F64);
                instructions.push(Instruction::LocalSet(temp_local)); // Store right F64
                instructions.push(Instruction::F64ConvertI32S); // Convert left I32 to F64
                instructions.push(Instruction::LocalGet(temp_local)); // Get right F64
                let pow_func = self.get_function_index_or_error("math_pow")?;
                instructions.push(Instruction::Call(pow_func));
                Ok(WasmType::F64)
            },
            (WasmType::F64, WasmType::I32) => {
                // Left is already F64, convert right I32 to F64
                // Stack: [left_f64, right_i32] -> [left_f64, right_f64] -> [result_f64]
                instructions.push(Instruction::F64ConvertI32S); // Convert right I32 to F64
                let pow_func = self.get_function_index_or_error("math_pow")?;
                instructions.push(Instruction::Call(pow_func));
                Ok(WasmType::F64)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Power operation not supported for types {:?} and {:?}", left_type, right_type),
                None,
                None
            ))
        }
    }

    fn generate_is_operation(
        &mut self,
        left_type: WasmType,
        right_type: WasmType,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                instructions.push(Instruction::I32Eq);
                Ok(WasmType::I32)
            },
            (WasmType::F64, WasmType::F64) => {
                instructions.push(Instruction::F64Eq);
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Is operation not supported for types {:?} and {:?}", left_type, right_type),
                None,
                None
            ))
        }
    }

    fn generate_not_operation(
        &mut self,
        _left_type: WasmType,
        _right_type: WasmType,
        _instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // "Not" is typically a unary operator, this might be a design issue
        Err(CompilerError::codegen_error(
            "Binary 'not' operation is not standard - should be unary".to_string(),
            None,
            None
        ))
    }

    /// Generate WASM instructions for string interpolation
    fn generate_string_interpolation_expression(
        &mut self,
        parts: &[StringPart],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        if parts.is_empty() {
            // Empty interpolation - return empty string
            let empty_string_ptr = self.allocate_string("")?;
            instructions.push(Instruction::I32Const(empty_string_ptr as i32));
            return Ok(WasmType::I32);
        }

        // Handle single part as optimization
        if parts.len() == 1 {
            match &parts[0] {
                StringPart::Text(text) => {
                    let string_ptr = self.allocate_string(text)?;
                    instructions.push(Instruction::I32Const(string_ptr as i32));
                    return Ok(WasmType::I32);
                },
                StringPart::Interpolation(expr) => {
                    // Single interpolated expression - convert to string
                    return self.generate_expression_to_string(expr, instructions);
                }
            }
        }

        // Multiple parts - need to concatenate
        
        for (i, part) in parts.iter().enumerate() {
            match part {
                StringPart::Text(text) => {
                    let string_ptr = self.allocate_string(text)?;
                    instructions.push(Instruction::I32Const(string_ptr as i32));
                },
                StringPart::Interpolation(expr) => {
                    // Generate expression and convert to string
                    self.generate_expression_to_string(expr, instructions)?;
                }
            }

            // If this is not the first part, concatenate with the previous result
            if i > 0 {
                let func_index = self.get_function_index_or_error("string.concat")?;
                instructions.push(Instruction::Call(func_index));
            }
        }

        Ok(WasmType::I32)
    }

    /// Generate WASM instructions to convert an expression to string
    fn generate_expression_to_string(
        &mut self,
        expr: &Expression,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Generate the expression first
        let expr_type = self.generate_expression(expr, instructions)?;
        
        // Convert to string based on the expression type
        match expr_type {
            WasmType::I32 => {
                // Check if this is actually a boolean by examining the expression
                let is_boolean = match expr {
                    Expression::Variable(var_name) => {
                        if let Some(var_type) = self.variable_types.get(var_name) {
                            matches!(var_type, crate::ast::Type::Boolean)
                        } else {
                            false
                        }
                    },
                    Expression::Literal(Value::Boolean(_)) => true,
                    _ => false
                };
                
                if is_boolean {
                    let func_index = self.get_function_index_or_error("bool_to_string")?;
                    instructions.push(Instruction::Call(func_index));
                } else {
                    // Check if it's a string (I32 pointer)
                    let is_string = match expr {
                        Expression::Variable(var_name) => {
                            if let Some(var_type) = self.variable_types.get(var_name) {
                                matches!(var_type, crate::ast::Type::String)
                            } else {
                                false
                            }
                        },
                        Expression::Literal(Value::String(_)) => true,
                        _ => false
                    };
                    
                    if !is_string {
                        // It's an integer, convert it
                        let func_index = self.get_function_index_or_error("int_to_string")?;
                        instructions.push(Instruction::Call(func_index));
                    }
                    // If it's already a string, no conversion needed
                }
            },
            WasmType::F64 => {
                let func_index = self.get_function_index_or_error("float_to_string")?;
                instructions.push(Instruction::Call(func_index));
            },
            _ => {
                // For other types, try to convert as integer
                let func_index = self.get_function_index_or_error("int_to_string")?;
                instructions.push(Instruction::Call(func_index));
            }
        }
        
        Ok(WasmType::I32)
    }

    /// Check if a variable name represents a namespace (compare, conditional, logical)
    fn is_namespace_variable(&self, name: &str) -> bool {
        matches!(name, "compare" | "conditional" | "logical" | "math" | "string" | "input")
    }

    /// Generate WASM instructions for base constructor calls
    fn generate_base_call_expression(
        &mut self,
        arguments: &[Expression],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Base calls in WASM are effectively no-ops for the runtime,
        // but we need to evaluate the arguments to ensure side effects are preserved
        for argument in arguments {
            let _arg_type = self.generate_expression(argument, instructions)?;
            // Discard the result since base calls don't return values in this context
            instructions.push(Instruction::Drop);
        }
        
        // Base calls conceptually return "void" but we use I32 as our void representation
        instructions.push(Instruction::I32Const(0));
        Ok(WasmType::I32)
    }

    /// Generate WASM instructions for conditional expressions (if-then-else)
    fn generate_conditional_expression(
        &mut self,
        condition: &Expression,
        then_expr: &Expression,
        else_expr: &Expression,
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Generate condition
        let _condition_type = self.generate_expression(condition, instructions)?;
        
        // Use WASM's select instruction for efficient conditional selection
        // Generate the two possible values
        let then_type = self.generate_expression(then_expr, instructions)?;
        let else_type = self.generate_expression(else_expr, instructions)?;
        
        // Ensure both branches have compatible types
        if then_type != else_type {
            return Err(CompilerError::codegen_error(
                format!("Conditional expression branches must have same type: then={:?}, else={:?}", 
                        then_type, else_type),
                None,
                None
            ));
        }
        
        // Use WASM select instruction: select(if_true_value, if_false_value, condition)
        // Stack: [condition, then_value, else_value] -> [result]
        match then_type {
            WasmType::I32 => {
                instructions.push(Instruction::Select);
            },
            WasmType::F64 => {
                // For f64, we can also use regular select since WASM 1.0 select works with any type
                instructions.push(Instruction::Select);
            },
            _ => return Err(CompilerError::codegen_error(
                format!("Unsupported type for conditional expression: {:?}", then_type),
                None,
                None
            ))
        }
        
        Ok(then_type)
    }

    /// Get the element type of an array expression for proper type inference
    fn get_array_element_type(&self, array_expr: &Expression) -> Result<WasmType, CompilerError> {
        match array_expr {
            Expression::Variable(var_name) => {
                // Look up variable type in variable_types table
                if let Some(var_type) = self.variable_types.get(var_name) {
                    match var_type {
                        crate::ast::Type::List(element_type) => {
                            // Convert AST type to WASM type
                            match element_type.as_ref() {
                                crate::ast::Type::Integer => Ok(WasmType::I32),
                                crate::ast::Type::Number => Ok(WasmType::F64),
                                crate::ast::Type::String => Ok(WasmType::I32), // String pointers
                                crate::ast::Type::Boolean => Ok(WasmType::I32),
                                _ => Ok(WasmType::I32), // Default to I32 for other types
                            }
                        },
                        _ => Err(CompilerError::codegen_error(
                            format!("Variable '{}' is not a list type", var_name),
                            None,
                            None
                        ))
                    }
                } else {
                    Err(CompilerError::codegen_error(
                        format!("Variable '{}' not found in type table", var_name),
                        None,
                        None
                    ))
                }
            },
            Expression::Literal(Value::List(elements)) => {
                // For list literals, infer type from first element
                if elements.is_empty() {
                    Ok(WasmType::I32) // Default for empty lists
                } else {
                    match &elements[0] {
                        Value::Integer(_) => Ok(WasmType::I32),
                        Value::Number(_) => Ok(WasmType::F64),
                        Value::String(_) => Ok(WasmType::I32),
                        Value::Boolean(_) => Ok(WasmType::I32),
                        _ => Ok(WasmType::I32),
                    }
                }
            },
            _ => {
                // For complex expressions, default to I32 and let runtime handle it
                Ok(WasmType::I32)
            }
        }
    }

    fn generate_static_method_call(
        &mut self,
        namespace: &str,
        method_name: &str,
        object: &Expression,
        arguments: &[Expression],
        instructions: &mut Vec<Instruction>
    ) -> Result<WasmType, CompilerError> {
        // Create a static method call - object becomes the first argument
        let mut all_arguments = vec![object.clone()];
        all_arguments.extend_from_slice(arguments);
        
        // Use the existing static method call generation
        self.generate_static_method_call_expression(namespace, method_name, &all_arguments, instructions)
    }
}