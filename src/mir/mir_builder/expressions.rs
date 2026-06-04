//! Expression lowering — lowers TAST expressions to MIR instructions.

use super::*;

impl MirBuilder {
    pub(super) fn build_expression(
        &mut self,
        context: &mut FunctionBuildContext,
        expression: &TastExpression,
    ) -> Result<ValueId, Vec<CompilerError>> {
        trace!(expression_kind = ?std::mem::discriminant(&expression.kind), "Processing expression");
        match &expression.kind {
            TastExpressionKind::Literal { value } => {
                let constant = self.convert_literal(value);
                let value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the ValueId as a temporary local for codegen
                let mir_type = self.convert_literal_type(value);
                self.register_temp_local(context, value_id, mir_type, expression.location.clone());

                let instruction = MirInstruction {
                    dest: Some(value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(constant),
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(value_id)
            }

            TastExpressionKind::Variable { symbol_id: _, name } => {
                trace!(
                    variable_name = %name,
                    has_class_context = context.class_context.is_some(),
                    scope_stack_depth = context.scope_stack.len(),
                    "Processing variable"
                );

                // Special case for 'this' - in class methods, 'this' refers to the first parameter
                if name == "this" && context.class_context.is_some() {
                    // In class methods, 'this' is the first parameter
                    // Get the actual ValueId of the first parameter
                    if let Some(first_param) = context.function.parameters.first() {
                        // Return the parameter's ValueId directly - no need to copy
                        return Ok(first_param.value_id);
                    } else {
                        return Err(vec![CompilerError::validation_error(
                            "'this' used in method without instance parameter",
                            expression.location.clone(),
                        )]);
                    }
                }

                // Look up variable in scope stack
                for scope in context.scope_stack.iter().rev() {
                    if let Some(&value_id) = scope.get(name) {
                        return Ok(value_id);
                    }
                }

                // Check if this variable refers to a computed property.
                // Computed properties are implemented as zero-parameter getter functions
                // named `__computed_{name}`.  Reading a computed variable emits a Call
                // to that getter so the body is re-evaluated every time.
                if let Some((getter_symbol_id, return_mir_type)) =
                    self.computed_properties.get(name).cloned()
                {
                    trace!(
                        computed_name = %name,
                        getter_symbol_id = getter_symbol_id.0,
                        return_mir_type = ?return_mir_type,
                        "Found computed property, emitting Call to getter"
                    );

                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Register the result local so codegen can allocate a slot for it.
                    let local = MirLocal {
                        name: Some(format!("computed_{}", name)),
                        local_type: return_mir_type,
                        is_mutable: false,
                        location: expression.location.clone(),
                    };
                    context.function.locals.insert(result_id, local);

                    // Emit a Call instruction to the getter function.
                    let call_instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::Function(getter_symbol_id),
                            arguments: vec![],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, call_instruction);

                    return Ok(result_id);
                }

                // Check if this is a state variable - emit GlobalLoad instruction
                if let Some((symbol_id, mir_type)) = self.state_variables.get(name).cloned() {
                    trace!(
                        state_variable = %name,
                        symbol_id = ?symbol_id,
                        mir_type = ?mir_type,
                        "Found state variable, emitting GlobalLoad"
                    );

                    // Create a new ValueId for the loaded value
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Register the result as a local for type tracking
                    let local = MirLocal {
                        name: Some(format!("loaded_state_{}", name)),
                        local_type: mir_type.clone(),
                        is_mutable: false, // This is a loaded value, not the actual global
                        location: expression.location.clone(),
                    };
                    context.function.locals.insert(result_id, local);

                    // Emit GlobalLoad instruction
                    let load_instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::GlobalLoad {
                            global_id: symbol_id,
                            global_type: mir_type,
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, load_instruction);

                    return Ok(result_id);
                }

                // If not found in scope and we're in a class method, check class fields.
                // Extract both the field index and field type before any mutable borrows.
                let field_info_opt = if let Some(ref class) = context.class_context {
                    trace!(
                        field_name = %name,
                        field_count = class.fields.len(),
                        "Looking for field in class"
                    );
                    let result = class
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(_, f)| f.name == *name)
                        .map(|(idx, f)| (idx, f.field_type.clone()));
                    trace!(result = ?result, "Field search result");
                    result
                } else {
                    trace!(variable_name = %name, "No class context for variable");
                    None
                };

                if let Some((field_index, field_concrete_type)) = field_info_opt {
                    trace!(
                        field_name = %name,
                        field_index = field_index,
                        "Found field, generating load instructions"
                    );
                    // This is an implicit field access - treat as this.field
                    // Need to load 'this', get field pointer, and load field value

                    // Get 'this' from the first parameter
                    let this_value_id =
                        if let Some(first_param) = context.function.parameters.first() {
                            first_param.value_id
                        } else {
                            return Err(vec![CompilerError::validation_error(
                                "Implicit field access in method without instance parameter",
                                expression.location.clone(),
                            )]);
                        };

                    // Create GetElementPtr to get field address
                    let field_ptr_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    // Register the pointer local (always I32)
                    context.function.locals.insert(
                        field_ptr_id,
                        MirLocal {
                            name: Some(format!("field_ptr_{}", name)),
                            local_type: MirType::I32,
                            is_mutable: false,
                            location: expression.location.clone(),
                        },
                    );

                    let gep_instruction = MirInstruction {
                        dest: Some(field_ptr_id),
                        operation: MirOperation::GetElementPtr {
                            base: MirOperand::Value(this_value_id),
                            indices: vec![MirOperand::Constant(MirConstant::Integer(
                                field_index as i64,
                            ))],
                            is_array: false, // Class field access - no header offset
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, gep_instruction);

                    // Load the field value — register with the correct MIR type so the
                    // codegen emits F64Load / I32Load correctly (not always I32Load).
                    let value_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    let field_mir_type = self.convert_concrete_type(&field_concrete_type);
                    context.function.locals.insert(
                        value_id,
                        MirLocal {
                            name: Some(format!("field_{}", name)),
                            local_type: field_mir_type,
                            is_mutable: false,
                            location: expression.location.clone(),
                        },
                    );

                    let load_instruction = MirInstruction {
                        dest: Some(value_id),
                        operation: MirOperation::Load {
                            source: MirOperand::Value(field_ptr_id),
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, load_instruction);

                    return Ok(value_id);
                }

                // Check if this is a function reference (first-class function)
                // Functions can be referenced by name and passed as values
                let is_function = self.all_functions.iter().any(|f| &f.name == name);

                if is_function {
                    // Function reference — emit the function's WASM index as i32 via
                    // `MirOperand::Function(symbol_id)`. Codegen resolves it to the
                    // concrete function index. The table + __indirect_function_table
                    // export make the value usable as a call_indirect target from
                    // the host side.
                    let symbol_id = self
                        .all_functions
                        .iter()
                        .find(|f| &f.name == name)
                        .map(|f| f.symbol_id)
                        .ok_or_else(|| {
                            vec![CompilerError::type_error(
                                format!("Function '{}' not found for reference", name),
                                None,
                                Some(expression.location.clone()),
                            )]
                        })?;

                    let value_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    let local = MirLocal {
                        name: Some(format!("funcref_{}", name)),
                        local_type: MirType::I32,
                        is_mutable: false,
                        location: expression.location.clone(),
                    };
                    context.function.locals.insert(value_id, local);

                    let copy_instruction = MirInstruction {
                        dest: Some(value_id),
                        operation: MirOperation::Copy {
                            source: MirOperand::Function(symbol_id),
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, copy_instruction);

                    return Ok(value_id);
                }

                // Parent class field access resolved via inheritance chain at codegen time
                Err(vec![CompilerError::type_error(
                    format!("Undefined variable: {}", name),
                    None,
                    Some(expression.location.clone()),
                )])
            }

            TastExpressionKind::BinaryOperation {
                operator,
                left,
                right,
            } => {
                // NOTE: Handle Power operator as runtime function call
                if matches!(operator, BinaryOperator::Power) {
                    // Power operation requires runtime pow function
                    // For integers: pow_i32(base, exponent) -> i32
                    // For floats: pow_f64(base, exponent) -> f64
                    let left_id = self.build_expression(context, left)?;
                    let right_id = self.build_expression(context, right)?;
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Determine result type based on operand types
                    let result_type = if matches!(left.expr_type, ConcreteType::Number)
                        || matches!(right.expr_type, ConcreteType::Number)
                    {
                        MirType::F64 // Float power
                    } else {
                        MirType::I32 // Integer power
                    };

                    self.register_temp_local(
                        context,
                        result_id,
                        result_type.clone(),
                        expression.location.clone(),
                    );

                    // Use SymbolId(1001) for pow_f64, SymbolId(1002) for pow_i32
                    let pow_function = if matches!(result_type, MirType::F64) {
                        SymbolId(1001) // pow_f64
                    } else {
                        SymbolId(1002) // pow_i32
                    };

                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::Function(pow_function),
                            arguments: vec![
                                MirOperand::Value(left_id),
                                MirOperand::Value(right_id),
                            ],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    return Ok(result_id);
                }

                // BOOK: null-coalescing - Handle NullCoalesce operator (value default fallback)
                // Returns left if not null, otherwise returns right
                // IMPORTANT: 0, false, "" are NOT null - only the null literal is null
                if matches!(operator, BinaryOperator::NullCoalesce) {
                    use crate::typechecker::tast::{TastExpressionKind, TastLiteral};

                    // Check if left is definitely null (literal null or Null type)
                    let left_is_null_literal = matches!(
                        &left.kind,
                        TastExpressionKind::Literal {
                            value: TastLiteral::Null
                        }
                    );
                    let left_is_null_type = matches!(&left.expr_type, ConcreteType::Null);

                    if left_is_null_literal || left_is_null_type {
                        // Left is definitely null - just return the fallback (right)
                        return self.build_expression(context, right);
                    }

                    // Check if left is definitely NOT null (non-null literal)
                    let left_is_non_null_literal = match &left.kind {
                        TastExpressionKind::Literal { value } => {
                            !matches!(value, TastLiteral::Null)
                        }
                        _ => false,
                    };

                    if left_is_non_null_literal {
                        // Left is definitely NOT null - just return the original (left)
                        return self.build_expression(context, left);
                    }

                    // For variables and other expressions, we need runtime checking
                    // However, since we don't have proper nullable types yet, we use a
                    // special "null marker" approach: null is represented as a pointer value
                    // of 0 for reference types, but for primitives (int, bool), 0/false are valid values.
                    //
                    // Current behavior: If we can't determine at compile time, we check
                    // if the expression type could be null. For now, we just return the left value
                    // since we can't distinguish null from 0/false at runtime for primitives.
                    //
                    // Note: A proper nullable type system (Option<T> or T?) would improve this.

                    // For now, evaluate left and return it (we don't have runtime null tracking)
                    // This handles the case where left is a variable that could be null
                    // In the future, we'll track nullability in the type system
                    let left_id = self.build_expression(context, left)?;

                    // Get the MIR type of the left expression
                    let left_mir_type = context
                        .function
                        .locals
                        .get(&left_id)
                        .map(|local| local.local_type.clone())
                        .unwrap_or_else(|| MirType::from_concrete_type(&left.expr_type));

                    // Evaluate right expression (the fallback value) - needed for later
                    let right_id = self.build_expression(context, right)?;

                    // Create result local with same type as left
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    self.register_temp_local(
                        context,
                        result_id,
                        left_mir_type.clone(),
                        expression.location.clone(),
                    );

                    // Generate select instruction: if left != 0 then left else right
                    // NOTE: This only works correctly for reference types (pointers) where
                    // null = 0 and valid values are non-zero. For primitives like int and bool,
                    // this will incorrectly treat 0 and false as null.
                    // A proper nullable type system would allow distinguishing null from valid zero values.
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Select {
                            condition: MirOperand::Value(left_id),
                            true_value: MirOperand::Value(left_id),
                            false_value: MirOperand::Value(right_id),
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    return Ok(result_id);
                }

                // Check if this is string concatenation (String + String or String + other)
                // Also include any type since string + any should use string concat with any.toString()
                let is_string_concat = matches!(operator, BinaryOperator::Add)
                    && (matches!(left.expr_type, ConcreteType::String)
                        || matches!(right.expr_type, ConcreteType::String)
                        || matches!(left.expr_type, ConcreteType::Any)
                        || matches!(right.expr_type, ConcreteType::Any));

                if is_string_concat {
                    // String concatenation uses runtime string.concat function
                    // string.concat(str1_ptr, str1_len, str2_ptr, str2_len) -> result_ptr
                    let left_id = self.build_expression(context, left)?;
                    let right_id = self.build_expression(context, right)?;

                    // Check if left/right are Any type (check both TAST and MIR)
                    // Need to do this before mutable borrows of context
                    let left_is_any = if matches!(left.expr_type, ConcreteType::Any) {
                        true
                    } else if matches!(left.expr_type, ConcreteType::Unknown) {
                        context
                            .function
                            .locals
                            .get(&left_id)
                            .map(|local| matches!(local.local_type, MirType::Any))
                            .unwrap_or(false)
                    } else {
                        false
                    };

                    let right_is_any = if matches!(right.expr_type, ConcreteType::Any) {
                        true
                    } else if matches!(right.expr_type, ConcreteType::Unknown) {
                        context
                            .function
                            .locals
                            .get(&right_id)
                            .map(|local| matches!(local.local_type, MirType::Any))
                            .unwrap_or(false)
                    } else {
                        false
                    };

                    // Convert any values to strings using AnyToString
                    let left_string_id = if left_is_any {
                        let any_to_string_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;
                        // Register as Ptr(U8) - AnyToString returns a string pointer
                        self.register_temp_local(
                            context,
                            any_to_string_id,
                            MirType::Ptr(Box::new(MirType::U8)),
                            left.location.clone(),
                        );
                        let instruction = MirInstruction {
                            dest: Some(any_to_string_id),
                            operation: MirOperation::AnyToString {
                                value: MirOperand::Value(left_id),
                            },
                            location: left.location.clone(),
                        };
                        self.add_instruction(context, instruction);
                        any_to_string_id
                    } else if !matches!(left.expr_type, ConcreteType::String) {
                        // Convert non-string to string
                        self.convert_value_to_string(
                            context,
                            left_id,
                            &left.expr_type,
                            &left.location,
                        )?
                    } else {
                        left_id
                    };

                    let right_string_id = if right_is_any {
                        let any_to_string_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;
                        // Register as Ptr(U8) - AnyToString returns a string pointer
                        self.register_temp_local(
                            context,
                            any_to_string_id,
                            MirType::Ptr(Box::new(MirType::U8)),
                            right.location.clone(),
                        );
                        let instruction = MirInstruction {
                            dest: Some(any_to_string_id),
                            operation: MirOperation::AnyToString {
                                value: MirOperand::Value(right_id),
                            },
                            location: right.location.clone(),
                        };
                        self.add_instruction(context, instruction);
                        any_to_string_id
                    } else if !matches!(right.expr_type, ConcreteType::String) {
                        // Convert non-string to string
                        self.convert_value_to_string(
                            context,
                            right_id,
                            &right.expr_type,
                            &right.location,
                        )?
                    } else {
                        right_id
                    };

                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Result is a string pointer to [len|content] structure in memory
                    // NOTE: Use Ptr(U8) to distinguish string pointers from integers
                    // This ensures print() knows to expand as string (ptr+4, len) not convert via int_to_string
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::Ptr(Box::new(MirType::U8)),
                        expression.location.clone(),
                    );

                    // Generate call to string.concat runtime function
                    // Use SymbolId(1000) as a fixed ID for string.concat built-in
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::Function(SymbolId(1000)),
                            arguments: vec![
                                MirOperand::Value(left_string_id),
                                MirOperand::Value(right_string_id),
                            ],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    Ok(result_id)
                } else {
                    // Regular binary operation (arithmetic, comparison, etc.)
                    let left_id = self.build_expression(context, left)?;
                    let right_id = self.build_expression(context, right)?;
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // NOTE: Use actual MIR types from built expressions, not TAST expr_type
                    // TAST expr_type may be Unknown for method calls like toNumber()
                    let left_mir_type = context
                        .function
                        .locals
                        .get(&left_id)
                        .map(|local| local.local_type.clone())
                        .unwrap_or_else(|| MirType::from_concrete_type(&left.expr_type));
                    let right_mir_type = context
                        .function
                        .locals
                        .get(&right_id)
                        .map(|local| local.local_type.clone())
                        .unwrap_or_else(|| MirType::from_concrete_type(&right.expr_type));

                    let left_concrete = Self::mir_type_to_concrete(&left_mir_type);
                    let right_concrete = Self::mir_type_to_concrete(&right_mir_type);

                    let result_type =
                        self.infer_binary_operation_type(&left_concrete, &right_concrete, operator);
                    self.register_temp_local(
                        context,
                        result_id,
                        result_type,
                        expression.location.clone(),
                    );

                    let mir_op = self.convert_binary_op(operator);
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::BinaryOp {
                            op: mir_op,
                            left: MirOperand::Value(left_id),
                            right: MirOperand::Value(right_id),
                        },
                        location: expression.location.clone(),
                    };

                    self.add_instruction(context, instruction);
                    Ok(result_id)
                }
            }

            TastExpressionKind::UnaryOperation { operator, operand } => {
                // NOTE: Handle unary Plus as a no-op (identity operation)
                if matches!(operator, UnaryOperator::Plus) {
                    // Unary plus is a no-op - just return the operand value unchanged
                    return self.build_expression(context, operand);
                }

                // NOTE: Increment/Decrement operators need special desugaring
                // These operators should have been desugared by the TAST phase, but if they reach here,
                // we need to handle them properly with proper state mutation
                match operator {
                    UnaryOperator::PreIncrement
                    | UnaryOperator::PostIncrement
                    | UnaryOperator::PreDecrement
                    | UnaryOperator::PostDecrement => {
                        return Err(vec![CompilerError::validation_error(
                            format!(
                                "Increment/decrement operators must be desugared before MIR lowering. \
                                 Operator {:?} cannot be directly lowered to MIR.",
                                operator
                            ),
                            expression.location.clone(),
                        )]);
                    }
                    _ => {}
                }

                let operand_id = self.build_expression(context, operand)?;
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the result as a temporary local for codegen
                let result_type = self.infer_unary_operation_type(&operand.expr_type);
                self.register_temp_local(
                    context,
                    result_id,
                    result_type,
                    expression.location.clone(),
                );

                let mir_op = self.convert_unary_op(operator);
                let instruction = MirInstruction {
                    dest: Some(result_id),
                    operation: MirOperation::UnaryOp {
                        op: mir_op,
                        operand: MirOperand::Value(operand_id),
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(result_id)
            }

            TastExpressionKind::FunctionCall {
                function,
                arguments,
                type_args: _,
            } => {
                // Extract function symbol ID and name from the function expression
                let (function_symbol_id, function_name_opt) = match &function.kind {
                    TastExpressionKind::Variable {
                        symbol_id, name, ..
                    } => (*symbol_id, Some(name.clone())),
                    _ => {
                        return Err(vec![CompilerError::validation_error(
                            "Function calls to non-simple function names not yet supported",
                            function.location.clone(),
                        )])
                    }
                };

                // NOTE: Handle standalone toString(any) function calls
                // When calling toString(value) where value is Any type, we need to use
                // the AnyToString operation for proper runtime type dispatch
                if matches!(function_name_opt.as_deref(), Some("toString"))
                    && arguments.len() == 1
                    && matches!(arguments[0].expr_type, ConcreteType::Any)
                {
                    // Build the argument expression
                    let arg_id = self.build_expression(context, &arguments[0])?;

                    // Allocate result ValueId
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Register the result as Ptr(U8) to distinguish string pointers from Any
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::Ptr(Box::new(MirType::U8)),
                        expression.location.clone(),
                    );

                    // Generate AnyToString operation for runtime type dispatch
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::AnyToString {
                            value: MirOperand::Value(arg_id),
                        },
                        location: expression.location.clone(),
                    };

                    self.add_instruction(context, instruction);

                    trace!(
                        result_id = ?result_id,
                        arg_id = ?arg_id,
                        "toString(any) with AnyToString operation"
                    );

                    return Ok(result_id);
                }

                // Build argument operands
                let mut mir_arguments = Vec::new();

                // Check if this is a constructor call by examining the return type
                // Constructor calls return a Class type
                if let ConcreteType::Class {
                    symbol_id: class_symbol_id,
                    ..
                } = &expression.expr_type
                {
                    // This is a constructor call - we need to allocate instance memory
                    // and pass the instance pointer as the first argument

                    // Find the class definition to get field count
                    let class_def = context
                        .all_classes
                        .iter()
                        .find(|c| c.symbol_id == *class_symbol_id)
                        .ok_or_else(|| {
                            vec![CompilerError::validation_error(
                                format!(
                                    "Class definition not found for symbol {:?}",
                                    class_symbol_id
                                ),
                                expression.location.clone(),
                            )]
                        })?;

                    // Calculate TOTAL instance size including all inherited fields
                    // with proper byte sizes for each field type (i32=4, f64=8, etc.)
                    let instance_size =
                        self.calculate_instance_byte_size(context, *class_symbol_id);
                    let total_field_count =
                        self.count_all_fields_in_hierarchy(context, *class_symbol_id);
                    tracing::debug!(
                        "Allocating class {} with {} total fields ({} bytes)",
                        class_def.name,
                        total_field_count,
                        instance_size
                    );

                    // Generate Alloca instruction to allocate instance memory
                    // Note: For heap allocation, this should be converted to mem_alloc during codegen
                    let alloc_result = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Register the allocation result as a temporary local
                    self.register_temp_local(
                        context,
                        alloc_result,
                        MirType::I32,
                        expression.location.clone(),
                    );

                    // Create allocation instruction
                    let alloc_instr = MirInstruction {
                        dest: Some(alloc_result),
                        operation: MirOperation::Alloca {
                            size: MirOperand::Constant(MirConstant::Integer(instance_size as i64)),
                            alignment: 4,
                        },
                        location: expression.location.clone(),
                    };

                    self.add_instruction(context, alloc_instr);

                    // Prepend instance pointer as first argument to constructor
                    mir_arguments.push(MirOperand::Value(alloc_result));
                }

                // NOTE: For print/printl function calls, convert all arguments to strings
                // Check if this is a print/printl call BY NAME, not just SymbolId
                // SymbolId(0) is also used as a placeholder for namespace functions like math.sqrt
                let is_print_call = matches!(
                    function_name_opt.as_deref(),
                    Some("print") | Some("printl") | Some("println")
                );

                // Look up function parameter types for boxing any-typed parameters
                // Check both all_functions and class constructors
                let param_types: Vec<ConcreteType> = context
                    .all_functions
                    .iter()
                    .find(|f| f.symbol_id == function_symbol_id)
                    .map(|f| f.parameters.iter().map(|p| p.param_type.clone()).collect())
                    .or_else(|| {
                        // Search in class constructors
                        context.all_classes.iter().find_map(|class| {
                            class
                                .constructors
                                .iter()
                                .find(|c| c.symbol_id == function_symbol_id)
                                .map(|c| {
                                    c.parameters.iter().map(|p| p.param_type.clone()).collect()
                                })
                        })
                    })
                    .unwrap_or_default();

                // Add user-provided arguments
                for (arg_idx, arg) in arguments.iter().enumerate() {
                    let arg_id = self.build_expression(context, arg)?;

                    // For print calls, convert arguments to strings
                    let final_arg_id = if is_print_call {
                        self.convert_value_to_string(
                            context,
                            arg_id,
                            &arg.expr_type,
                            &arg.location,
                        )?
                    } else {
                        // Check if parameter type is Any and argument type is not Any
                        // If so, we need to box the value
                        let needs_boxing = if let Some(param_type) = param_types.get(arg_idx) {
                            matches!(param_type, ConcreteType::Any)
                                && !matches!(arg.expr_type, ConcreteType::Any)
                        } else {
                            false
                        };

                        if needs_boxing {
                            trace!(
                                arg_idx = arg_idx,
                                arg_type = ?arg.expr_type,
                                "Boxing argument to any type"
                            );
                            self.emit_box_any(context, arg_id, &arg.expr_type, &arg.location)
                        } else {
                            arg_id
                        }
                    };

                    mir_arguments.push(MirOperand::Value(final_arg_id));
                }

                // Fill in default parameters for missing arguments
                // NOTE: Skip default parameter handling for builtin/stdlib functions
                // Stdlib functions don't have default parameters and aren't in the all_functions list
                // Check if this function is actually in all_functions before trying to look it up
                let function_in_all_functions = context
                    .all_functions
                    .iter()
                    .any(|f| f.symbol_id == function_symbol_id);

                // Look up the function definition and clone the default values to avoid borrow conflicts
                let default_params: Vec<(String, Option<TastExpression>)> =
                    if !function_in_all_functions {
                        // Stdlib/builtin functions don't have default parameters, skip lookup
                        Vec::new()
                    } else {
                        let function_def = context
                            .all_functions
                            .iter()
                            .find(|f| f.symbol_id == function_symbol_id)
                            .expect("Function must exist - we just checked with any()");

                        // Clone the default values for parameters beyond what was provided
                        function_def
                            .parameters
                            .iter()
                            .skip(arguments.len())
                            .map(|p| (p.name.clone(), p.default_value.clone()))
                            .collect()
                    };

                // For each missing parameter, check if it has a default value
                for (param_name, default_value_opt) in default_params {
                    if let Some(default_expr) = default_value_opt {
                        // Build the default value expression
                        let default_id = self.build_expression(context, &default_expr)?;
                        mir_arguments.push(MirOperand::Value(default_id));
                    } else {
                        return Err(vec![CompilerError::validation_error(
                            format!("Missing required argument for parameter '{}'", param_name),
                            expression.location.clone(),
                        )]);
                    }
                }

                // Always allocate a result ValueId for consistency in MIR SSA form
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // NOTE: Infer return type for namespace functions like list.get
                // When called with Any type argument, the return should also be Any
                let inferred_type = if let Some(ref func_name) = function_name_opt {
                    if func_name == "list.get" || func_name == "list_get" {
                        // Check if first argument is Any type
                        if !arguments.is_empty()
                            && matches!(arguments[0].expr_type, ConcreteType::Any)
                        {
                            // list.get on Any returns Any
                            ConcreteType::Any
                        } else if !arguments.is_empty() {
                            // For Array<T>, extract element type
                            if let ConcreteType::Array(element_type) = &arguments[0].expr_type {
                                element_type.as_ref().clone()
                            } else {
                                expression.expr_type.clone()
                            }
                        } else {
                            expression.expr_type.clone()
                        }
                    } else if func_name == "list.size" || func_name == "list.length" {
                        // list.size always returns Integer
                        ConcreteType::Integer
                    } else if func_name.starts_with("math.") {
                        // NOTE: All math namespace functions return Number (f64)
                        ConcreteType::Number
                    } else if func_name.ends_with(".toNumber") {
                        // NOTE: All .toNumber() methods return Number (f64)
                        // This includes string.toNumber, integer.toNumber, boolean.toNumber, number.toNumber
                        ConcreteType::Number
                    } else if func_name.ends_with(".toInteger") {
                        // NOTE: All .toInteger() methods return Integer (i32)
                        // This includes string.toInteger, number.toInteger
                        ConcreteType::Integer
                    } else if func_name.ends_with(".toBoolean") {
                        // NOTE: All .toBoolean() methods return Boolean (i32)
                        ConcreteType::Boolean
                    } else if func_name.ends_with(".determinant") {
                        // Matrix.determinant() returns Number (f64)
                        ConcreteType::Number
                    } else if func_name.ends_with(".toString") {
                        // All .toString() methods return String
                        ConcreteType::String
                    } else {
                        expression.expr_type.clone()
                    }
                } else {
                    expression.expr_type.clone()
                };

                // Convert the expression type to MIR type
                let result_type = self.convert_concrete_type(&inferred_type);

                // NOTE: Check if this is a void function
                // Void functions have Null or Undefined types, which convert to void-related MIR types
                // Also check for known void functions by name (builtin functions that return nothing)
                trace!(
                    function_symbol_id = ?function_symbol_id,
                    function_name = ?function_name_opt,
                    expr_type = ?expression.expr_type,
                    result_type = ?result_type,
                    "Checking if void function"
                );

                // Check for known void functions by name
                // These are builtin/stdlib functions that return nothing (modify in-place or have side effects only)
                // NOTE: list.push is NOT void - it returns the list for chaining
                let is_known_void_function = matches!(
                    function_name_opt.as_deref(),
                    Some("list.set")
                        | Some("list.clear")
                        | Some("print")
                        | Some("printl")
                        | Some("println")
                        | Some("mem_release")
                        | Some("mem_retain")
                );

                // NOTE: Do NOT treat Ptr(Void) as void!
                // Ptr(Void) means "unknown pointer type" which happens when type inference
                // doesn't know the return type (e.g., namespace functions like string.split).
                // These functions DO return values (pointers), they just have unknown types.
                // Only treat actual MirType::Void (not Ptr(Void)) as void.
                let is_void = is_known_void_function
                    || matches!(inferred_type, ConcreteType::Null | ConcreteType::Undefined)
                    || matches!(result_type, MirType::Void);
                trace!(
                    is_void = is_void,
                    is_known_void_function = is_known_void_function,
                    "Is void result"
                );

                // ALWAYS register the local to maintain SSA invariant (learned from Context7)
                // This ensures every ValueId has a corresponding entry in the locals map
                self.register_temp_local(
                    context,
                    result_id,
                    result_type,
                    expression.location.clone(),
                );

                // For void functions, set dest = None so codegen knows not to store the result
                let dest_opt = if is_void { None } else { Some(result_id) };

                // NOTE: For namespace functions (SymbolId(0)), create NamedFunction operand
                // so codegen can look up the function by name instead of symbol ID
                let function_operand = if function_symbol_id.0 == 0 {
                    // Get function name from the Variable expression
                    let function_name = match &function.kind {
                        TastExpressionKind::Variable { name, .. } => name.clone(),
                        _ => String::from("unknown"),
                    };
                    trace!(function_name = %function_name, "Creating NamedFunction for SymbolId(0)");
                    MirOperand::NamedFunction {
                        name: function_name,
                        symbol_id: function_symbol_id,
                    }
                } else {
                    MirOperand::Function(function_symbol_id)
                };

                let instruction = MirInstruction {
                    dest: dest_opt,
                    operation: MirOperation::Call {
                        function: function_operand,
                        arguments: mir_arguments,
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(result_id)
            }

            TastExpressionKind::MethodCall {
                receiver,
                method_name,
                method_symbol,
                arguments,
                type_args: _,
            } => {
                // Build the receiver (object) first
                let receiver_id = self.build_expression(context, receiver)?;

                // NOTE: Get the ACTUAL type of the receiver
                // Priority: 1) Use TAST type if not Unknown, 2) Infer from locals map, 3) Use Unknown
                let receiver_actual_type = if !matches!(receiver.expr_type, ConcreteType::Unknown) {
                    // TAST has the type - use it
                    receiver.expr_type.clone()
                } else {
                    // TAST type is Unknown - try to infer from locals map
                    context
                        .function
                        .locals
                        .get(&receiver_id)
                        .map(|mir_local| Self::mir_type_to_concrete(&mir_local.local_type))
                        .unwrap_or(ConcreteType::Unknown)
                };

                trace!(
                    method_name = %method_name,
                    receiver_id = ?receiver_id,
                    tast_type = ?receiver.expr_type,
                    actual_type = ?receiver_actual_type,
                    "Method call receiver"
                );

                // SPECIAL CASE: String.toString() is identity operation - just return the receiver
                if method_symbol.0 == 0
                    && matches!(&receiver.expr_type, ConcreteType::String)
                    && method_name == "toString"
                {
                    return Ok(receiver_id);
                }

                // NOTE: Handle Any.toString() EARLY, regardless of method_symbol
                // For chained calls like c.get().toString() where c.get() returns Any,
                // the method_symbol might be non-zero, but we need to use AnyToString operation
                // NOTE: Must use receiver_actual_type, not receiver.expr_type, because TAST might have Unknown
                if matches!(&receiver_actual_type, ConcreteType::Any) && method_name == "toString" {
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Register the result as Ptr(U8) to distinguish string pointers from Any
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::Ptr(Box::new(MirType::U8)),
                        expression.location.clone(),
                    );

                    // Use the special AnyToString operation that does runtime type dispatch
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::AnyToString {
                            value: MirOperand::Value(receiver_id),
                        },
                        location: expression.location.clone(),
                    };

                    self.add_instruction(context, instruction);

                    trace!(
                        result_id = ?result_id,
                        receiver_id = ?receiver_id,
                        "Any.toString() with AnyToString operation (early detection)"
                    );

                    return Ok(result_id);
                }

                // NOTE: Handle Any.toInteger() EARLY for chained calls
                // For chained calls like c.get().toInteger() where c.get() returns Any,
                // the method_symbol might be non-zero, but we need to use UnboxAnyToI32 operation
                if matches!(&receiver_actual_type, ConcreteType::Any) && method_name == "toInteger"
                {
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Register the result as i32
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::I32,
                        expression.location.clone(),
                    );

                    // Use UnboxAnyToI32 to extract the integer value
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::UnboxAnyToI32 {
                            value: MirOperand::Value(receiver_id),
                        },
                        location: expression.location.clone(),
                    };

                    self.add_instruction(context, instruction);

                    trace!(
                        result_id = ?result_id,
                        receiver_id = ?receiver_id,
                        "Any.toInteger() with UnboxAnyToI32 operation (early detection)"
                    );

                    return Ok(result_id);
                }

                // Handle Any.toBoolean() EARLY for chained calls like p.bool.toBoolean()
                // The boxed Any value has type tag at offset 0: tag 1=false, tag 2=true.
                // We use UnboxAnyToBoolean to correctly read the tag and return 0 or 1.
                if matches!(&receiver_actual_type, ConcreteType::Any) && method_name == "toBoolean"
                {
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::I32,
                        expression.location.clone(),
                    );

                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::UnboxAnyToBoolean {
                            value: MirOperand::Value(receiver_id),
                        },
                        location: expression.location.clone(),
                    };

                    self.add_instruction(context, instruction);

                    trace!(
                        result_id = ?result_id,
                        receiver_id = ?receiver_id,
                        "Any.toBoolean() with UnboxAnyToBoolean operation (early detection)"
                    );

                    return Ok(result_id);
                }

                // NOTE: Handle Array/List methods FIRST, regardless of method_symbol
                // These methods have non-zero method_symbol (e.g., 103) but need special handling
                // to use the correct stdlib function indices
                if let ConcreteType::Array(element_type) = &receiver.expr_type {
                    match method_name.as_str() {
                        "add" => {
                            // CRITICAL: list.add modifies IN-PLACE and returns same list pointer
                            // Use SymbolId(1007) for i32, SymbolId(1008) for f64
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // List pointer
                                expression.location.clone(),
                            );

                            let add_symbol = match element_type.as_ref() {
                                ConcreteType::Number => SymbolId(1008), // list.add_f64 (in-place)
                                _ => SymbolId(1007),                    // list.add (in-place)
                            };

                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(add_symbol),
                                    arguments: args,
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        "push" => {
                            // list.push is an alias for list.add (in-place append)
                            // Routes to the same in-place add operation
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // List pointer
                                expression.location.clone(),
                            );

                            let add_symbol = match element_type.as_ref() {
                                ConcreteType::Number => SymbolId(1008), // list.add_f64 (in-place)
                                _ => SymbolId(1007),                    // list.add (in-place)
                            };

                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(add_symbol),
                                    arguments: args,
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        "size" | "length" => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // Size as integer
                                expression.location.clone(),
                            );

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(SymbolId(1006)), // list.size
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        _ => {} // Fall through for other methods
                    }
                }

