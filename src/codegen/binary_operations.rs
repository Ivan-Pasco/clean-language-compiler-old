use crate::ast::{BinaryOperator, Expression, Value};
use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::types::WasmType;
use wasm_encoder::{BlockType, Instruction};

impl CodeGenerator {
    pub fn generate_binary_operation_new(
        &mut self,
        left: &Expression,
        op: &BinaryOperator,
        right: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Special handling for string concatenation
        if let BinaryOperator::Add = op {
            if self.is_string_type_new(left) && self.is_string_type_new(right) {
                let _left_type = self.generate_expression(left, instructions)?;
                let _right_type = self.generate_expression(right, instructions)?;

                // Call string concatenation function
                if let Ok(concat_index) = self.get_string_concat_index() {
                    instructions.push(Instruction::Call(concat_index));
                    return Ok(WasmType::I32); // String pointer
                } else {
                    return Err(CompilerError::codegen_error(
                        "String concatenation function not found",
                        None,
                        None,
                    ));
                }
            }
        }

        let left_type = self.generate_expression(left, instructions)?;
        let right_type = self.generate_expression(right, instructions)?;

        // Special handling for division by zero
        if let BinaryOperator::Divide = op {
            match right {
                Expression::Literal(Value::Integer(0)) => {
                    return Err(CompilerError::division_by_zero_error(None));
                }
                Expression::Literal(Value::Number(n)) if *n == 0.0 => {
                    return Err(CompilerError::division_by_zero_error(None));
                }
                _ => {
                    // For non-literal divisors, add a runtime check
                    let temp_local_idx = self.add_local(right_type);
                    instructions.push(Instruction::LocalSet(temp_local_idx));
                    instructions.push(Instruction::LocalGet(temp_local_idx));

                    match right_type {
                        WasmType::I32 => {
                            instructions.push(Instruction::I32Eqz); // Check if zero
                            instructions.push(Instruction::If(BlockType::Empty));
                            instructions.push(Instruction::Unreachable);
                            instructions.push(Instruction::End);
                        }
                        WasmType::F64 => {
                            instructions.push(Instruction::F64Const(0.0));
                            instructions.push(Instruction::F64Eq); // Check if zero
                            instructions.push(Instruction::If(BlockType::Empty));
                            instructions.push(Instruction::Unreachable);
                            instructions.push(Instruction::End);
                        }
                        _ => {} // No check for other types
                    }
                    instructions.push(Instruction::LocalGet(temp_local_idx));
                }
            }
        }

        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                match op {
                    // Use correct AST variant names
                    BinaryOperator::Add => {
                        instructions.push(Instruction::I32Add);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Subtract => {
                        instructions.push(Instruction::I32Sub);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Multiply => {
                        instructions.push(Instruction::I32Mul);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Divide => {
                        instructions.push(Instruction::I32DivS);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Equal => {
                        instructions.push(Instruction::I32Eq);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::NotEqual => {
                        instructions.push(Instruction::I32Ne);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Less => {
                        instructions.push(Instruction::I32LtS);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Greater => {
                        instructions.push(Instruction::I32GtS);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::LessEqual => {
                        instructions.push(Instruction::I32LeS);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::GreaterEqual => {
                        instructions.push(Instruction::I32GeS);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Modulo => {
                        instructions.push(Instruction::I32RemS);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Power => {
                        // For I32 ^ I32, we need to convert both operands to F64,
                        // call pow, then convert the result back to I32
                        // Stack currently has: [left_i32, right_i32]

                        // Store right operand temporarily
                        let temp_local = self.add_local(WasmType::I32);
                        instructions.push(Instruction::LocalSet(temp_local));

                        // Convert left operand to F64
                        instructions.push(Instruction::F64ConvertI32S);

                        // Get right operand and convert to F64
                        instructions.push(Instruction::LocalGet(temp_local));
                        instructions.push(Instruction::F64ConvertI32S);

                        // Call power function
                        if let Some(pow_index) = self.get_function_index("pow") {
                            instructions.push(Instruction::Call(pow_index));
                            // Convert result back to I32 for integer operands
                            instructions.push(Instruction::I32TruncF64S);
                            Ok(WasmType::I32)
                        } else {
                            Err(CompilerError::type_error(
                                "Power function not found".to_string(),
                                None,
                                None,
                            ))
                        }
                    }
                    BinaryOperator::And => {
                        instructions.push(Instruction::I32And);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Or => {
                        instructions.push(Instruction::I32Or);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Is => {
                        instructions.push(Instruction::I32Eq);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Not => {
                        instructions.push(Instruction::I32Ne);
                        Ok(WasmType::I32)
                    }
                }
            }

            (WasmType::F64, WasmType::F64) => {
                match op {
                    BinaryOperator::Add => {
                        instructions.push(Instruction::F64Add);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Subtract => {
                        instructions.push(Instruction::F64Sub);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Multiply => {
                        instructions.push(Instruction::F64Mul);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Divide => {
                        instructions.push(Instruction::F64Div);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Modulo => {
                        // For F64 mod, call the mod function if available
                        if let Some(mod_index) = self.get_function_index("mod") {
                            instructions.push(Instruction::Call(mod_index));
                            Ok(WasmType::F64)
                        } else {
                            Err(CompilerError::type_error(
                                "Modulo function not found".to_string(),
                                None,
                                None,
                            ))
                        }
                    }
                    BinaryOperator::Power => {
                        // For F64 ^ F64, call the power function
                        if let Some(pow_index) = self.get_function_index("pow") {
                            instructions.push(Instruction::Call(pow_index));
                            Ok(WasmType::F64)
                        } else {
                            Err(CompilerError::type_error(
                                "Power function not found".to_string(),
                                None,
                                None,
                            ))
                        }
                    }
                    BinaryOperator::Equal => {
                        instructions.push(Instruction::F64Eq);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::NotEqual => {
                        instructions.push(Instruction::F64Ne);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Less => {
                        instructions.push(Instruction::F64Lt);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Greater => {
                        instructions.push(Instruction::F64Gt);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::LessEqual => {
                        instructions.push(Instruction::F64Le);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::GreaterEqual => {
                        instructions.push(Instruction::F64Ge);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::And => {
                        instructions.push(Instruction::I32TruncF64S);
                        instructions.push(Instruction::I32And);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Or => {
                        instructions.push(Instruction::I32TruncF64S);
                        instructions.push(Instruction::I32Or);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Is => {
                        instructions.push(Instruction::F64Eq);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Not => {
                        instructions.push(Instruction::F64Ne);
                        Ok(WasmType::I32)
                    }
                }
            }

            (WasmType::F32, WasmType::F32) => {
                match op {
                    BinaryOperator::Add => {
                        instructions.push(Instruction::F32Add);
                        Ok(WasmType::F32)
                    }
                    BinaryOperator::Subtract => {
                        instructions.push(Instruction::F32Sub);
                        Ok(WasmType::F32)
                    }
                    BinaryOperator::Multiply => {
                        instructions.push(Instruction::F32Mul);
                        Ok(WasmType::F32)
                    }
                    BinaryOperator::Divide => {
                        instructions.push(Instruction::F32Div);
                        Ok(WasmType::F32)
                    }
                    BinaryOperator::Modulo => {
                        // F32 modulo requires conversion to F64
                        // Stack currently has: [F32_left, F32_right]
                        // Store right operand temporarily
                        let temp_f32_local = self.add_local(WasmType::F32);
                        instructions.push(Instruction::LocalSet(temp_f32_local));

                        // Convert left operand to F64
                        instructions.push(Instruction::F64PromoteF32);

                        // Get right operand and convert to F64
                        instructions.push(Instruction::LocalGet(temp_f32_local));
                        instructions.push(Instruction::F64PromoteF32);

                        if let Some(mod_index) = self.get_function_index("mod") {
                            instructions.push(Instruction::Call(mod_index));
                            instructions.push(Instruction::F32DemoteF64);
                            Ok(WasmType::F32)
                        } else {
                            Err(CompilerError::type_error(
                                "Modulo function not found".to_string(),
                                None,
                                None,
                            ))
                        }
                    }
                    BinaryOperator::Power => {
                        // F32 power requires conversion to F64
                        // Stack currently has: [F32_left, F32_right]
                        // Store right operand temporarily
                        let temp_f32_local = self.add_local(WasmType::F32);
                        instructions.push(Instruction::LocalSet(temp_f32_local));

                        // Convert left operand to F64
                        instructions.push(Instruction::F64PromoteF32);

                        // Get right operand and convert to F64
                        instructions.push(Instruction::LocalGet(temp_f32_local));
                        instructions.push(Instruction::F64PromoteF32);

                        if let Some(pow_index) = self.get_function_index("pow") {
                            instructions.push(Instruction::Call(pow_index));
                            instructions.push(Instruction::F32DemoteF64);
                            Ok(WasmType::F32)
                        } else {
                            Err(CompilerError::type_error(
                                "Power function not found".to_string(),
                                None,
                                None,
                            ))
                        }
                    }
                    BinaryOperator::Equal => {
                        instructions.push(Instruction::F32Eq);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::NotEqual => {
                        instructions.push(Instruction::F32Ne);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Less => {
                        instructions.push(Instruction::F32Lt);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Greater => {
                        instructions.push(Instruction::F32Gt);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::LessEqual => {
                        instructions.push(Instruction::F32Le);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::GreaterEqual => {
                        instructions.push(Instruction::F32Ge);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::And => {
                        // Store right operand temporarily
                        let temp_f32_local = self.add_local(WasmType::F32);
                        instructions.push(Instruction::LocalSet(temp_f32_local));

                        // Convert left to I32
                        instructions.push(Instruction::I32TruncF32S);

                        // Get right and convert to I32
                        instructions.push(Instruction::LocalGet(temp_f32_local));
                        instructions.push(Instruction::I32TruncF32S);

                        instructions.push(Instruction::I32And);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Or => {
                        // Store right operand temporarily
                        let temp_f32_local = self.add_local(WasmType::F32);
                        instructions.push(Instruction::LocalSet(temp_f32_local));

                        // Convert left to I32
                        instructions.push(Instruction::I32TruncF32S);

                        // Get right and convert to I32
                        instructions.push(Instruction::LocalGet(temp_f32_local));
                        instructions.push(Instruction::I32TruncF32S);

                        instructions.push(Instruction::I32Or);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Is => {
                        instructions.push(Instruction::F32Eq);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Not => {
                        instructions.push(Instruction::F32Ne);
                        Ok(WasmType::I32)
                    }
                }
            }

            (WasmType::I32, WasmType::F64) => {
                // Convert I32 to F64 and perform F64 operation
                // Need to convert the I32 (left operand) to F64
                // Stack currently has: [I32_left, F64_right]
                // We need: [F64_left, F64_right]

                // Store the F64 right operand temporarily
                let temp_f64_local = self.add_local(WasmType::F64);
                instructions.push(Instruction::LocalSet(temp_f64_local));

                // Convert the I32 left operand to F64
                instructions.push(Instruction::F64ConvertI32S);

                // Restore the F64 right operand
                instructions.push(Instruction::LocalGet(temp_f64_local));

                match op {
                    BinaryOperator::Add => {
                        instructions.push(Instruction::F64Add);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Subtract => {
                        instructions.push(Instruction::F64Sub);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Multiply => {
                        instructions.push(Instruction::F64Mul);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Divide => {
                        instructions.push(Instruction::F64Div);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Equal => {
                        instructions.push(Instruction::F64Eq);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::NotEqual => {
                        instructions.push(Instruction::F64Ne);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Less => {
                        instructions.push(Instruction::F64Lt);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Greater => {
                        instructions.push(Instruction::F64Gt);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::LessEqual => {
                        instructions.push(Instruction::F64Le);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::GreaterEqual => {
                        instructions.push(Instruction::F64Ge);
                        Ok(WasmType::I32)
                    }
                    _ => Err(CompilerError::type_error(
                        format!("Unsupported operation {:?} between I32 and F64", op),
                        None,
                        None,
                    )),
                }
            }

            (WasmType::F64, WasmType::I32) => {
                // Convert I32 to F64 and perform F64 operation
                // Stack currently has: [F64_left, I32_right]
                // We need: [F64_left, F64_right]

                // Convert the I32 right operand to F64
                instructions.push(Instruction::F64ConvertI32S);

                match op {
                    BinaryOperator::Add => {
                        instructions.push(Instruction::F64Add);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Subtract => {
                        instructions.push(Instruction::F64Sub);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Multiply => {
                        instructions.push(Instruction::F64Mul);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Divide => {
                        instructions.push(Instruction::F64Div);
                        Ok(WasmType::F64)
                    }
                    BinaryOperator::Equal => {
                        instructions.push(Instruction::F64Eq);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::NotEqual => {
                        instructions.push(Instruction::F64Ne);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Less => {
                        instructions.push(Instruction::F64Lt);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::Greater => {
                        instructions.push(Instruction::F64Gt);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::LessEqual => {
                        instructions.push(Instruction::F64Le);
                        Ok(WasmType::I32)
                    }
                    BinaryOperator::GreaterEqual => {
                        instructions.push(Instruction::F64Ge);
                        Ok(WasmType::I32)
                    }
                    _ => Err(CompilerError::type_error(
                        format!("Unsupported operation {:?} between F64 and I32", op),
                        None,
                        None,
                    )),
                }
            }

            _ => Err(CompilerError::type_error(
                format!(
                    "Unsupported operation {:?} between {:?} and {:?}",
                    op, left_type, right_type
                ),
                None,
                None,
            )),
        }
    }

    pub fn is_string_type_new(&self, expr: &Expression) -> bool {
        match expr {
            // Correct patterns
            Expression::Literal(Value::String(_)) => true,
            Expression::Variable(_name) => {
                /* ... */
                false
            } // Needs type lookup
            Expression::StringInterpolation(_) => true,
            _ => false,
        }
    }
}