                // NOTE: Handle String.matches EARLY — pattern name is resolved to a compile-time
                // integer ID so the host bridge receives (str_ptr, str_len, pattern_id: i32).
                // ID mapping matches server v1.9.27+: email=0 url=1 uuid=2 phone=3 date=4
                //   integer=5 number=6 alphanumeric=7
                if matches!(&receiver.expr_type, ConcreteType::String) && method_name == "matches" {
                    use crate::typechecker::tast::{TastExpressionKind, TastLiteral};

                    let pattern_id: i64 = if let Some(arg) = arguments.first() {
                        match &arg.kind {
                            TastExpressionKind::Literal {
                                value: TastLiteral::String(pattern),
                            } => match pattern.as_str() {
                                "email" => 0,
                                "url" => 1,
                                "uuid" => 2,
                                "phone" => 3,
                                "date" => 4,
                                "integer" => 5,
                                "number" => 6,
                                "alphanumeric" => 7,
                                other => {
                                    return Err(vec![crate::error::CompilerError::semantic_error(
                                        format!(
                                            "string.matches(): unknown pattern '{}'. \
                                             Valid: email, url, uuid, phone, date, integer, number, alphanumeric",
                                            other
                                        ),
                                        None,
                                        Some(expression.location.clone()),
                                    )]);
                                }
                            },
                            _ => {
                                return Err(vec![crate::error::CompilerError::semantic_error(
                                    "string.matches(): pattern argument must be a string literal",
                                    None,
                                    Some(expression.location.clone()),
                                )]);
                            }
                        }
                    } else {
                        return Err(vec![crate::error::CompilerError::semantic_error(
                            "string.matches() requires a pattern name argument",
                            None,
                            Some(expression.location.clone()),
                        )]);
                    };

                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::I32,
                        expression.location.clone(),
                    );

                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::NamedFunction {
                                name: "string.matches".to_string(),
                                symbol_id: SymbolId(0),
                            },
                            arguments: vec![
                                MirOperand::Value(receiver_id),
                                MirOperand::Constant(MirConstant::Integer(pattern_id)),
                            ],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    return Ok(result_id);
                }

                // NOTE: Handle String.indexOf methods EARLY, regardless of method_symbol
                // indexOf has a non-zero method_symbol (e.g., 71) but needs special handling
                // to dispatch to the correct function based on argument count
                if matches!(&receiver.expr_type, ConcreteType::String) && method_name == "indexOf" {
                    // indexOf has two variants:
                    // - 1 arg: str.indexOf(needle) -> call string.indexOf (2 params)
                    // - 2 args: str.indexOf(needle, startIndex) -> call string.indexOfFrom (3 params)
                    let mut args = vec![MirOperand::Value(receiver_id)];
                    for arg in arguments {
                        let arg_id = self.build_expression(context, arg)?;
                        args.push(MirOperand::Value(arg_id));
                    }

                    // Use NamedFunction to call the correct function based on arg count
                    let func_name = if arguments.len() == 2 {
                        "string.indexOfFrom".to_string()
                    } else {
                        "string.indexOf".to_string()
                    };

                    // Allocate result
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::I32, // Integer result
                        expression.location.clone(),
                    );

                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::NamedFunction {
                                name: func_name.clone(),
                                symbol_id: SymbolId(0), // Namespace function
                            },
                            arguments: args,
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    return Ok(result_id);
                }

                // NOTE: Handle String.lastIndexOf methods EARLY, regardless of method_symbol
                if matches!(&receiver.expr_type, ConcreteType::String)
                    && method_name == "lastIndexOf"
                {
                    // lastIndexOf has two variants:
                    // - 1 arg: str.lastIndexOf(needle) -> call string.lastIndexOf (2 params)
                    // - 2 args: str.lastIndexOf(needle, startIndex) -> call string.lastIndexOfFrom (3 params)
                    let mut args = vec![MirOperand::Value(receiver_id)];
                    for arg in arguments {
                        let arg_id = self.build_expression(context, arg)?;
                        args.push(MirOperand::Value(arg_id));
                    }

                    // Use NamedFunction to call the correct function based on arg count
                    let func_name = if arguments.len() == 2 {
                        "string.lastIndexOfFrom".to_string()
                    } else {
                        "string.lastIndexOf".to_string()
                    };

                    // Allocate result
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::I32, // Integer result
                        expression.location.clone(),
                    );

                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::NamedFunction {
                                name: func_name.clone(),
                                symbol_id: SymbolId(0), // Namespace function
                            },
                            arguments: args,
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    return Ok(result_id);
                }

                // NOTE: Handle String.substring methods EARLY, regardless of method_symbol
                // substring has two variants:
                // - 1 arg: str.substring(start) -> call string.substring(str, start, str.length)
                // - 2 args: str.substring(start, end) -> call string.substring(str, start, end)
                // NOTE: Must use receiver_actual_type, not receiver.expr_type, because TAST might have Unknown
                if matches!(&receiver_actual_type, ConcreteType::String)
                    && method_name == "substring"
                {
                    let mut args = vec![MirOperand::Value(receiver_id)];

                    // Build start argument
                    if !arguments.is_empty() {
                        let start_id = self.build_expression(context, &arguments[0])?;
                        args.push(MirOperand::Value(start_id));
                    }

                    // Build end argument
                    if arguments.len() >= 2 {
                        // 2-arg: use the provided end
                        let end_id = self.build_expression(context, &arguments[1])?;
                        args.push(MirOperand::Value(end_id));
                    } else {
                        // 1-arg: end = string.length
                        // Call string.length to get the end value
                        let length_result_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;
                        self.register_temp_local(
                            context,
                            length_result_id,
                            MirType::I32,
                            expression.location.clone(),
                        );

                        let length_instruction = MirInstruction {
                            dest: Some(length_result_id),
                            operation: MirOperation::Call {
                                function: MirOperand::NamedFunction {
                                    name: "string.length".to_string(),
                                    symbol_id: SymbolId(0),
                                },
                                arguments: vec![MirOperand::Value(receiver_id)],
                            },
                            location: expression.location.clone(),
                        };
                        self.add_instruction(context, length_instruction);
                        args.push(MirOperand::Value(length_result_id));
                    }

                    // Allocate result
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::Ptr(Box::new(MirType::U8)), // String pointer
                        expression.location.clone(),
                    );

                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::NamedFunction {
                                name: "string.substring".to_string(),
                                symbol_id: SymbolId(0),
                            },
                            arguments: args,
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    return Ok(result_id);
                }

                // SPECIAL CASE: Type conversion methods - emit Cast instructions or builtin calls
                if method_symbol.0 == 0 {
                    let receiver_type = &receiver.expr_type;
                    match (receiver_type, method_name.as_str()) {
                        // Integer to String conversion - call int_to_string
                        (ConcreteType::Integer, "toString") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as Ptr(U8) to distinguish string pointers from integers
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::Ptr(Box::new(MirType::U8)),
                                expression.location.clone(),
                            );

                            let symbol_id = self
                                .symbol_table
                                .lookup_symbol("int_to_string")
                                .unwrap_or_else(|| {
                                    warn!("int_to_string not found in symbol table, using SymbolId(166)");
                                    SymbolId(166)
                                });

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(symbol_id),
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Number to String conversion - call float_to_string
                        (ConcreteType::Number, "toString") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as Ptr(U8) to distinguish string pointers from numbers
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::Ptr(Box::new(MirType::U8)),
                                expression.location.clone(),
                            );

                            let symbol_id = self
                                .symbol_table
                                .lookup_symbol("float_to_string")
                                .unwrap_or_else(|| {
                                    warn!("float_to_string not found in symbol table, using SymbolId(167)");
                                    SymbolId(167)
                                });

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(symbol_id),
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Boolean to String conversion - call bool_to_string
                        (ConcreteType::Boolean, "toString") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as Ptr(U8) to distinguish string pointers from booleans
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::Ptr(Box::new(MirType::U8)),
                                expression.location.clone(),
                            );

                            let symbol_id = self
                                .symbol_table
                                .lookup_symbol("bool_to_string")
                                .unwrap_or_else(|| {
                                    warn!("bool_to_string not found in symbol table, using SymbolId(165)");
                                    SymbolId(165)
                                });

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(symbol_id),
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Any to String conversion with proper type dispatch
                        // Uses the boxed any value's type tag to call the correct toString function
                        (ConcreteType::Any, "toString") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as Ptr(U8) to distinguish string pointers from Any
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::Ptr(Box::new(MirType::U8)),
                                expression.location.clone(),
                            );

                            // Use the special AnyToString operation that does runtime type dispatch
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::AnyToString {
                                    value: MirOperand::Value(receiver_id),
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Any to Integer conversion - unbox and convert to i32
                        // Handles both Integer (tag 1) and Number (tag 3) boxed values
                        (ConcreteType::Any, "toInteger") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as i32
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );

                            // Use UnboxAnyToI32 to extract the integer value
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::UnboxAnyToI32 {
                                    value: MirOperand::Value(receiver_id),
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Any to Number conversion - unbox and convert to f64
                        // Handles both Integer (tag 1) and Number (tag 3) boxed values
                        (ConcreteType::Any, "toNumber") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as f64
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::F64,
                                expression.location.clone(),
                            );

                            // Use UnboxAnyToF64 to extract the number value
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::UnboxAnyToF64 {
                                    value: MirOperand::Value(receiver_id),
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Number to Integer conversion (f64 -> i32)
                        (ConcreteType::Number, "toInteger") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as i32
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );

                            // Emit Cast instruction from F64 to I32
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Cast {
                                    value: MirOperand::Value(receiver_id),
                                    target_type: MirType::I32,
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Integer to Number conversion (i32 -> f64)
                        (ConcreteType::Integer, "toNumber") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as f64
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::F64,
                                expression.location.clone(),
                            );

                            // Emit Cast instruction from I32 to F64
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Cast {
                                    value: MirOperand::Value(receiver_id),
                                    target_type: MirType::F64,
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Integer to Boolean conversion (i32 -> i32, non-zero = true)
                        (ConcreteType::Integer, "toBoolean") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as i32 (booleans are i32 in WASM)
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );

                            // Emit comparison: receiver != 0
                            let zero_const = MirConstant::Integer(0);
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::BinaryOp {
                                    op: MirBinaryOp::Ne,
                                    left: MirOperand::Value(receiver_id),
                                    right: MirOperand::Constant(zero_const),
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        _ => {
                            // Not a type conversion method, continue to built-in method handling
                        }
                    }
                }

                // Check if this is a built-in method that should be mapped to a conversion function
                // SymbolId(0) is used as a placeholder for built-in methods that don't have real symbols
                let (function_symbol, mir_arguments) = if method_symbol.0 == 0 {
                    // This is a built-in method - determine the correct function based on receiver type and method name
                    // NOTE: Use actual_type instead of TAST expr_type to handle inferred types correctly
                    let receiver_type = &receiver_actual_type;
                    match (receiver_type, method_name.as_str()) {
                        // Type conversion methods - look up correct SymbolIds from symbol table
                        (ConcreteType::Integer, "toString") => {
                            // Call int_to_string with the integer value
                            let symbol_id = self.symbol_table.lookup_symbol("int_to_string")
                                .unwrap_or_else(|| {
                                    warn!("int_to_string not found in symbol table, using SymbolId(166)");
                                    SymbolId(166)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Number, "toString") => {
                            // Call float_to_string with the float value
                            let symbol_id = self.symbol_table.lookup_symbol("float_to_string")
                                .unwrap_or_else(|| {
                                    warn!("float_to_string not found in symbol table, using SymbolId(167)");
                                    SymbolId(167)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Boolean, "toString") => {
                            // Call bool_to_string with the boolean value
                            let symbol_id = self.symbol_table.lookup_symbol("bool_to_string")
                                .unwrap_or_else(|| {
                                    warn!("bool_to_string not found in symbol table, using SymbolId(165)");
                                    SymbolId(165)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        // Generic type with toString() - infer from MIR type
                        (ConcreteType::Generic { .. } | ConcreteType::Unknown, "toString") => {
                            // Check the MIR type to determine which conversion function to use
                            let mir_type = context
                                .function
                                .locals
                                .get(&receiver_id)
                                .map(|l| l.local_type.clone())
                                .unwrap_or(MirType::I32);

                            trace!(
                                receiver_id = ?receiver_id,
                                mir_type = ?mir_type,
                                "Generic toString"
                            );

                            match mir_type {
                                MirType::I32 => {
                                    // Call int_to_string
                                    let symbol_id = self.symbol_table.lookup_symbol("int_to_string")
                                        .unwrap_or_else(|| {
                                            warn!("int_to_string not found in symbol table, using SymbolId(166)");
                                            SymbolId(166)
                                        });
                                    (symbol_id, vec![MirOperand::Value(receiver_id)])
                                }
                                MirType::F64 => {
                                    // Call float_to_string
                                    let symbol_id = self.symbol_table.lookup_symbol("float_to_string")
                                        .unwrap_or_else(|| {
                                            warn!("float_to_string not found in symbol table, using SymbolId(167)");
                                            SymbolId(167)
                                        });
                                    (symbol_id, vec![MirOperand::Value(receiver_id)])
                                }
                                _ => {
                                    // Assume it's already a string or object with built-in toString
                                    warn!(mir_type = ?mir_type, "Unknown MIR type for Generic.toString(), treating as string");
                                    (*method_symbol, vec![MirOperand::Value(receiver_id)])
                                }
                            }
                        }
                        // String methods - look up correct SymbolIds from symbol table
                        (ConcreteType::String, "length") => {
                            // Call string.length with the string value
                            let symbol_id = self.symbol_table.lookup_symbol("string.length")
                                .unwrap_or_else(|| {
                                    warn!("string.length not found in symbol table, using SymbolId(67)");
                                    SymbolId(67)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "toUpperCase") => {
                            // Call string.toUpperCase
                            let symbol_id = self.symbol_table.lookup_symbol("string.toUpperCase")
                                .unwrap_or_else(|| {
                                    warn!("string.toUpperCase not found in symbol table, using SymbolId(74)");
                                    SymbolId(74)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "toLowerCase") => {
                            // Call string.toLowerCase
                            let symbol_id = self.symbol_table.lookup_symbol("string.toLowerCase")
                                .unwrap_or_else(|| {
                                    warn!("string.toLowerCase not found in symbol table, using SymbolId(75)");
                                    SymbolId(75)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "substring") => {
                            // Call string_substring (SymbolId 49)
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (SymbolId(49), args)
                        }
                        (ConcreteType::String, "contains") => {
                            // Call string_contains (SymbolId 52)
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (SymbolId(52), args)
                        }
                        (ConcreteType::String, "indexOf") => {
                            // indexOf has two variants:
                            // - 1 arg: str.indexOf(needle) -> call string.indexOf (2 params)
                            // - 2 args: str.indexOf(needle, startIndex) -> call string.indexOfFrom (3 params)
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }

                            // Use NamedFunction to call the correct function based on arg count
                            let func_name = if arguments.len() == 2 {
                                "string.indexOfFrom".to_string()
                            } else {
                                "string.indexOf".to_string()
                            };

                            // Allocate result
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // Integer result
                                expression.location.clone(),
                            );

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::NamedFunction {
                                        name: func_name.clone(),
                                        symbol_id: SymbolId(0), // Namespace function
                                    },
                                    arguments: args,
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        (ConcreteType::String, "lastIndexOf") => {
                            // lastIndexOf has two variants:
                            // - 1 arg: str.lastIndexOf(needle) -> call string.lastIndexOf (2 params)
                            // - 2 args: str.lastIndexOf(needle, startIndex) -> call string.lastIndexOfFrom (3 params)
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }

                            // Use NamedFunction to call the correct function based on arg count
                            let func_name = if arguments.len() == 2 {
                                "string.lastIndexOfFrom".to_string()
                            } else {
                                "string.lastIndexOf".to_string()
                            };

                            // Allocate result
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // Integer result
                                expression.location.clone(),
                            );

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::NamedFunction {
                                        name: func_name.clone(),
                                        symbol_id: SymbolId(0), // Namespace function
                                    },
                                    arguments: args,
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        // Array/List methods
                        (ConcreteType::Array(_), "size" | "length") => {
                            // NOTE: Use synthetic SymbolId(1006) for list.size
                            // Symbol table lookup returns wrong function index
                            (SymbolId(1006), vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Array(element_type), "add") => {
                            // CRITICAL: list.add modifies IN-PLACE - use SymbolId(1007/1008)
                            let list_add_symbol = match element_type.as_ref() {
                                ConcreteType::Number => SymbolId(1008), // list.add_f64 (in-place)
                                _ => SymbolId(1007),                    // list.add (in-place)
                            };
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (list_add_symbol, args)
                        }
                        (ConcreteType::Array(element_type), "push") => {
                            // list.push is an alias for list.add (in-place append)
                            let list_add_symbol = match element_type.as_ref() {
                                ConcreteType::Number => SymbolId(1008), // list.add_f64 (in-place)
                                _ => SymbolId(1007),                    // list.add (in-place)
                            };
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (list_add_symbol, args)
                        }
                        (ConcreteType::Array(_), "setFlags") => {
                            // Store compile-time-known behavior flags at offset 12.
                            let set_flags_symbol = self
                                .symbol_table
                                .lookup_symbol("list.setFlags")
                                .unwrap_or_else(|| {
                                    warn!(
                                        "list.setFlags not found in symbol table, using fallback"
                                    );
                                    *method_symbol
                                });
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (set_flags_symbol, args)
                        }
                        (ConcreteType::Array(_), "remove" | "pop" | "removeLast") => {
                            // Use the behavior-aware list.remove so that LINE (FIFO) and
                            // PILE (LIFO) flags are respected at runtime.
                            let list_remove_symbol = self
                                .symbol_table
                                .lookup_symbol("list.remove")
                                .unwrap_or_else(|| {
                                    warn!("list.remove not found in symbol table, using fallback");
                                    *method_symbol
                                });
                            (list_remove_symbol, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Array(_), "get") => {
                            // Call list_get - look up from symbol table
                            let list_get_symbol = self
                                .symbol_table
                                .lookup_symbol("list_get")
                                .unwrap_or_else(|| {
                                    warn!("list_get not found in symbol table, using fallback");
                                    *method_symbol
                                });
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (list_get_symbol, args)
                        }
                        // NOTE: Handle Any.get() method for JSON field access
                        // This must generate AnyGetField or AnyGetIndex based on argument type
                        (ConcreteType::Any, "get") => {
                            // Get the key/index argument
                            if arguments.len() != 1 {
                                return Err(vec![CompilerError::validation_error(
                                    format!(
                                        "any.get() expects 1 argument, got {}",
                                        arguments.len()
                                    ),
                                    expression.location.clone(),
                                )]);
                            }

                            let arg = &arguments[0];
                            let arg_id = self.build_expression(context, arg)?;

                            // Allocate result ValueId
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Result is also Any type
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::Any,
                                expression.location.clone(),
                            );

                            // Check argument type to determine operation
                            let operation = match &arg.expr_type {
                                ConcreteType::Integer | ConcreteType::Number => {
                                    // Integer/Number index: use AnyGetIndex for array access
                                    MirOperation::AnyGetIndex {
                                        array: MirOperand::Value(receiver_id),
                                        index: MirOperand::Value(arg_id),
                                    }
                                }
                                _ => {
                                    // String or other type: use AnyGetField for object field access
                                    MirOperation::AnyGetField {
                                        object: MirOperand::Value(receiver_id),
                                        key: MirOperand::Value(arg_id),
                                    }
                                }
                            };

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation,
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            trace!(
                                result_id = ?result_id,
                                receiver_id = ?receiver_id,
                                arg_id = ?arg_id,
                                arg_type = ?arg.expr_type,
                                "Any.get() dispatched based on argument type"
                            );

                            return Ok(result_id);
                        }
                        // NOTE: Handle any type's isDefined/isNotDefined/isEmpty/isNotEmpty methods
                        // These were falling through to the default case which used SymbolId(0) (print function)
                        // Use NamedFunction since these are stdlib functions registered by name, not in symbol table
                        (ConcreteType::Any, "isDefined") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // Boolean
                                expression.location.clone(),
                            );
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::NamedFunction {
                                        name: "value.isDefined".to_string(),
                                        symbol_id: SymbolId(0),
                                    },
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        (ConcreteType::Any, "isNotDefined") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // Boolean
                                expression.location.clone(),
                            );
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::NamedFunction {
                                        name: "value.isNotDefined".to_string(),
                                        symbol_id: SymbolId(0),
                                    },
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        (ConcreteType::Any, "isEmpty") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // Boolean
                                expression.location.clone(),
                            );
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::NamedFunction {
                                        name: "value.isEmpty".to_string(),
                                        symbol_id: SymbolId(0),
                                    },
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        (ConcreteType::Any, "isNotEmpty") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // Boolean
                                expression.location.clone(),
                            );
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::NamedFunction {
                                        name: "value.isNotEmpty".to_string(),
                                        symbol_id: SymbolId(0),
                                    },
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        // For other built-in methods, fall back to treating as instance method
                        _ => {
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (*method_symbol, args)
                        }
                    }
                } else {
                    // This is a user-defined method - receiver becomes first argument
                    let mut args = vec![MirOperand::Value(receiver_id)];

                    // Look up method parameter types from the class
                    // First, get the class symbol from the receiver type
                    let method_param_types: Vec<ConcreteType> = if let ConcreteType::Class {
                        symbol_id: class_symbol,
                        ..
                    } = &receiver.expr_type
                    {
                        // Find the class and then the method
                        context.all_classes.iter()
                            .find(|c| c.symbol_id == *class_symbol)
                            .and_then(|class| {
                                trace!(
                                    class_name = %class.name,
                                    method_symbol = ?method_symbol,
                                    method_count = %class.methods.len(),
                                    "Looking for method in class"
                                );
                                class.methods.iter()
                                    .find(|m| m.symbol_id == *method_symbol)
                                    .map(|method| {
                                        trace!(
                                            method_name = %method.name,
                                            param_count = %method.parameters.len(),
                                            params = ?method.parameters.iter().map(|p| &p.param_type).collect::<Vec<_>>(),
                                            "Found method with parameters"
                                        );
                                        // NOTE: TastFunction.parameters does NOT include 'this' for methods
                                        // so we don't need to skip anything
                                        method.parameters.iter()
                                            .map(|p| p.param_type.clone())
                                            .collect()
                                    })
                            })
                            .unwrap_or_default()
                    } else {
                        trace!(
                            receiver_type = ?receiver.expr_type,
                            "Receiver is not a Class type"
                        );
                        Vec::new()
                    };

                    trace!(
                        method_name = %method_name,
                        method_param_types = ?method_param_types,
                        arg_count = %arguments.len(),
                        "Method call parameter types"
                    );

                    // Process arguments with boxing for 'any' parameters
                    for (i, arg) in arguments.iter().enumerate() {
                        let arg_id = self.build_expression(context, arg)?;

                        // Check if parameter expects 'any' but argument is not 'any'
                        let final_arg_id = if i < method_param_types.len() {
                            let param_type = &method_param_types[i];
                            if matches!(param_type, ConcreteType::Any)
                                && !matches!(arg.expr_type, ConcreteType::Any)
                            {
                                // Box the argument
                                self.emit_box_any(context, arg_id, &arg.expr_type, &arg.location)
                            } else {
                                arg_id
                            }
                        } else {
                            arg_id
                        };

                        args.push(MirOperand::Value(final_arg_id));
                    }
                    (*method_symbol, args)
                };

                // Always allocate a result ValueId for consistency in MIR SSA form
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                trace!(
                    result_id = ?result_id,
                    method_name = %method_name,
                    "Method call allocated"
                );

                // NOTE: For list.get and toString, infer return type from receiver instead of using Unknown
                // The typechecker returns Unknown for these methods, but we can infer from the receiver type
                let inferred_type = if method_name == "get" {
                    if let ConcreteType::Array(element_type) = &receiver.expr_type {
                        // Extract element type from Array<T> -> T
                        element_type.as_ref().clone()
                    } else if matches!(&receiver.expr_type, ConcreteType::Any) {
                        // NOTE: When calling .get() on Any type (e.g., JSON data),
                        // the result is also Any since we don't know the element type
                        ConcreteType::Any
                    } else if let ConcreteType::Class {
                        symbol_id: class_symbol,
                        ..
                    } = &receiver.expr_type
                    {
                        // For user-defined classes, look up the method return type
                        context
                            .all_classes
                            .iter()
                            .find(|c| c.symbol_id == *class_symbol)
                            .and_then(|class| {
                                class
                                    .methods
                                    .iter()
                                    .find(|m| m.name == *method_name)
                                    .map(|method| method.return_type.clone())
                            })
                            .unwrap_or_else(|| expression.expr_type.clone())
                    } else {
                        expression.expr_type.clone()
                    }
                } else if method_name == "toString" {
                    // toString always returns String
                    ConcreteType::String
                } else if matches!(&receiver.expr_type, ConcreteType::String) {
                    // NOTE: String methods have known return types
                    // Don't rely on expression.expr_type which may be Unknown
                    match method_name.as_str() {
                        // Methods that return String
                        "substring" | "trim" | "trimStart" | "trimEnd" | "toUpperCase"
                        | "toLowerCase" | "replace" | "replaceAll" | "padStart" | "padEnd"
                        | "charAt" | "concat" | "split" | "repeat" => ConcreteType::String,
                        // Methods that return Integer
                        "length" | "size" | "indexOf" | "lastIndexOf" | "charCodeAt"
                        | "toInteger" => ConcreteType::Integer,
                        // Methods that return Boolean
                        "contains" | "startsWith" | "endsWith" | "isEmpty" | "isBlank"
                        | "toBoolean" | "matches" => ConcreteType::Boolean,
                        // Type conversion methods that return Number
                        "toNumber" => ConcreteType::Number,
                        // Default to expression type for unknown methods
                        _ => expression.expr_type.clone(),
                    }
                } else if matches!(&receiver.expr_type, ConcreteType::Integer) {
                    // NOTE: Integer methods have known return types
                    match method_name.as_str() {
                        "toString" => ConcreteType::String,
                        "toNumber" => ConcreteType::Number,
                        "toBoolean" => ConcreteType::Boolean,
                        _ => expression.expr_type.clone(),
                    }
                } else if matches!(&receiver.expr_type, ConcreteType::Number) {
                    // NOTE: Number methods have known return types
                    match method_name.as_str() {
                        "toString" => ConcreteType::String,
                        "toInteger" => ConcreteType::Integer,
                        "toBoolean" => ConcreteType::Boolean,
                        "toNumber" => ConcreteType::Number, // identity
                        _ => expression.expr_type.clone(),
                    }
                } else if matches!(&receiver.expr_type, ConcreteType::Boolean) {
                    // NOTE: Boolean methods have known return types
                    match method_name.as_str() {
                        "toString" => ConcreteType::String,
                        "toNumber" => ConcreteType::Number,
                        "toInteger" => ConcreteType::Integer,
                        _ => expression.expr_type.clone(),
                    }
                } else if matches!(&receiver.expr_type, ConcreteType::Array(_)) {
                    // NOTE: Array/List methods with known void return type
                    match method_name.as_str() {
                        "setFlags" => ConcreteType::Undefined,
                        _ => expression.expr_type.clone(),
                    }
                } else if matches!(&receiver.expr_type, ConcreteType::Matrix(_)) {
                    // NOTE: Matrix methods have known return types
                    match method_name.as_str() {
                        "toString" => ConcreteType::String,
                        "determinant" => ConcreteType::Number,
                        "transpose" | "inverse" => receiver.expr_type.clone(),
                        "rows" | "cols" | "size" => ConcreteType::Integer,
                        _ => expression.expr_type.clone(),
                    }
                } else if let ConcreteType::Pairs(_, value_type) = &receiver.expr_type {
                    // NOTE: Pairs methods have known return types based on the value type
                    match method_name.as_str() {
                        "get" => (**value_type).clone(),
                        "has" => ConcreteType::Boolean,
                        "len" | "size" => ConcreteType::Integer,
                        "set" | "remove" => ConcreteType::Undefined,
                        _ => expression.expr_type.clone(),
                    }
                } else if let ConcreteType::Class {
                    symbol_id: class_symbol,
                    ..
                } = &receiver.expr_type
                {
                    // For other user-defined class methods, look up the return type
                    context
                        .all_classes
                        .iter()
                        .find(|c| c.symbol_id == *class_symbol)
                        .and_then(|class| {
                            class
                                .methods
                                .iter()
                                .find(|m| m.name == *method_name)
                                .map(|method| method.return_type.clone())
                        })
                        .unwrap_or_else(|| expression.expr_type.clone())
                } else {
                    expression.expr_type.clone()
                };

                // Convert the expression type to MIR type
                let result_type = self.convert_concrete_type(&inferred_type);

                // NOTE: Check if this is a void method
                // Unknown types should NOT be treated as void - they represent unresolved return types
                // that likely return values. Only explicitly Null/Undefined should be treated as void.
                let is_void = !matches!(inferred_type, ConcreteType::Unknown)
                    && (matches!(inferred_type, ConcreteType::Null | ConcreteType::Undefined)
                        || matches!(result_type, MirType::Void)
                        || matches!(&result_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void)));

                trace!(
                    result_id = ?result_id,
                    is_void = is_void,
                    inferred_type = ?inferred_type,
                    mir_type = ?result_type,
                    "Method call type check"
                );

                // ALWAYS register the local to maintain SSA invariant (learned from Context7)
                // This ensures every ValueId has a corresponding entry in the locals map
                self.register_temp_local(
                    context,
                    result_id,
                    result_type.clone(),
                    expression.location.clone(),
                );

                trace!(
                    result_id = ?result_id,
                    is_void = is_void,
                    "Method call registered in locals"
                );

                // For void methods, set dest = None so codegen knows not to store the result
                let dest_opt = if is_void { None } else { Some(result_id) };

                // NOTE: For namespace functions (string.*, list.*, etc), use NamedFunction operand
                // Check if this is a method call that should be converted to a namespace function call
                // NOTE: Must use receiver_actual_type, not receiver.expr_type, because TAST might have Unknown
                let function_operand = {
                    let receiver_type_name = match &receiver_actual_type {
                        ConcreteType::String => Some("string"),
                        ConcreteType::Array(_) => Some("list"),
                        ConcreteType::Integer => Some("integer"),
                        ConcreteType::Number => Some("number"),
                        ConcreteType::Boolean => Some("boolean"),
                        ConcreteType::Pairs(_, _) => Some("pairs"),
                        _ => None,
                    };

                    if let Some(type_name) = receiver_type_name {
                        // This is a method call on a known type - convert to namespace function
                        let namespace_function_name = format!("{}.{}", type_name, method_name);
                        trace!(
                            namespace_function_name = %namespace_function_name,
                            "Creating NamedFunction for method call"
                        );
                        MirOperand::NamedFunction {
                            name: namespace_function_name,
                            symbol_id: function_symbol,
                        }
                    } else {
                        // Regular function call with symbol ID
                        MirOperand::Function(function_symbol)
                    }
                };

                let instruction = MirInstruction {
                    dest: dest_opt,
                    operation: MirOperation::Call {
                        function: function_operand,
                        arguments: mir_arguments,
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(result_id)
            }

            TastExpressionKind::StaticMethodCall {
                class_name,
                method_name,
                method_symbol,
                arguments,
                type_args: _,
            } => {
                // Build all arguments (NO 'this' parameter for static methods!)
                let mut mir_arguments = Vec::new();
                for arg in arguments {
                    let arg_id = self.build_expression(context, arg)?;
                    mir_arguments.push(MirOperand::Value(arg_id));
                }

                // Create result
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Get return type from expression
                let mir_return_type = self.convert_concrete_type(&expression.expr_type);

                // Register result local
                self.register_temp_local(
                    context,
                    result_id,
                    mir_return_type.clone(),
                    expression.location.clone(),
                );

                // Check if this is a namespace function (like math.pow, string.length)
                // These use SymbolId(0) and need to be looked up by name
                trace!(
                    class_name = %class_name,
                    method_name = %method_name,
                    method_symbol = method_symbol.0,
                    "Static method call"
                );
                let function_operand = if method_symbol.0 == 0 {
                    // Namespace function - use NamedFunction pattern
                    let full_name = format!("{}.{}", class_name, method_name);
                    trace!(full_name = %full_name, "Creating NamedFunction for static call");
                    MirOperand::NamedFunction {
                        name: full_name,
                        symbol_id: *method_symbol,
                    }
                } else {
                    // Regular static method - use symbol ID directly
                    trace!(
                        method_symbol = method_symbol.0,
                        "Creating Function for static call"
                    );
                    MirOperand::Function(*method_symbol)
                };

                // Emit Call instruction - NO 'this' parameter prepended!
                let instruction = MirInstruction {
                    dest: Some(result_id),
                    operation: MirOperation::Call {
                        function: function_operand,
                        arguments: mir_arguments,
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(result_id)
            }

            TastExpressionKind::PropertyAccess {
                object,
                property_name,
                property_symbol,
            } => {
                // Build the object expression first
                let object_id = self.build_expression(context, object)?;

                // Use GetElementPtr for field access - this will be handled by codegen
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Add GetElementPtr result to locals
                let gep_local = MirLocal {
                    name: None,               // Temporary value
                    local_type: MirType::I32, // Pointer type
                    is_mutable: false,
                    location: expression.location.clone(),
                };
                context.function.locals.insert(result_id, gep_local);

                // BOOK: safe-access - Handle Any type property access using AnyGetField
                // For Any type (JSON/dynamic objects), use AnyGetField instead of GetElementPtr
                if matches!(object.expr_type, ConcreteType::Any) {
                    // Any type: generate AnyGetField operation
                    // Get string pool index for the property name
                    let string_index = self.get_string_index(property_name.clone());

                    // NOTE: Pass string constant directly to AnyGetField
                    // Do NOT create an intermediate I32 variable - this causes load_string_argument_for_print
                    // to mistakenly call int_to_string instead of expanding the string properly.
                    // Generate AnyGetField operation with constant key directly
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::AnyGetField {
                            object: MirOperand::Value(object_id),
                            key: MirOperand::Constant(MirConstant::String(string_index)),
                        },
                        location: expression.location.clone(),
                    };

                    // Update the result local type to Any
                    if let Some(local) = context.function.locals.get_mut(&result_id) {
                        local.local_type = MirType::Any;
                    }

                    self.add_instruction(context, instruction);
                    return Ok(result_id);
                }

                // NOTE: Get the class from the object's type, not from class_context
                // This allows field access from any context (e.g., start() function)
                let object_class_symbol = match &object.expr_type {
                    ConcreteType::Class { symbol_id, .. } => Some(*symbol_id),
                    _ => None,
                };

                // Find the BYTE OFFSET of the field in the class hierarchy
                // This is critical for correct memory layout with mixed-size fields (i32=4, f64=8)
                let field_byte_offset = if let Some(class_symbol) = object_class_symbol {
                    // Calculate the byte offset from the start of the object
                    self.calculate_field_byte_offset(context, class_symbol, property_symbol)
                        .ok_or_else(|| {
                            vec![CompilerError::validation_error(
                                format!(
                                    "Field '{}' not found in class or parent classes",
                                    property_name
                                ),
                                expression.location.clone(),
                            )]
                        })? as i64
                } else {
                    // Object doesn't have a class type - this shouldn't happen for field access
                    return Err(vec![CompilerError::validation_error(
                        format!(
                            "Cannot access field '{}' on non-class type: {:?}",
                            property_name, object.expr_type
                        ),
                        expression.location.clone(),
                    )]);
                };

                let field_offset = MirOperand::Constant(MirConstant::Integer(field_byte_offset));
                let instruction = MirInstruction {
                    dest: Some(result_id),
                    operation: MirOperation::GetElementPtr {
                        base: MirOperand::Value(object_id),
                        indices: vec![field_offset],
                        is_array: false, // Class field access - byte offset, no header
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);

                // Now load the value from the field pointer
                let load_result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Add Load result to locals
                // Use the actual field type from the expression instead of hardcoding I32
                let field_type = self.convert_concrete_type(&expression.expr_type);
                let load_local = MirLocal {
                    name: Some(format!("field_{}", property_name)),
                    local_type: field_type,
                    is_mutable: false,
                    location: expression.location.clone(),
                };
                context.function.locals.insert(load_result_id, load_local);

                let load_instruction = MirInstruction {
                    dest: Some(load_result_id),
                    operation: MirOperation::Load {
                        source: MirOperand::Value(result_id),
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, load_instruction);
                Ok(load_result_id)
            }

            TastExpressionKind::ArrayAccess { array, index } => {
                // Build array and index expressions
                let array_id = self.build_expression(context, array)?;
                let index_id = self.build_expression(context, index)?;

                // Check if this is Any type access (JSON object/array access)
                match &array.expr_type {
                    ConcreteType::Any => {
                        // For Any type, generate AnyGetField or AnyGetIndex based on index type
                        let result_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Result is also Any type (i32 pointer)
                        self.register_temp_local(
                            context,
                            result_id,
                            MirType::Any,
                            expression.location.clone(),
                        );

                        let operation = match &index.expr_type {
                            ConcreteType::String => {
                                // String key: object field access
                                MirOperation::AnyGetField {
                                    object: MirOperand::Value(array_id),
                                    key: MirOperand::Value(index_id),
                                }
                            }
                            ConcreteType::Integer => {
                                // Integer index: array element access
                                MirOperation::AnyGetIndex {
                                    array: MirOperand::Value(array_id),
                                    index: MirOperand::Value(index_id),
                                }
                            }
                            _ => {
                                // Fallback to integer index for other types
                                MirOperation::AnyGetIndex {
                                    array: MirOperand::Value(array_id),
                                    index: MirOperand::Value(index_id),
                                }
                            }
                        };

                        let instruction = MirInstruction {
                            dest: Some(result_id),
                            operation,
                            location: expression.location.clone(),
                        };

                        self.add_instruction(context, instruction);
                        Ok(result_id)
                    }

                    // Regular array/matrix access uses GetElementPtr
                    _ => {
                        // Use GetElementPtr for array access
                        let result_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // NOTE: Register the pointer result as a local
                        // GetElementPtr returns a pointer to the array element
                        self.register_temp_local(
                            context,
                            result_id,
                            MirType::Ptr(Box::new(MirType::I32)), // Pointer to element
                            expression.location.clone(),
                        );

                        let instruction = MirInstruction {
                            dest: Some(result_id),
                            operation: MirOperation::GetElementPtr {
                                base: MirOperand::Value(array_id),
                                indices: vec![MirOperand::Value(index_id)],
                                is_array: true, // Array access, needs 16-byte header offset
                            },
                            location: expression.location.clone(),
                        };

                        self.add_instruction(context, instruction);

                        // Load the value from the array element pointer
                        let load_result_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // NOTE: Register the loaded value as a local
                        // Determine the type from the array expression type
                        let element_type = match &array.expr_type {
                            ConcreteType::Array(elem_type) => self.convert_concrete_type(elem_type),
                            ConcreteType::Matrix(elem_type) => {
                                // Matrix is 2D array, so element is 1D array
                                MirType::Ptr(Box::new(self.convert_concrete_type(elem_type)))
                            }
                            _ => MirType::I32, // Default fallback
                        };

                        self.register_temp_local(
                            context,
                            load_result_id,
                            element_type,
                            expression.location.clone(),
                        );

                        let load_instruction = MirInstruction {
                            dest: Some(load_result_id),
                            operation: MirOperation::Load {
                                source: MirOperand::Value(result_id),
                            },
                            location: expression.location.clone(),
                        };

                        self.add_instruction(context, load_instruction);
                        Ok(load_result_id)
                    }
                }
            }

            TastExpressionKind::OnError {
                expression: expr,
                fallback: _,
            } => {
                // Evaluate main expression (fallback requires WASM exception handling)
                self.build_expression(context, expr)
            }

            TastExpressionKind::Conditional {
                condition: _,
                then_expr,
                else_expr: _,
            } => {
                // Evaluates then branch (full ternary requires control flow - handled in if/else codegen)
                self.build_expression(context, then_expr)
            }

            TastExpressionKind::BaseCall {
                parent_class_symbol_id,
                arguments,
            } => {
                // Base constructor call: base(args...)
                // This is called from a derived class constructor to initialize the base class fields
                // The derived class constructor has already allocated the instance, so we pass
                // the 'this' pointer as the first argument to the base constructor

                trace!(
                    parent_class_symbol_id = ?parent_class_symbol_id,
                    "Processing base() call to parent class"
                );

                // Build argument operands
                let mut mir_arguments = Vec::new();

                // Get 'this' from the first parameter of the current constructor
                // In class constructors, 'this' is always the first parameter
                let this_value_id = if let Some(first_param) = context.function.parameters.first() {
                    first_param.value_id
                } else {
                    return Err(vec![CompilerError::validation_error(
                        "base() call outside of constructor or constructor has no 'this' parameter",
                        expression.location.clone(),
                    )]);
                };

                trace!(this_value_id = ?this_value_id, "Got 'this' for base call");

                // Prepend 'this' as first argument to base constructor
                mir_arguments.push(MirOperand::Value(this_value_id));

                // Add user-provided arguments
                for (i, arg) in arguments.iter().enumerate() {
                    trace!(
                        arg_index = i + 1,
                        total_args = arguments.len(),
                        "Processing base call argument"
                    );
                    let arg_id = self.build_expression(context, arg)?;
                    mir_arguments.push(MirOperand::Value(arg_id));
                }

                trace!(
                    total_arguments = mir_arguments.len(),
                    "Base call arguments (including this)"
                );

                // Allocate a result ValueId even though base constructors return void
                // This maintains SSA form consistency
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Base constructors always return void
                let result_type = MirType::Void;

                // Register the ValueId as a local (even for void)
                self.register_temp_local(
                    context,
                    result_id,
                    result_type,
                    expression.location.clone(),
                );

                // NOTE: Find the parent class constructor SymbolId
                // parent_class_symbol_id is the CLASS, we need to find its CONSTRUCTOR
                let parent_constructor_symbol_id = if let Some(parent_symbol) =
                    self.symbol_table.all_symbols().get(parent_class_symbol_id)
                {
                    // For classes, look for a child symbol that is a Constructor for this class
                    let constructor_id = self.symbol_table.all_symbols().iter()
                        .find(|(_, symbol)| {
                            matches!(symbol.kind, crate::resolver::SymbolKind::Constructor { class_id, .. } if class_id == *parent_class_symbol_id)
                        })
                        .map(|(id, _)| *id);

                    if let Some(constructor_id) = constructor_id {
                        trace!(
                            constructor_id = constructor_id.0,
                            class_name = %parent_symbol.name,
                            "Found parent constructor"
                        );
                        constructor_id
                    } else {
                        warn!(
                            class_name = %parent_symbol.name,
                            class_symbol_id = parent_class_symbol_id.0,
                            "No constructor found for parent class, using class SymbolId"
                        );
                        *parent_class_symbol_id
                    }
                } else {
                    warn!(
                        parent_class_symbol_id = parent_class_symbol_id.0,
                        "Parent class not found in symbol table"
                    );
                    *parent_class_symbol_id
                };

                // Create function call instruction
                // Base constructors are void, so dest = None
                let call_instruction = MirInstruction {
                    dest: None, // Void return
                    operation: MirOperation::Call {
                        function: MirOperand::Function(parent_constructor_symbol_id),
                        arguments: mir_arguments,
                    },
                    location: expression.location.clone(),
                };

                trace!(call_instruction = ?call_instruction, "Generated base call instruction");

                self.add_instruction(context, call_instruction);

                // Return the result_id for consistency (even though it represents void)
                Ok(result_id)
            }

            TastExpressionKind::ArrayLiteral {
                elements,
                element_type: _,
            } => {
                // NOTE: Handle array literal creation properly
                // Array literals like [1, 2, 3] need to be materialized into actual memory
                trace!(element_count = elements.len(), "Creating array literal");

                // Strategy:
                // 1. Allocate empty list using list.allocate (synthetic SymbolId(1003))
                // 2. For each element, call list.push (synthetic SymbolId(1004)) to add it
                // 3. Return the list pointer

                // Allocate the result ValueId for the list
                let list_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the list as a Ptr(I32) local
                self.register_temp_local(
                    context,
                    list_value_id,
                    MirType::Ptr(Box::new(MirType::I32)),
                    expression.location.clone(),
                );

                // Call list.allocate(size) to create initial list
                // Use synthetic SymbolId(1003) for list.allocate
                let size_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                self.register_temp_local(
                    context,
                    size_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                // Create size constant
                let size_instruction = MirInstruction {
                    dest: Some(size_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::Integer(elements.len() as i64)),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, size_instruction);

                // Call list.allocate (for arrays) or pairs.new (for pairs) with the size hint.
                // Pairs literals use {} syntax and have a Pairs concrete type.
                let alloc_function = if matches!(&expression.expr_type, ConcreteType::Pairs(_, _)) {
                    MirOperand::NamedFunction {
                        name: "pairs.new".to_string(),
                        symbol_id: SymbolId(1003),
                    }
                } else {
                    MirOperand::Function(SymbolId(1003))
                };
                let alloc_instruction = MirInstruction {
                    dest: Some(list_value_id),
                    operation: MirOperation::Call {
                        function: alloc_function,
                        arguments: vec![MirOperand::Value(size_value_id)],
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, alloc_instruction);

                // Now add each element using list.push (synthetic SymbolId(1004))
                for (idx, element) in elements.iter().enumerate() {
                    trace!(element_index = idx, "Adding array literal element");

                    // Build the element expression
                    let element_value_id = self.build_expression(context, element)?;

                    // NOTE: Detect element type and use appropriate list.push function
                    // For F64 elements, use list.push_f64 (SymbolId(1005))
                    // For I32 elements, use list.push (SymbolId(1004))
                    let element_type = context
                        .function
                        .locals
                        .get(&element_value_id)
                        .map(|local| local.local_type.clone())
                        .unwrap_or(MirType::I32);

                    let push_symbol = match element_type {
                        MirType::F64 => {
                            trace!(element_index = idx, "Element is F64, using list.push_f64");
                            SymbolId(1005) // list.push_f64
                        }
                        _ => {
                            trace!(element_index = idx, element_type = ?element_type, "Element using list.push");
                            SymbolId(1004) // list.push
                        }
                    };

                    // Call list.push(list, element) or list.push_f64(list, element)
                    // Note: list.push returns the list pointer, so we need to capture it
                    let push_result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    self.register_temp_local(
                        context,
                        push_result_id,
                        MirType::Ptr(Box::new(MirType::I32)),
                        expression.location.clone(),
                    );

                    let push_instruction = MirInstruction {
                        dest: Some(push_result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::Function(push_symbol),
                            arguments: vec![
                                MirOperand::Value(list_value_id),
                                MirOperand::Value(element_value_id),
                            ],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, push_instruction);

                    // Update list_value_id to point to the result of push
                    // (list.push returns the updated list pointer)
                    // We need to copy this back to list_value_id for the next iteration
                    let copy_instruction = MirInstruction {
                        dest: Some(list_value_id),
                        operation: MirOperation::Copy {
                            source: MirOperand::Value(push_result_id),
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, copy_instruction);
                }

                trace!(list_value_id = ?list_value_id, "Array literal created");
                Ok(list_value_id)
            }

            TastExpressionKind::ObjectLiteral { fields } => {
                let n = fields.len();
                let loc = expression.location.clone();
                trace!(field_count = n, "Creating object literal");

                // Allocate raw object memory: 4 bytes for count + 8 bytes per entry (key_ptr, val_ptr)
                let alloc_size = (4 + n * 8) as i64;
                let raw_obj_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(context, raw_obj_id, MirType::I32, loc.clone());

                self.add_instruction(
                    context,
                    MirInstruction {
                        dest: Some(raw_obj_id),
                        operation: MirOperation::Call {
                            function: MirOperand::NamedFunction {
                                name: "mem_alloc".to_string(),
                                symbol_id: SymbolId(0),
                            },
                            arguments: vec![
                                MirOperand::Constant(MirConstant::Integer(0)),
                                MirOperand::Constant(MirConstant::Integer(alloc_size)),
                            ],
                        },
                        location: loc.clone(),
                    },
                );

                // Write field count at offset 0
                self.add_instruction(
                    context,
                    MirInstruction {
                        dest: None,
                        operation: MirOperation::Store {
                            destination: MirOperand::Value(raw_obj_id),
                            value: MirOperand::Constant(MirConstant::Integer(n as i64)),
                        },
                        location: loc.clone(),
                    },
                );

                // For each field: write key_ptr at offset 4+i*8, boxed val_ptr at offset 4+i*8+4
                for (i, field) in fields.iter().enumerate() {
                    let key_byte_offset = (4 + i * 8) as i64;
                    let val_byte_offset = (4 + i * 8 + 4) as i64;

                    // Key: build as a string literal — produces the [len|content] struct pointer
                    let key_str_expr = TastExpression {
                        kind: TastExpressionKind::Literal {
                            value: TastLiteral::String(field.key.clone()),
                        },
                        expr_type: ConcreteType::String,
                        location: field.location.clone(),
                    };
                    let key_id = self.build_expression(context, &key_str_expr)?;

                    // Value: build expression, then box to Any (unless already Any-typed)
                    let field_val_id = self.build_expression(context, &field.value)?;
                    let boxed_val_id = if matches!(field.value.expr_type, ConcreteType::Any) {
                        field_val_id
                    } else {
                        self.emit_box_any(
                            context,
                            field_val_id,
                            &field.value.expr_type,
                            &field.location,
                        )
                    };

                    // Compute offset pointers via Add
                    let key_ptr_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    self.register_temp_local(context, key_ptr_id, MirType::I32, loc.clone());
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: Some(key_ptr_id),
                            operation: MirOperation::BinaryOp {
                                op: MirBinaryOp::Add,
                                left: MirOperand::Value(raw_obj_id),
                                right: MirOperand::Constant(MirConstant::Integer(key_byte_offset)),
                            },
                            location: loc.clone(),
                        },
                    );
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: None,
                            operation: MirOperation::Store {
                                destination: MirOperand::Value(key_ptr_id),
                                value: MirOperand::Value(key_id),
                            },
                            location: loc.clone(),
                        },
                    );

                    let val_ptr_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    self.register_temp_local(context, val_ptr_id, MirType::I32, loc.clone());
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: Some(val_ptr_id),
                            operation: MirOperation::BinaryOp {
                                op: MirBinaryOp::Add,
                                left: MirOperand::Value(raw_obj_id),
                                right: MirOperand::Constant(MirConstant::Integer(val_byte_offset)),
                            },
                            location: loc.clone(),
                        },
                    );
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: None,
                            operation: MirOperation::Store {
                                destination: MirOperand::Value(val_ptr_id),
                                value: MirOperand::Value(boxed_val_id),
                            },
                            location: loc.clone(),
                        },
                    );
                }

                // Box raw_obj_id as an Object-typed Any value: [tag=6][raw_obj_id][0]
                let boxed_obj_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(context, boxed_obj_id, MirType::Any, loc.clone());
                self.add_instruction(
                    context,
                    MirInstruction {
                        dest: Some(boxed_obj_id),
                        operation: MirOperation::BoxAny {
                            value: MirOperand::Value(raw_obj_id),
                            type_tag: AnyTypeTag::Object,
                            source_type: MirType::I32,
                        },
                        location: loc,
                    },
                );

                trace!(boxed_obj_id = ?boxed_obj_id, "Object literal created");
                Ok(boxed_obj_id)
            }

            TastExpressionKind::Range {
                start,
                end,
                step: _, // Step is handled in build_range_for_loop for iteration, not here for array creation
                inclusive,
            } => {
                // Generate a range as an array of integers from start to end
                trace!(inclusive = inclusive, "Creating range array");

                // Evaluate start and end expressions
                let start_value_id = self.build_expression(context, start)?;
                let end_value_id = self.build_expression(context, end)?;

                // Calculate size: end - start + (1 if inclusive else 0)
                let size_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    size_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                // Subtract: size = end - start
                let diff_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    diff_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                let sub_instruction = MirInstruction {
                    dest: Some(diff_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Sub,
                        left: MirOperand::Value(end_value_id),
                        right: MirOperand::Value(start_value_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, sub_instruction);

                // Add 1 if inclusive: size = diff + 1
                let adjustment = if *inclusive { 1 } else { 0 };
                let adjustment_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    adjustment_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                let const_instruction = MirInstruction {
                    dest: Some(adjustment_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::Integer(adjustment)),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, const_instruction);

                let add_instruction = MirInstruction {
                    dest: Some(size_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Add,
                        left: MirOperand::Value(diff_value_id),
                        right: MirOperand::Value(adjustment_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, add_instruction);

                // Allocate the list using list.allocate (SymbolId(1003))
                let list_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    list_value_id,
                    MirType::Ptr(Box::new(MirType::I32)),
                    expression.location.clone(),
                );

                let alloc_instruction = MirInstruction {
                    dest: Some(list_value_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(SymbolId(1003)),
                        arguments: vec![MirOperand::Value(size_value_id)],
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, alloc_instruction);

                // Now populate the list with values from start to end
                // Use a counter variable to track current value
                let counter_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    counter_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                // Initialize counter to start value
                let init_instruction = MirInstruction {
                    dest: Some(counter_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Value(start_value_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, init_instruction);

                // Create loop blocks using next_block_id counter
                let base_block_id = context.function.next_block_id;
                let loop_header = BasicBlockId(base_block_id);
                let loop_body = BasicBlockId(base_block_id + 1);
                let loop_increment = BasicBlockId(base_block_id + 2);
                let loop_exit = BasicBlockId(base_block_id + 3);
                context.function.next_block_id = base_block_id + 4;

                // Jump to loop header
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: loop_header,
                    },
                );

                // Loop header: check if counter <= end (or < end if not inclusive)
                context.function.blocks.insert(
                    loop_header,
                    MirBasicBlock {
                        id: loop_header,
                        label: Some("range_header".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: expression.location.clone(),
                    },
                );
                self.current_block = Some(loop_header);

                let condition_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    condition_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                let comparison_op = if *inclusive {
                    MirBinaryOp::Le
                } else {
                    MirBinaryOp::Lt
                };

                let cmp_instruction = MirInstruction {
                    dest: Some(condition_value_id),
                    operation: MirOperation::BinaryOp {
                        op: comparison_op,
                        left: MirOperand::Value(counter_value_id),
                        right: MirOperand::Value(end_value_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, cmp_instruction);

                self.set_block_terminator(
                    context,
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_value_id),
                        true_block: loop_body,
                        false_block: loop_exit,
                    },
                );

                // Loop body: push counter value to list
                context.function.blocks.insert(
                    loop_body,
                    MirBasicBlock {
                        id: loop_body,
                        label: Some("range_body".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: expression.location.clone(),
                    },
                );
                self.current_block = Some(loop_body);

                let push_result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    push_result_id,
                    MirType::Ptr(Box::new(MirType::I32)),
                    expression.location.clone(),
                );

                let push_instruction = MirInstruction {
                    dest: Some(push_result_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(SymbolId(1004)), // list.push
                        arguments: vec![
                            MirOperand::Value(list_value_id),
                            MirOperand::Value(counter_value_id),
                        ],
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, push_instruction);

                // Update list pointer (list.push returns updated list)
                let copy_instruction = MirInstruction {
                    dest: Some(list_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Value(push_result_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, copy_instruction);

                // Jump to increment block
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: loop_increment,
                    },
                );

                // Increment block: counter = counter + 1
                context.function.blocks.insert(
                    loop_increment,
                    MirBasicBlock {
                        id: loop_increment,
                        label: Some("range_increment".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: expression.location.clone(),
                    },
                );
                self.current_block = Some(loop_increment);

                let one_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    one_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                let one_instruction = MirInstruction {
                    dest: Some(one_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::Integer(1)),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, one_instruction);

                let inc_instruction = MirInstruction {
                    dest: Some(counter_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Add,
                        left: MirOperand::Value(counter_value_id),
                        right: MirOperand::Value(one_value_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, inc_instruction);

                // Jump back to loop header
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: loop_header,
                    },
                );

                // Loop exit: continue with the list
                context.function.blocks.insert(
                    loop_exit,
                    MirBasicBlock {
                        id: loop_exit,
                        label: Some("range_exit".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: expression.location.clone(),
                    },
                );
                self.current_block = Some(loop_exit);

                trace!("Range array created");
                Ok(list_value_id)
            }

            _ => {
                // Unsupported expression type - return error with details
                Err(vec![CompilerError::validation_error(
                    format!("Expression type not yet implemented: {:?}", expression.kind),
                    expression.location.clone(),
                )])
            }
        }
    }
}
