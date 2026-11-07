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
                let constant = self.convert_literal(value, &expression.expr_type);
                let value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the ValueId as a temporary local for codegen
                let mir_type = self.convert_literal_type(value, &expression.expr_type);
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

                // Special case for 'this' - in class methods, 'this' refers to the first parameter.
                if name == "this" && context.class_context.is_some() {
                    // Instance methods have `this` as their first parameter (added in
                    // functions.rs only when `is_static = false`).
                    if let Some(first_param) = context.function.parameters.first() {
                        if first_param.name == "this" {
                            return Ok(first_param.value_id);
                        }
                    }
                    // Static methods have no `this` parameter. The only place the
                    // typechecker leaves a `this` reference reachable from a static
                    // method body is the implicit receiver of a sibling MethodCall —
                    // the MethodCall MIR builder drops that receiver when the callee
                    // is also static. Emit a dummy `i32.const 0` so the receiver
                    // ValueId is well-defined; it never reaches the operand stack.
                    let dummy_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    self.register_temp_local(
                        context,
                        dummy_id,
                        MirType::I32,
                        expression.location.clone(),
                    );
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: Some(dummy_id),
                            operation: MirOperation::Copy {
                                source: MirOperand::Constant(MirConstant::Integer(0)),
                            },
                            location: expression.location.clone(),
                        },
                    );
                    return Ok(dummy_id);
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
                // Short-circuit lowering for the logical operators `and` / `or`.
                //
                // The right-hand operand must not execute when the left
                // already decides the result — otherwise boundary-guarded
                // expressions like
                // `i < s.length() and s.substring(i, i+1) == " "` evaluate
                // substring with an out-of-bounds index and trap.
                //
                // We capture the right-hand operand's MIR instructions
                // into a side buffer (without leaving them in any block)
                // and pack them into a single
                // `MirOperation::LogicalShortCircuit`. The codegen emits a
                // WASM `if (result i32) ... else ... end` whose
                // then-branch inlines those instructions, so the rhs only
                // executes when the lhs cannot short-circuit.
                //
                // Bitwise variants (`BitwiseAnd`, `BitwiseOr`, `Xor`,
                // `Shl`, `Shr`) fall through to the generic BinaryOp path
                // below.
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
                    return self.build_short_circuit_logical(
                        context,
                        operator,
                        left,
                        right,
                        &expression.location,
                    );
                }

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

                    // Pick the math.pow variant whose calling convention matches the result type.
                    let pow_function = if matches!(result_type, MirType::F64) {
                        SYM_BUILTIN_MATH_POW_F64
                    } else {
                        SYM_BUILTIN_MATH_POW_I32
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

                // `value default fallback` (06-expressions.md §Default Operator):
                // the fallback is evaluated only when `value` is null. Only
                // null coalesces — `0`, `false` and `""` are values.
                if matches!(operator, BinaryOperator::NullCoalesce) {
                    let left_is_null = matches!(
                        &left.kind,
                        TastExpressionKind::Literal {
                            value: TastLiteral::Null
                        }
                    ) || matches!(&left.expr_type, ConcreteType::Null);
                    if left_is_null {
                        return self.build_expression(context, right);
                    }
                    // A non-null literal, and a plain integer, number or
                    // boolean (only `T?` holds null), is never null.
                    let left_is_non_null_literal = matches!(
                        &left.kind,
                        TastExpressionKind::Literal { value } if !matches!(value, TastLiteral::Null)
                    );
                    if left_is_non_null_literal || Self::is_non_nullable_value_type(&left.expr_type)
                    {
                        return self.build_expression(context, left);
                    }

                    let left_id = self.build_expression(context, left)?;
                    let non_null =
                        self.emit_non_null_test(context, left_id, &left.expr_type, &left.location);
                    return self.build_two_way_value(
                        context,
                        non_null,
                        BranchArm::Built(left_id, left),
                        BranchArm::Lazy(right),
                        &expression.expr_type,
                        &expression.location,
                    );
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

                    // Generate call to string.concat runtime function.
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::Function(SYM_BUILTIN_STRING_CONCAT),
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

                    // Auto-unbox `any` operands when the other side is a primitive.
                    //
                    // The result of expressions like `data.field` on an `any`-typed
                    // receiver is itself an `any` (boxed [tag, value] heap struct).
                    // A naive BinaryOp would compare the boxed pointer to the
                    // primitive literal (e.g. `pointer == 0`), which is never true
                    // for a found field and always true for `!= 0`. The compiler
                    // already has the unbox machinery used by assignments and
                    // explicit `.toInteger()` / `.toNumber()` method calls — we
                    // reuse it here so `if data.count == 0` and similar patterns
                    // evaluate against the contained value, not the heap pointer.
                    //
                    // For String, we dispatch to `AnyToString` (runtime type-tag
                    // dispatch producing a real length-prefixed string) rather
                    // than `UnboxAnyToI32`, because the downstream string-compare
                    // branch needs a real string pointer — reading offset 4 raw
                    // would only work for string-tagged any values and produce
                    // garbage for other tags. Without this, `data.kind == "user"`
                    // compares the boxed-any pointer bits to the literal's first
                    // 4 bytes and silently always evaluates false.
                    //
                    // Two `any` operands of `==` / `!=` compare their values
                    // (`emit_any_values_equal`), not their box pointers.
                    //
                    // Regression tests:
                    //   tests/test_any_int_compare.rs    (ANY_INT_COMPARE_USES_POINTER)
                    //   tests/test_any_string_compare.rs (ANY_STRING_COMPARE_USES_POINTER)
                    let left_is_any = matches!(left_mir_type, MirType::Any);
                    let right_is_any = matches!(right_mir_type, MirType::Any);

                    let (left_id, left_mir_type) = if left_is_any && !right_is_any {
                        let target = Self::mir_type_to_concrete(&right_mir_type);
                        match target {
                            ConcreteType::Integer
                            | ConcreteType::Number
                            | ConcreteType::Boolean => {
                                let unboxed =
                                    self.emit_unbox_any(context, left_id, &target, &left.location);
                                let new_type = context
                                    .function
                                    .locals
                                    .get(&unboxed)
                                    .map(|l| l.local_type.clone())
                                    .unwrap_or(left_mir_type);
                                (unboxed, new_type)
                            }
                            ConcreteType::String => {
                                let converted =
                                    self.emit_any_to_string(context, left_id, &left.location);
                                let new_type = context
                                    .function
                                    .locals
                                    .get(&converted)
                                    .map(|l| l.local_type.clone())
                                    .unwrap_or(left_mir_type);
                                (converted, new_type)
                            }
                            _ => (left_id, left_mir_type),
                        }
                    } else {
                        (left_id, left_mir_type)
                    };

                    let (right_id, right_mir_type) = if right_is_any && !left_is_any {
                        let target = Self::mir_type_to_concrete(&left_mir_type);
                        match target {
                            ConcreteType::Integer
                            | ConcreteType::Number
                            | ConcreteType::Boolean => {
                                let unboxed = self.emit_unbox_any(
                                    context,
                                    right_id,
                                    &target,
                                    &right.location,
                                );
                                let new_type = context
                                    .function
                                    .locals
                                    .get(&unboxed)
                                    .map(|l| l.local_type.clone())
                                    .unwrap_or(right_mir_type);
                                (unboxed, new_type)
                            }
                            ConcreteType::String => {
                                let converted =
                                    self.emit_any_to_string(context, right_id, &right.location);
                                let new_type = context
                                    .function
                                    .locals
                                    .get(&converted)
                                    .map(|l| l.local_type.clone())
                                    .unwrap_or(right_mir_type);
                                (converted, new_type)
                            }
                            _ => (right_id, right_mir_type),
                        }
                    } else {
                        (right_id, right_mir_type)
                    };

                    if left_is_any
                        && right_is_any
                        && matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual)
                    {
                        let equal = self.emit_any_values_equal(
                            context,
                            left_id,
                            right_id,
                            &expression.location,
                        );
                        if matches!(operator, BinaryOperator::Equal) {
                            return Ok(equal);
                        }
                        let result_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;
                        self.register_temp_local(
                            context,
                            result_id,
                            MirType::I32,
                            expression.location.clone(),
                        );
                        self.add_instruction(
                            context,
                            MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::BinaryOp {
                                    op: MirBinaryOp::Eq,
                                    left: MirOperand::Value(equal),
                                    right: MirOperand::Constant(MirConstant::Integer(0)),
                                },
                                location: expression.location.clone(),
                            },
                        );
                        return Ok(result_id);
                    }

                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    let left_concrete = Self::mir_type_to_concrete(&left_mir_type);
                    let right_concrete = Self::mir_type_to_concrete(&right_mir_type);

                    let mut result_type =
                        self.infer_binary_operation_type(&left_concrete, &right_concrete, operator);
                    // `integer:64` operands: arithmetic and bitwise results are
                    // i64 (the i32 side widens, SEM001). ConcreteType has no
                    // integer width, so the inference above cannot see this.
                    let is_int64 = |t: &MirType| matches!(t, MirType::I64 | MirType::U64);
                    let is_float = |t: &MirType| matches!(t, MirType::F32 | MirType::F64);
                    let is_comparison_or_logical = matches!(
                        operator,
                        BinaryOperator::Equal
                            | BinaryOperator::NotEqual
                            | BinaryOperator::LessThan
                            | BinaryOperator::GreaterThan
                            | BinaryOperator::LessThanOrEqual
                            | BinaryOperator::GreaterThanOrEqual
                            | BinaryOperator::And
                            | BinaryOperator::Or
                    );
                    if !is_comparison_or_logical
                        && (is_int64(&left_mir_type) || is_int64(&right_mir_type))
                        && !is_float(&left_mir_type)
                        && !is_float(&right_mir_type)
                    {
                        result_type = MirType::I64;
                    }
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

                // `value!` checks for null; a plain integer, number or boolean
                // is never null, and `0` / `false` must pass the check.
                if matches!(operator, UnaryOperator::Required)
                    && Self::is_non_nullable_value_type(&operand.expr_type)
                {
                    return Ok(operand_id);
                }

                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the result as a temporary local for codegen
                // A numeric operand keeps its built width: an integer literal
                // outside the i32 range is built as an `integer:64` constant even
                // though the checker typed it `integer`, and `-5000000000`
                // must negate in i64.
                let operand_mir_type = context
                    .function
                    .locals
                    .get(&operand_id)
                    .map(|local| local.local_type.clone());
                let result_type = match (operator, operand_mir_type) {
                    (
                        UnaryOperator::Negate,
                        Some(t @ (MirType::I64 | MirType::U64 | MirType::F64)),
                    ) => t,
                    _ => self.infer_unary_operation_type(&operand.expr_type),
                };
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

                // `list.<method>(items, args...)` is the namespace form of
                // `items.<method>(args...)`. Lower it through the method-call
                // path so both forms share element-typed dispatch (`number`
                // lists use 8-byte slots) and the receiver write-back of
                // operations that can move the list (`add`, `push`, `insert`).
                if let Some(method) = function_name_opt
                    .as_deref()
                    .and_then(|name| name.strip_prefix("list."))
                {
                    if Self::is_list_receiver_method(method)
                        && arguments
                            .first()
                            .is_some_and(|a| matches!(a.expr_type, ConcreteType::Array(_)))
                    {
                        let method_call = TastExpression {
                            kind: TastExpressionKind::MethodCall {
                                receiver: Box::new(arguments[0].clone()),
                                method_name: method.to_string(),
                                method_symbol: function_symbol_id,
                                arguments: arguments[1..].to_vec(),
                                type_args: Vec::new(),
                            },
                            ..expression.clone()
                        };
                        return self.build_expression(context, &method_call);
                    }
                }

                // `list.fill(size, value)` with a `number` or `integer:64`
                // value builds a list of 8-byte elements
                // (src/stdlib/list_class.rs).
                if function_name_opt.as_deref() == Some("list.fill")
                    && arguments.len() == 2
                    && matches!(
                        arguments[1].expr_type,
                        ConcreteType::Number | ConcreteType::IntegerSized { bits: 64, .. }
                    )
                {
                    let fill_name = Self::list_method_variant("fill", &arguments[1].expr_type)
                        .expect("8-byte elements have a fill variant");
                    let mut args = Vec::with_capacity(2);
                    for arg in arguments {
                        args.push(MirOperand::Value(self.build_expression(context, arg)?));
                    }
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    let result_type = self.convert_concrete_type(&expression.expr_type);
                    self.register_temp_local(
                        context,
                        result_id,
                        result_type,
                        expression.location.clone(),
                    );
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: Some(result_id),
                            operation: MirOperation::Call {
                                function: MirOperand::NamedFunction {
                                    name: fill_name,
                                    symbol_id: SymbolId(0),
                                },
                                arguments: args,
                            },
                            location: expression.location.clone(),
                        },
                    );
                    return Ok(result_id);
                }

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

                // NOTE: Handle standalone toString(number) function calls.
                //
                // `toString` is registered in the resolver's symbol table as
                // `(Integer) -> String` (see resolver_impl.rs). When callers pass a
                // Number (f64) argument, the type checker resolves the call to that
                // symbol and the general FunctionCall codegen path loads an f64 onto
                // the stack — into the callee's i32 parameter slot. wasmparser then
                // rejects the module with "type mismatch: expected i32, found f64"
                // (COM001 fingerprint f26672a3b9a11308, discovered while investigating
                // the sibling Number.toString() method-call bug in July 2026).
                //
                // Redirect to the `float_to_string` host import, which has the
                // correct `(f64) -> i32` signature.
                if matches!(function_name_opt.as_deref(), Some("toString"))
                    && arguments.len() == 1
                    && matches!(arguments[0].expr_type, ConcreteType::Number)
                {
                    let arg_id = self.build_expression(context, &arguments[0])?;

                    return self.emit_builtin_call(
                        context,
                        "float_to_string",
                        vec![MirOperand::Value(arg_id)],
                        MirType::Ptr(Box::new(MirType::U8)),
                        &expression.location,
                    );
                }

                // JSON calls lower to the Layer 2 host bridges
                // (foundation/spec/stdlib-reference.md §8); the compiler
                // embeds no JSON parser or serializer.
                if let Some(result_id) = self.build_json_bridge_call(
                    context,
                    function_name_opt.as_deref(),
                    arguments,
                    &expression.location,
                )? {
                    return Ok(result_id);
                }

                // Built-in `db.*` calls lower to their canonical bridge
                // import (stdlib-reference §9), so they resolve with or
                // without the frame.data plugin's language aliases.
                if let Some(result_id) = self.build_db_bridge_call(
                    context,
                    function_name_opt.as_deref(),
                    arguments,
                    &expression.expr_type,
                    &expression.location,
                )? {
                    return Ok(result_id);
                }

                // Build argument operands
                let mut mir_arguments = Vec::new();

                // Check if this is a constructor call: it returns a Class type and
                // targets `<Class>.constructor`. A regular function that returns a
                // class instance (`K e = makeK()`) is an ordinary call — it must not
                // get a freshly allocated instance prepended as an argument.
                let is_constructor_target = function_name_opt
                    .as_deref()
                    .is_some_and(|name| name == "constructor" || name.ends_with(".constructor"));
                if let (
                    ConcreteType::Class {
                        symbol_id: class_symbol_id,
                        ..
                    },
                    true,
                ) = (&expression.expr_type, is_constructor_target)
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

                    // Write the object header (bytes 0-3): the class mark and
                    // the runtime class-id. Read by dynamic dispatch on
                    // capability-typed receivers and by the JSON bridges,
                    // which build the object of the instance's own class.
                    // See helpers.rs::OBJECT_HEADER_SIZE and MEMORY_MODEL.md.
                    let class_id_val = self.class_ids.get(class_symbol_id).copied().unwrap_or(0);
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: None,
                            operation: MirOperation::Store {
                                destination: MirOperand::Value(alloc_result),
                                value: MirOperand::Constant(MirConstant::Integer(
                                    crate::mir::class_header_word(class_id_val) as i64,
                                )),
                            },
                            location: expression.location.clone(),
                        },
                    );

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
                    .or_else(|| {
                        // Fallback: look up builtin/namespace functions in the symbol table by
                        // name. Builtins (json.get, math.sqrt, etc.) all share SymbolId(0) in
                        // the TAST, so the all_functions lookup above always misses them.
                        // Without this, Any-parameter boxing is skipped for string/integer
                        // arguments to builtins like json.get(string, key) (RUNTIME002).
                        let name = function_name_opt.as_deref()?;
                        let sym_id = self.symbol_table.lookup_symbol(name)?;
                        let symbol = self.symbol_table.get_symbol(sym_id)?;
                        if let crate::resolver::SymbolKind::Function { parameters, .. } =
                            &symbol.kind
                        {
                            if !parameters.is_empty() {
                                let types: Vec<ConcreteType> = parameters
                                    .iter()
                                    .map(|hir_type| match hir_type {
                                        crate::hir::HirType::Any => ConcreteType::Any,
                                        crate::hir::HirType::String => ConcreteType::String,
                                        crate::hir::HirType::Integer => ConcreteType::Integer,
                                        crate::hir::HirType::Number => ConcreteType::Number,
                                        crate::hir::HirType::Boolean => ConcreteType::Boolean,
                                        _ => ConcreteType::Unknown,
                                    })
                                    .collect();
                                return Some(types);
                            }
                        }
                        None
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
                        let param_type_opt = param_types.get(arg_idx);

                        // When a class instance is passed where a string is expected,
                        // auto-serialize it to JSON text: its compiler-generated __json_tree_ClassName
                        // object encoded by the _json_encode_v2 bridge.
                        // This enables api.post(url, classInstance) without explicit json.encode().
                        let needs_class_serialize =
                            matches!(param_type_opt, Some(ConcreteType::String))
                                && matches!(&arg.expr_type, ConcreteType::Class { .. });

                        // A plugin `ptr` parameter takes the string or byte
                        // handle as is (type-system.md §9b), never a box.
                        let raw_ptr_param = function_name_opt
                            .as_deref()
                            .is_some_and(|name| self.symbol_table.is_raw_ptr_param(name, arg_idx));

                        // Check if parameter type is Any and argument type is not Any.
                        let needs_boxing = !needs_class_serialize
                            && !raw_ptr_param
                            && matches!(param_type_opt, Some(ConcreteType::Any))
                            && !matches!(arg.expr_type, ConcreteType::Any);

                        if needs_class_serialize {
                            let result_id = self.emit_class_json_text(
                                context,
                                arg_id,
                                &arg.expr_type,
                                &arg.location,
                            );
                            trace!(
                                arg_type = ?arg.expr_type,
                                "Auto-serialized class arg to JSON string"
                            );
                            result_id
                        } else if needs_boxing {
                            trace!(
                                arg_idx = arg_idx,
                                arg_type = ?arg.expr_type,
                                "Boxing argument to any type"
                            );
                            self.emit_box_any(context, arg_id, &arg.expr_type, &arg.location)
                        } else {
                            self.unbox_any_argument(
                                context,
                                arg_id,
                                &arg.expr_type,
                                param_type_opt,
                                &arg.location,
                            )
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

                // `obj.toString()` on a class that declares no `toString()`
                // (nor inherits one) is the class name (07-statements.md
                // "Default Class Behavior").
                if let ConcreteType::Class {
                    symbol_id: class_symbol,
                    ..
                } = &receiver_actual_type
                {
                    if method_name == "toString" && method_symbol.0 == 0 && arguments.is_empty() {
                        if let Some(class_name) = self
                            .all_classes
                            .iter()
                            .find(|c| c.symbol_id == *class_symbol)
                            .map(|c| c.name.clone())
                        {
                            let name_literal = TastExpression {
                                kind: TastExpressionKind::Literal {
                                    value: TastLiteral::String(class_name),
                                },
                                expr_type: ConcreteType::String,
                                location: expression.location.clone(),
                            };
                            return self.build_expression(context, &name_literal);
                        }
                    }
                }

                // Capability-typed receiver — emit dynamic dispatch via
                // MirOperation::CallCapability. The receiver's class-id (at
                // offset 0 of its instance header) drives the class switch
                // in codegen; the target method's return type comes from
                // the capability's declared signature.
                // See Clean Language Specification §Capabilities.
                if let ConcreteType::Interface {
                    symbol_id: cap_sym, ..
                } = &receiver_actual_type
                {
                    let slot_index = self.symbol_table.get_symbol(*cap_sym).and_then(|s| {
                        if let crate::resolver::symbol_table::SymbolKind::Capability { methods } =
                            &s.kind
                        {
                            methods.iter().position(|m| &m.name == method_name)
                        } else {
                            None
                        }
                    });
                    let Some(slot_index) = slot_index else {
                        // Type checker should have caught this — capability has
                        // no such method. Fall through to the general error path.
                        return Err(vec![CompilerError::codegen_error(
                            format!(
                                "Capability #{} has no method '{}' — this should have been rejected at type-check time",
                                cap_sym.0, method_name
                            ),
                            None,
                            Some(expression.location.clone()),
                        )]);
                    };

                    // Lower arguments (receiver is separate).
                    let mut mir_args = Vec::with_capacity(arguments.len());
                    for arg in arguments {
                        let arg_id = self.build_expression(context, arg)?;
                        mir_args.push(MirOperand::Value(arg_id));
                    }

                    // Determine the result type from the capability method's declared return.
                    let (result_type, has_return) = self
                        .symbol_table
                        .get_symbol(*cap_sym)
                        .and_then(|s| {
                            if let crate::resolver::symbol_table::SymbolKind::Capability {
                                methods,
                            } = &s.kind
                            {
                                methods.get(slot_index).map(|m| {
                                    let has_ret =
                                        !matches!(&m.return_type, crate::hir::HirType::Void);
                                    // MIR's HirType-aware conversion path uses a
                                    // Concrete detour via TAST — for our purposes
                                    // (Interface method dispatch), map the HirType
                                    // directly to a MirType via existing helpers.
                                    let mir_ty = match &m.return_type {
                                        crate::hir::HirType::Integer => MirType::I32,
                                        crate::hir::HirType::Number => MirType::F64,
                                        crate::hir::HirType::Boolean => MirType::I32,
                                        crate::hir::HirType::String => {
                                            MirType::Ptr(Box::new(MirType::I8))
                                        }
                                        crate::hir::HirType::Void => MirType::Void,
                                        _ => MirType::I32, // Pointers for classes, lists, etc.
                                    };
                                    (mir_ty, has_ret)
                                })
                            } else {
                                None
                            }
                        })
                        .unwrap_or((MirType::Void, false));

                    // Always allocate a fresh result ValueId, even for void
                    // returns. The MethodCall caller sometimes routes the
                    // returned ValueId into a Return terminator (which would
                    // emit `local.get N; return`), so we cannot re-use the
                    // receiver's ValueId as a placeholder — that would push
                    // the receiver onto the stack of a void-returning
                    // function. For void methods we register a dummy i32
                    // local that codegen never stores to.
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    let dest = if has_return {
                        self.register_temp_local(
                            context,
                            result_id,
                            result_type,
                            expression.location.clone(),
                        );
                        Some(result_id)
                    } else {
                        // Void method — register an unused i32 marker so any
                        // downstream lookup on `result_id` returns something
                        // sensible, but don't set dest so codegen skips
                        // store_to_local.
                        self.register_temp_local(
                            context,
                            result_id,
                            MirType::I32,
                            expression.location.clone(),
                        );
                        None
                    };

                    let instruction = MirInstruction {
                        dest,
                        operation: MirOperation::CallCapability {
                            receiver: MirOperand::Value(receiver_id),
                            capability_symbol: *cap_sym,
                            slot_index,
                            arguments: mir_args,
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);

                    return Ok(result_id);
                }

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

                // NOTE: Handle IntegerSized{64}.toString() EARLY — bypass the regular
                // `integer.toString` dispatch which lowers to native `int_to_string(i32)` and
                // would corrupt the high bits. Route to `int64_to_string` which takes i64.
                // The receiver's MIR type decides too: an `integer:64` field read
                // (`obj.total`) can carry the unsized `integer` type in the TAST
                // while its value is loaded as an i64.
                let receiver_is_i64 = matches!(
                    &receiver_actual_type,
                    ConcreteType::IntegerSized { bits: 64, .. }
                ) || matches!(
                    context
                        .function
                        .locals
                        .get(&receiver_id)
                        .map(|l| &l.local_type),
                    Some(MirType::I64 | MirType::U64)
                );
                if receiver_is_i64 && method_name == "toString" {
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::Ptr(Box::new(MirType::U8)),
                        expression.location.clone(),
                    );

                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::NamedFunction {
                                name: "int64_to_string".to_string(),
                                symbol_id: SymbolId(0),
                            },
                            arguments: vec![MirOperand::Value(receiver_id)],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    return Ok(result_id);
                }

                // NOTE: Handle Integer.toString() EARLY — same rationale as
                // IntegerSized{64} above, but for i32 receivers. Without this
                // early-dispatch, a non-zero method_symbol (e.g. resolver
                // registered `toString` as symbol 11) skips the type-conversion
                // fast-path at line ~1944 and falls into the general method-call
                // path, which treats the value as Any and emits UnboxAnyToI32
                // — reading the receiver's i32 value as a pointer and OOBing
                // when the value is a large timestamp (from time.now → wrap_i64).
                // Fingerprints 95c8a7cd (TIME-TOSTRING-001), ba77c082
                // (CODEGEN-I64-TOSTRING-OOB), 97d07985 (COD001).
                if matches!(&receiver_actual_type, ConcreteType::Integer)
                    && method_name == "toString"
                {
                    return self.emit_builtin_call(
                        context,
                        "int_to_string",
                        vec![MirOperand::Value(receiver_id)],
                        MirType::Ptr(Box::new(MirType::U8)),
                        &expression.location,
                    );
                }

                // NOTE: Handle Number.toString() EARLY — same rationale as the
                // Integer.toString() dispatch above. `stdlib::method_style`
                // registers `number.toString` as an `(i32) → i32` identity stub
                // (same shape as `integer.toString`/`boolean.toString`), so the
                // type checker resolves a non-zero method symbol and skips the
                // `method_symbol.0 == 0` special case at line ~2034 that would
                // have dispatched to `float_to_string`. Without this early
                // dispatch, an f64 value is passed to an i32 slot and wasmparser
                // rejects the module with "type mismatch: expected i32, found
                // f64". Reproduces on `data:` models with a `number` field when
                // the frame.data plugin generates `<field>.toString()` in
                // `__to_params`/`__update_set` builders.
                if matches!(&receiver_actual_type, ConcreteType::Number)
                    && method_name == "toString"
                {
                    return self.emit_builtin_call(
                        context,
                        "float_to_string",
                        vec![MirOperand::Value(receiver_id)],
                        MirType::Ptr(Box::new(MirType::U8)),
                        &expression.location,
                    );
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

                            let add_symbol = Self::list_add_symbol(element_type);

                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                let arg_id = self.coerce_list_element_arg(
                                    context,
                                    arg_id,
                                    &arg.expr_type,
                                    element_type,
                                    &arg.location,
                                );
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
                            self.write_back_list_receiver(
                                context,
                                receiver,
                                result_id,
                                &expression.location,
                            )?;
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

                            let add_symbol = Self::list_add_symbol(element_type);

                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                let arg_id = self.coerce_list_element_arg(
                                    context,
                                    arg_id,
                                    &arg.expr_type,
                                    element_type,
                                    &arg.location,
                                );
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
                            self.write_back_list_receiver(
                                context,
                                receiver,
                                result_id,
                                &expression.location,
                            )?;
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
                                    function: MirOperand::Function(SYM_BUILTIN_LIST_SIZE),
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        // `list<string>.contains(needle)` / `.indexOf(needle)`
                        // — dispatch to the byte-comparing variants when the
                        // element type is String. The default `list.contains`
                        // symbol routes to a pointer-equality body that
                        // returns `false` for byte-identical strings living at
                        // different heap offsets. Handled in the same "first
                        // pass regardless of method_symbol" block as `add`/
                        // `push`/`size` because the standard `list.contains`
                        // symbol IS registered non-zero and would otherwise
                        // short-circuit before the type-conversion match.
                        // Fixes prompt baf3ccc5.
                        "contains" | "indexOf"
                            if matches!(**element_type, ConcreteType::String) =>
                        {
                            let alias = if method_name == "contains" {
                                "list.contains.string"
                            } else {
                                "list.indexOf.string"
                            };
                            let str_symbol = match self.symbol_table.lookup_symbol(alias) {
                                Some(sym) => sym,
                                None => {
                                    warn!(
                                        "{} not registered; falling through to \
                                         pointer-equality variant (bug baf3ccc5 may surface)",
                                        alias
                                    );
                                    return Err(vec![CompilerError::validation_error(
                                        format!("stdlib symbol `{}` missing", alias),
                                        expression.location.clone(),
                                    )]);
                                }
                            };

                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32, // Boolean or Integer — both are i32
                                expression.location.clone(),
                            );

                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(str_symbol),
                                    arguments: args,
                                },
                                location: expression.location.clone(),
                            };
                            self.add_instruction(context, instruction);
                            return Ok(result_id);
                        }
                        // Element-slot access whose body depends on the element
                        // representation (src/stdlib/list_class.rs):
                        // `.remove(index)`, the behavior-aware `.remove()`
                        // (front of a `.line`, last element otherwise),
                        // `.pop()` / `.removeLast()`, `.set(index, value)` and
                        // `list<number>.get(index)`.
                        "remove" | "pop" | "removeLast" | "set" | "get"
                            if method_name != "get"
                                || matches!(
                                    **element_type,
                                    ConcreteType::Number
                                        | ConcreteType::IntegerSized { bits: 64, .. }
                                ) =>
                        {
                            let variant = match method_name.as_str() {
                                "remove" if arguments.len() == 1 => "removeAt",
                                "pop" => "removeLast",
                                other => other,
                            };
                            let function_name = Self::list_method_variant(variant, element_type)
                                .expect(
                                    "element-slot methods have a variant for every element type",
                                );
                            let is_set = method_name == "set";
                            let result_type = if is_set {
                                self.convert_concrete_type(&expression.expr_type)
                            } else {
                                self.convert_concrete_type(element_type)
                            };
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                result_type,
                                expression.location.clone(),
                            );
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for (index, arg) in arguments.iter().enumerate() {
                                let mut arg_id = self.build_expression(context, arg)?;
                                if Self::list_element_arg_index(method_name) == Some(index) {
                                    arg_id = self.coerce_list_element_arg(
                                        context,
                                        arg_id,
                                        &arg.expr_type,
                                        element_type,
                                        &arg.location,
                                    );
                                }
                                args.push(MirOperand::Value(arg_id));
                            }
                            self.add_instruction(
                                context,
                                MirInstruction {
                                    dest: if is_set { None } else { Some(result_id) },
                                    operation: MirOperation::Call {
                                        function: MirOperand::NamedFunction {
                                            name: function_name,
                                            symbol_id: SymbolId(0),
                                        },
                                        arguments: args,
                                    },
                                    location: expression.location.clone(),
                                },
                            );
                            return Ok(result_id);
                        }
                        // List methods whose implementation depends on the
                        // element representation (src/stdlib/list_class.rs):
                        // 8-byte `number` elements use the `.f64` variants,
                        // strings compare by bytes for ordering/search, and
                        // every other element type uses the i32 body.
                        "first" | "last" | "peek" | "indexOf" | "lastIndexOf" | "contains"
                        | "reverse" | "sort" | "slice" | "concat" | "insert"
                            if Self::list_method_variant(method_name, element_type).is_some() =>
                        {
                            let function_name =
                                Self::list_method_variant(method_name, element_type)
                                    .expect("guarded by is_some");
                            let result_type = match method_name.as_str() {
                                "first" | "last" | "peek" => {
                                    self.convert_concrete_type(element_type)
                                }
                                "indexOf" | "lastIndexOf" | "contains" | "insert" => MirType::I32,
                                _ => self.convert_concrete_type(&receiver.expr_type),
                            };
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                result_type,
                                expression.location.clone(),
                            );
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for (index, arg) in arguments.iter().enumerate() {
                                let mut arg_id = self.build_expression(context, arg)?;
                                if Self::list_element_arg_index(method_name) == Some(index) {
                                    arg_id = self.coerce_list_element_arg(
                                        context,
                                        arg_id,
                                        &arg.expr_type,
                                        element_type,
                                        &arg.location,
                                    );
                                }
                                args.push(MirOperand::Value(arg_id));
                            }
                            self.add_instruction(
                                context,
                                MirInstruction {
                                    dest: Some(result_id),
                                    operation: MirOperation::Call {
                                        function: MirOperand::NamedFunction {
                                            name: function_name,
                                            symbol_id: SymbolId(0),
                                        },
                                        arguments: args,
                                    },
                                    location: expression.location.clone(),
                                },
                            );
                            // insert grows the list through list.add and returns the
                            // (possibly moved) list pointer.
                            if method_name == "insert" {
                                self.write_back_list_receiver(
                                    context,
                                    receiver,
                                    result_id,
                                    &expression.location,
                                )?;
                            }
                            return Ok(result_id);
                        }
                        _ => {} // Fall through for other methods
                    }
                }

                // NOTE: Handle String.matches EARLY — the pattern name is resolved to
                // the compile-time pattern id the `string_matches` host import takes
                // (`STRING_MATCH_PATTERNS`, shared with the type checker's SEM010).
                if matches!(&receiver.expr_type, ConcreteType::String) && method_name == "matches" {
                    let pattern_id = match arguments.first().map(|a| &a.kind) {
                        Some(TastExpressionKind::Literal {
                            value: TastLiteral::String(pattern),
                        }) => crate::typechecker::tast::string_match_pattern_id(pattern),
                        _ => None,
                    };
                    let Some(pattern_id) = pattern_id else {
                        return Err(vec![CompilerError::validation_error(
                            "string.matches() needs one of the supported pattern names as a string literal",
                            expression.location.clone(),
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
                            return self.emit_builtin_call(
                                context,
                                "int_to_string",
                                vec![MirOperand::Value(receiver_id)],
                                MirType::Ptr(Box::new(MirType::U8)),
                                &expression.location,
                            );
                        }
                        // Number to String conversion - call float_to_string
                        (ConcreteType::Number, "toString") => {
                            return self.emit_builtin_call(
                                context,
                                "float_to_string",
                                vec![MirOperand::Value(receiver_id)],
                                MirType::Ptr(Box::new(MirType::U8)),
                                &expression.location,
                            );
                        }
                        // Boolean to String conversion - call bool_to_string
                        (ConcreteType::Boolean, "toString") => {
                            return self.emit_builtin_call(
                                context,
                                "bool_to_string",
                                vec![MirOperand::Value(receiver_id)],
                                MirType::Ptr(Box::new(MirType::U8)),
                                &expression.location,
                            );
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
                        // Integer to Number conversion (i32/i64 -> f64). The Cast
                        // picks the instruction from the receiver's MIR width.
                        (ConcreteType::Integer | ConcreteType::IntegerSized { .. }, "toNumber") => {
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
                        // Number to Boolean conversion (f64 -> i32, non-zero = true)
                        (ConcreteType::Number, "toBoolean") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );

                            // Emit comparison: receiver != 0.0
                            let zero_const = MirConstant::Float(0.0);
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
                        // Identity conversions — return the receiver unchanged.
                        (ConcreteType::Integer, "toInteger")
                        | (ConcreteType::Number, "toNumber") => {
                            return Ok(receiver_id);
                        }
                        // `integer:64` → `integer`: the explicit narrowing
                        // conversion (type-system.md §7). The Cast wraps i64
                        // to i32; 8–32-bit sized integers are already i32.
                        (ConcreteType::IntegerSized { .. }, "toInteger") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );
                            self.add_instruction(
                                context,
                                MirInstruction {
                                    dest: Some(result_id),
                                    operation: MirOperation::Cast {
                                        value: MirOperand::Value(receiver_id),
                                        target_type: MirType::I32,
                                    },
                                    location: expression.location.clone(),
                                },
                            );
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
                            let symbol_id =
                                self.builtin_symbol("int_to_string", &expression.location)?;
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Number, "toString") => {
                            // Call float_to_string with the float value
                            let symbol_id =
                                self.builtin_symbol("float_to_string", &expression.location)?;
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Boolean, "toString") => {
                            // Call bool_to_string with the boolean value
                            let symbol_id =
                                self.builtin_symbol("bool_to_string", &expression.location)?;
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
                                    let symbol_id =
                                        self.builtin_symbol("int_to_string", &expression.location)?;
                                    (symbol_id, vec![MirOperand::Value(receiver_id)])
                                }
                                MirType::F64 => {
                                    // Call float_to_string
                                    let symbol_id = self
                                        .builtin_symbol("float_to_string", &expression.location)?;
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
                            let symbol_id =
                                self.builtin_symbol("string.length", &expression.location)?;
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "toUpperCase") => {
                            // Call string.toUpperCase
                            let symbol_id =
                                self.builtin_symbol("string.toUpperCase", &expression.location)?;
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "toLowerCase") => {
                            // Call string.toLowerCase
                            let symbol_id =
                                self.builtin_symbol("string.toLowerCase", &expression.location)?;
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "substring") => {
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (
                                self.builtin_symbol("string.substring", &expression.location)?,
                                args,
                            )
                        }
                        (ConcreteType::String, "contains") => {
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (
                                self.builtin_symbol("string.contains", &expression.location)?,
                                args,
                            )
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
                            // Symbol-table lookup returns the wrong function index for
                            // these stdlib namespace calls; route through the synthetic
                            // SYM_BUILTIN_LIST_SIZE so codegen finds list.size directly.
                            (SYM_BUILTIN_LIST_SIZE, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Array(element_type), "add") => {
                            // CRITICAL: list.add modifies IN-PLACE - use SymbolId(1007/1008)
                            let list_add_symbol = Self::list_add_symbol(element_type);
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                let arg_id = self.coerce_list_element_arg(
                                    context,
                                    arg_id,
                                    &arg.expr_type,
                                    element_type,
                                    &arg.location,
                                );
                                args.push(MirOperand::Value(arg_id));
                            }
                            (list_add_symbol, args)
                        }
                        (ConcreteType::Array(element_type), "push") => {
                            // list.push is an alias for list.add (in-place append)
                            let list_add_symbol = Self::list_add_symbol(element_type);
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                let arg_id = self.coerce_list_element_arg(
                                    context,
                                    arg_id,
                                    &arg.expr_type,
                                    element_type,
                                    &arg.location,
                                );
                                args.push(MirOperand::Value(arg_id));
                            }
                            (list_add_symbol, args)
                        }
                        (ConcreteType::Array(_), "setFlags") => {
                            // Store compile-time-known behavior flags at offset 12.
                            let set_flags_symbol =
                                self.builtin_symbol("list.setFlags", &expression.location)?;
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (set_flags_symbol, args)
                        }
                        (ConcreteType::Array(_), "get") => {
                            let list_get_symbol =
                                self.builtin_symbol("list_get", &expression.location)?;
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (list_get_symbol, args)
                        }
                        // `list<string>.contains(needle)` / `.indexOf(needle)` —
                        // dispatch to the byte-comparing variants when the
                        // element type is String. The default aliases
                        // (`list.contains` / `list.indexOf`) route to i32
                        // pointer-equality, which returns false for byte-
                        // identical strings allocated at different offsets.
                        // Fixes prompt baf3ccc5.
                        (ConcreteType::Array(element_type), "contains" | "indexOf")
                            if matches!(**element_type, ConcreteType::String) =>
                        {
                            let alias = if method_name == "contains" {
                                "list.contains.string"
                            } else {
                                "list.indexOf.string"
                            };
                            let str_symbol = self.builtin_symbol(alias, &expression.location)?;
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (str_symbol, args)
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
                    // User-defined method call. Look up the resolved method so we can
                    // (1) check `is_static` — utility-class methods that don't reference
                    //     `this` are lowered without a `this` parameter (see
                    //     type_inference.rs `is_static = !body_uses_this(...)`). For these,
                    //     the WASM signature has N params, but pushing the receiver here
                    //     would emit N+1 stack values. WASM `return` ignores the leftover
                    //     so it slips past validation at function top-level, but an
                    //     enclosing structured block (if/else/loop) requires the stack to
                    //     match the block type and fails with "values remaining on stack
                    //     at end of block" (CODEGEN-INVALID-WASM).
                    // (2) pull the declared parameter types for `any`-boxing.
                    let resolved_method: Option<&TastFunction> = if let ConcreteType::Class {
                        symbol_id: class_symbol,
                        ..
                    } = &receiver.expr_type
                    {
                        context
                            .all_classes
                            .iter()
                            .find(|c| c.symbol_id == *class_symbol)
                            .and_then(|class| {
                                trace!(
                                    class_name = %class.name,
                                    method_symbol = ?method_symbol,
                                    method_count = %class.methods.len(),
                                    "Looking for method in class"
                                );
                                class.methods.iter().find(|m| m.symbol_id == *method_symbol)
                            })
                    } else {
                        trace!(
                            receiver_type = ?receiver.expr_type,
                            "Receiver is not a Class type"
                        );
                        None
                    };

                    let method_is_static = resolved_method.map(|m| m.is_static).unwrap_or(false);
                    let method_param_types: Vec<ConcreteType> = resolved_method
                        .map(|method| {
                            trace!(
                                method_name = %method.name,
                                param_count = %method.parameters.len(),
                                is_static = method.is_static,
                                params = ?method.parameters.iter().map(|p| &p.param_type).collect::<Vec<_>>(),
                                "Found method with parameters"
                            );
                            method
                                .parameters
                                .iter()
                                .map(|p| p.param_type.clone())
                                .collect()
                        })
                        .unwrap_or_default();

                    // Only pass the receiver when the method actually accepts `this`.
                    // Side effects in the receiver expression already executed when we
                    // built `receiver_id` above; we just drop the resulting ValueId.
                    let mut args = if method_is_static {
                        Vec::new()
                    } else {
                        vec![MirOperand::Value(receiver_id)]
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
                                self.unbox_any_argument(
                                    context,
                                    arg_id,
                                    &arg.expr_type,
                                    Some(param_type),
                                    &arg.location,
                                )
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

                // list.add and pairs.set may move a full collection to a larger
                // block and return its (possibly new) address, which is written
                // back into the receiver after the call — so their result is
                // never discarded.
                let writes_back_list = Self::is_list_add_symbol(function_symbol)
                    || (matches!(receiver_actual_type, ConcreteType::Pairs(_, _))
                        && method_name == "set");
                let (is_void, result_type) = if writes_back_list {
                    (false, MirType::I32)
                } else {
                    (is_void, result_type)
                };

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
                if writes_back_list {
                    self.write_back_list_receiver(
                        context,
                        receiver,
                        result_id,
                        &expression.location,
                    )?;
                }
                Ok(result_id)
            }

            TastExpressionKind::StaticMethodCall {
                class_name,
                method_name,
                method_symbol,
                arguments,
                type_args: _,
            } => {
                // Build all arguments (a null receiver only for instance-lowered methods)
                // A static call on a program class (e.g. `Database.save(user)`)
                // takes the instance itself; only bridge/namespace calls get the
                // JSON-serialized form.
                let callee_is_program_class =
                    context.all_classes.iter().any(|c| c.name == *class_name);
                let mut mir_arguments = Vec::new();
                // Methods of a class with fields, a parent or capabilities are
                // lowered as instance methods (`this` first parameter, see
                // type_inference.rs `is_static`). A `ClassName.method()` call is
                // only valid for methods that don't use instance fields (spec
                // 15-classes-and-objects.md "Static Methods"), so it passes a
                // null receiver to fill that parameter; without it the call is
                // one argument short and the module fails WASM validation.
                //
                // The method symbol can still be `SymbolId(0)` for a program
                // class when the caller was resolved before the class's methods
                // were registered (top-level functions and plugin fragments
                // resolve first); bind it by class + method name in that case,
                // walking up the parent chain for inherited methods.
                let callee_method: Option<(SymbolId, bool, ConcreteType)> = if method_symbol.0 != 0
                {
                    context
                        .all_classes
                        .iter()
                        .flat_map(|c| c.methods.iter())
                        .find(|m| m.symbol_id == *method_symbol)
                        .map(|m| (m.symbol_id, m.is_static, m.return_type.clone()))
                } else {
                    let mut class = context.all_classes.iter().find(|c| c.name == *class_name);
                    let mut found = None;
                    while let Some(c) = class {
                        if let Some(m) = c.methods.iter().find(|m| m.name == *method_name) {
                            found = Some((m.symbol_id, m.is_static, m.return_type.clone()));
                            break;
                        }
                        class = c.parent_class.and_then(|parent| {
                            context.all_classes.iter().find(|p| p.symbol_id == parent)
                        });
                    }
                    found
                };
                let callee_symbol = callee_method.as_ref().map_or(*method_symbol, |m| m.0);
                let callee_takes_this = callee_method.as_ref().is_some_and(|m| !m.1);
                if callee_takes_this {
                    let receiver_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    self.register_temp_local(
                        context,
                        receiver_id,
                        MirType::I32,
                        expression.location.clone(),
                    );
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: Some(receiver_id),
                            operation: MirOperation::Copy {
                                source: MirOperand::Constant(MirConstant::Integer(0)),
                            },
                            location: expression.location.clone(),
                        },
                    );
                    mir_arguments.push(MirOperand::Value(receiver_id));
                }
                for arg in arguments {
                    let arg_id = self.build_expression(context, arg)?;
                    // Auto-serialize class instances to JSON strings for bridge/namespace calls.
                    let final_id = if callee_is_program_class {
                        arg_id
                    } else if matches!(arg.expr_type, ConcreteType::Class { .. }) {
                        self.emit_class_json_text(context, arg_id, &arg.expr_type, &arg.location)
                    } else {
                        arg_id
                    };
                    mir_arguments.push(MirOperand::Value(final_id));
                }

                // Create result
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Get return type from expression. A method bound by name above
                // supplies its declared return type when the call's inferred type
                // stayed Unknown (callee resolved after the caller).
                let inferred_type = match (&expression.expr_type, &callee_method) {
                    (ConcreteType::Unknown, Some(m)) => &m.2,
                    (t, _) => t,
                };
                let mir_return_type = self.convert_concrete_type(inferred_type);

                // Detect void static methods. Without this, calling a
                // user-class void method via `ClassName.method(args)` emits a
                // Call with `dest = Some(...)`; codegen then writes a
                // `local.set` after a no-result WASM call → stack underflow
                // at wasmparser validation ("expected i32 but nothing on
                // stack"). Mirrors the void detection in the MethodCall arm;
                // `Unknown` stays non-void because that's reserved for
                // namespace/return-type holes that DO produce a value.
                // Resolves: CODEGEN_STACK_REMAINING fp 82badd8d3d63.
                let is_void = !matches!(inferred_type, ConcreteType::Unknown)
                    && (matches!(inferred_type, ConcreteType::Null | ConcreteType::Undefined)
                        || matches!(mir_return_type, MirType::Void)
                        || matches!(&mir_return_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void)));

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
                    is_void = is_void,
                    "Static method call"
                );
                let function_operand = if callee_symbol.0 == 0 {
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
                        method_symbol = callee_symbol.0,
                        "Creating Function for static call"
                    );
                    MirOperand::Function(callee_symbol)
                };

                // For void static methods, skip dest so codegen doesn't emit a local.set
                let dest_opt = if is_void { None } else { Some(result_id) };

                // Emit Call instruction (receiver, if any, is already in mir_arguments)
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

                // Add Load result to locals, typed by the field's declaration
                // (it decides the width of the load), else by the expression.
                let declared_type = object_class_symbol.and_then(|class_symbol| {
                    self.field_declared_type(context, class_symbol, property_symbol)
                });
                let field_type = self
                    .convert_concrete_type(declared_type.as_ref().unwrap_or(&expression.expr_type));
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

                    // `map[key]` reads like `map.get(key)`. Treating the map as a
                    // list and the key pointer as an element index read memory
                    // past the map.
                    ConcreteType::Pairs(_, value_type) => {
                        let index_id =
                            self.pairs_key(context, index_id, &index.expr_type, &index.location)?;
                        let result_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;
                        let result_type = self.convert_concrete_type(value_type);
                        self.register_temp_local(
                            context,
                            result_id,
                            result_type,
                            expression.location.clone(),
                        );
                        self.add_instruction(
                            context,
                            MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::NamedFunction {
                                        name: "pairs.get".to_string(),
                                        symbol_id: SymbolId(0),
                                    },
                                    arguments: vec![
                                        MirOperand::Value(array_id),
                                        MirOperand::Value(index_id),
                                    ],
                                },
                                location: expression.location.clone(),
                            },
                        );
                        Ok(result_id)
                    }

                    // Regular array/matrix access uses GetElementPtr
                    _ => {
                        // Use GetElementPtr for array access
                        let result_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Determine the element type from the array expression type.
                        // We need this BEFORE registering the GEP result so that the
                        // pointer can carry the element type — codegen reads it to
                        // pick the right per-element stride (4 bytes for i32-class
                        // elements, 8 bytes for i64/f64). Hardcoding Ptr<I32> here
                        // makes lists of `number` (f64) decode every element after
                        // the first as garbage (RUNTIME_ITERATE_LIST_NUMBER_WRONG_LOAD).
                        let element_type = match &array.expr_type {
                            ConcreteType::Array(elem_type) => self.convert_concrete_type(elem_type),
                            ConcreteType::Matrix(elem_type) => {
                                // Matrix is 2D array, so element is 1D array
                                MirType::Ptr(Box::new(self.convert_concrete_type(elem_type)))
                            }
                            _ => MirType::I32, // Default fallback
                        };

                        // NOTE: Register the pointer result as a local
                        // GetElementPtr returns a pointer to the array element.
                        // The inner T encodes the element MIR type so that
                        // GetElementPtr codegen can pick the right stride.
                        self.register_temp_local(
                            context,
                            result_id,
                            MirType::Ptr(Box::new(element_type.clone())),
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
                fallback,
            } => {
                // `expr onError fallback` (14-error-handling.md): evaluate
                // `expr` in an `onError` region; when an error raised inside
                // it reaches the region, the value is `fallback`.
                let location = expression.location.clone();
                let result_type = self.convert_concrete_type(&expression.expr_type);
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(context, result_id, result_type, location.clone());

                self.begin_error_region(context, &location);
                let value_id = self.build_expression(context, expr)?;
                let value_id =
                    self.coerce_on_error_branch(context, value_id, expr, &expression.expr_type);
                self.add_instruction(
                    context,
                    MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Copy {
                            source: MirOperand::Value(value_id),
                        },
                        location: location.clone(),
                    },
                );
                self.end_error_region(context, &location)?;

                let took = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(context, took, MirType::I32, location.clone());
                self.emit_error_runtime_call(context, "__err_take", Some(took), &location);

                // if took: result = fallback
                let base = context.function.next_block_id;
                context.function.next_block_id += 2;
                let (fallback_block, merge) = (BasicBlockId(base), BasicBlockId(base + 1));
                for (id, label) in [(fallback_block, "then"), (merge, "continue")] {
                    context.function.blocks.insert(
                        id,
                        MirBasicBlock {
                            id,
                            label: Some(label.to_string()),
                            instructions: Vec::new(),
                            terminator: MirTerminator::Unreachable,
                            predecessors: HashSet::new(),
                            successors: HashSet::new(),
                            location: location.clone(),
                        },
                    );
                }
                if let Some(branch_block) = self.current_block {
                    context.function.if_merges.insert(branch_block, merge);
                }
                self.set_block_terminator(
                    context,
                    MirTerminator::Branch {
                        condition: MirOperand::Value(took),
                        true_block: fallback_block,
                        false_block: merge,
                    },
                );
                self.current_block = Some(fallback_block);
                let fallback_id = self.build_expression(context, fallback)?;
                let fallback_id = self.coerce_on_error_branch(
                    context,
                    fallback_id,
                    fallback,
                    &expression.expr_type,
                );
                self.add_instruction(
                    context,
                    MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Copy {
                            source: MirOperand::Value(fallback_id),
                        },
                        location: location.clone(),
                    },
                );
                self.set_block_terminator(context, MirTerminator::Jump { target: merge });
                self.current_block = Some(merge);
                Ok(result_id)
            }

            TastExpressionKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                // `if c then a else b`: only the chosen branch is evaluated.
                let condition_id = self.build_expression(context, condition)?;
                self.build_two_way_value(
                    context,
                    condition_id,
                    BranchArm::Lazy(then_expr),
                    BranchArm::Lazy(else_expr),
                    &expression.expr_type,
                    &expression.location,
                )
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

                // The list allocator's byte size is `16 + capacity * 4` — it assumes a
                // 4-byte element stride. When the element type is f64 (Number, 8-byte
                // stride), passing the literal element count under-allocates by exactly
                // half. `list.push_f64` then writes past the end of the allocation and
                // clobbers adjacent heap memory (visible as garbage past element 0 in
                // `json.encode(list<number>)`). Compensate by doubling the capacity hint
                // for f64-element literals so the allocated byte region covers the full
                // `count * 8` range. This is a localized workaround — a cleaner fix
                // would add a stride parameter to list.allocate or split into separate
                // i32/f64 allocators, but the byte budget is what matters at runtime.
                let element_is_f64 = matches!(
                    expression.expr_type,
                    ConcreteType::Array(ref t)
                        if matches!(**t, ConcreteType::Number | ConcreteType::IntegerSized { bits: 64, .. })
                );
                let list_element_type = match &expression.expr_type {
                    ConcreteType::Array(t) => Some((**t).clone()),
                    _ => None,
                };
                // Empty list literals (`[]`) must reserve some capacity or the
                // first `list.push`/`list.add` writes past the allocation and
                // clobbers the next heap allocation (bump-allocated adjacent
                // list, string buffer, etc.). `list.add` (behavior-aware) does
                // an in-place write at `list_ptr + 16 + size*4` with no growth
                // check; `list_ops.rs::generate_list_add`'s contract is that
                // the caller pre-allocates capacity. Reserve 8 slots by
                // default — matches the invariant documented in
                // `native_stdlib/list_ops.rs::gen_push_i32_inplace`. For
                // non-empty literals we already have `elements.len()` slots
                // which is sufficient for the initial fill.
                const EMPTY_LIST_MIN_CAPACITY: i64 = 8;
                let raw_capacity = if element_is_f64 {
                    (elements.len() * 2) as i64
                } else {
                    elements.len() as i64
                };
                let capacity_hint = if elements.is_empty() {
                    EMPTY_LIST_MIN_CAPACITY
                } else {
                    raw_capacity
                };

                // Create size constant
                let size_instruction = MirInstruction {
                    dest: Some(size_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::Integer(capacity_hint)),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, size_instruction);

                // Call list.allocate (for arrays) or pairs.new (for pairs) with the size hint.
                // Pairs literals use {} syntax and have a Pairs concrete type.
                let alloc_function = if matches!(&expression.expr_type, ConcreteType::Pairs(_, _)) {
                    MirOperand::NamedFunction {
                        name: "pairs.new".to_string(),
                        symbol_id: SYM_BUILTIN_LIST_ALLOCATE,
                    }
                } else {
                    MirOperand::Function(SYM_BUILTIN_LIST_ALLOCATE)
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

                    // Build the element expression, converted to the list's
                    // element type (`[1, 2]` as a `list<number>`).
                    let mut element_value_id = self.build_expression(context, element)?;
                    if let Some(list_element_type) = &list_element_type {
                        element_value_id = self.coerce_list_element_arg(
                            context,
                            element_value_id,
                            &element.expr_type,
                            list_element_type,
                            &element.location,
                        );
                    }

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
                            SYM_BUILTIN_LIST_PUSH_F64
                        }
                        MirType::I64 | MirType::U64 => SYM_BUILTIN_LIST_ADD_I64,
                        _ => {
                            trace!(element_index = idx, element_type = ?element_type, "Element using list.push");
                            SYM_BUILTIN_LIST_PUSH
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

                // When the expected type is `Pairs<K, V>`, the literal must materialize
                // the native pairs heap layout — `[count:i32][capacity:i32][key_ptr,val_ptr]…` —
                // so that `pairs.get`, `pairs.set`, and the rest of the typed-pairs API
                // can operate on it. This path produces a raw pairs pointer (not a
                // boxed Any) since the target variable has type Pairs.
                if matches!(&expression.expr_type, ConcreteType::Pairs(_, _)) {
                    let pairs_ptr_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;
                    self.register_temp_local(context, pairs_ptr_id, MirType::I32, loc.clone());

                    // Allocate via pairs.new(n) so the header carries the correct count=0
                    // and capacity=n. pairs.set will then bump the count as entries land.
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: Some(pairs_ptr_id),
                            operation: MirOperation::Call {
                                function: MirOperand::NamedFunction {
                                    name: "pairs.new".to_string(),
                                    symbol_id: SYM_BUILTIN_LIST_ALLOCATE,
                                },
                                arguments: vec![MirOperand::Constant(MirConstant::Integer(
                                    n as i64,
                                ))],
                            },
                            location: loc.clone(),
                        },
                    );

                    for field in fields {
                        let key_str_expr = TastExpression {
                            kind: TastExpressionKind::Literal {
                                value: TastLiteral::String(field.key.clone()),
                            },
                            expr_type: ConcreteType::String,
                            location: field.location.clone(),
                        };
                        let key_id = self.build_expression(context, &key_str_expr)?;
                        let val_id = self.build_expression(context, &field.value)?;

                        // pairs.set(map_ptr, key_ptr, val_ptr) — value is the raw V (i32
                        // representation for the user's V type). No boxing — pairs.get
                        // returns the raw V, and json.encode walks the pairs natively.
                        // It returns the map's (possibly moved) address.
                        self.add_instruction(
                            context,
                            MirInstruction {
                                dest: Some(pairs_ptr_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::NamedFunction {
                                        name: "pairs.set".to_string(),
                                        symbol_id: SYM_BUILTIN_LIST_ALLOCATE,
                                    },
                                    arguments: vec![
                                        MirOperand::Value(pairs_ptr_id),
                                        MirOperand::Value(key_id),
                                        MirOperand::Value(val_id),
                                    ],
                                },
                                location: field.location.clone(),
                            },
                        );
                    }

                    trace!(pairs_ptr_id = ?pairs_ptr_id, "Pairs literal created");
                    return Ok(pairs_ptr_id);
                }

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
                        function: MirOperand::Function(SYM_BUILTIN_LIST_ALLOCATE),
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
                        function: MirOperand::Function(SYM_BUILTIN_LIST_PUSH),
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

    /// Lower `a and b` / `a or b` with short-circuit semantics.
    ///
    /// Both operands are boolean. The right-hand operand only executes
    /// when the left-hand cannot already determine the result.
    ///
    /// The emitted shape is a single `MirOperation::LogicalShortCircuit`
    /// in the current basic block — the rhs instructions are captured
    /// into a side buffer and packed into the op so the codegen can emit
    /// them inside a WASM `if (result i32) ... else ... end`. Keeping
    /// the operation inside a single MIR block (rather than splitting
    /// into multiple blocks joined by a Phi) means downstream passes —
    /// including the while-loop codegen, which expects the loop
    /// header's `Branch` terminator to point straight at body/exit —
    /// see only one block and continue working unchanged.
    ///
    /// `and`:
    ///   load lhs ; if (result i32) [emit rhs_instructions] load rhs_value
    ///   else                       i32.const 0                            end
    /// `or`:
    ///   load lhs ; if (result i32) i32.const 1
    ///   else                       [emit rhs_instructions] load rhs_value end
    fn build_short_circuit_logical(
        &mut self,
        context: &mut FunctionBuildContext,
        operator: &BinaryOperator,
        left: &TastExpression,
        right: &TastExpression,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        debug_assert!(matches!(operator, BinaryOperator::And | BinaryOperator::Or));
        let is_and = matches!(operator, BinaryOperator::And);

        // Evaluate the left operand inline in the current block.
        let lhs_id = self.build_expression(context, left)?;

        // Capture the right-hand operand's MIR instructions without
        // leaving them in any reachable block. Use a temporary scratch
        // block, swap `current_block` to it, build rhs, then steal the
        // instructions and discard the scratch block. Any locals
        // allocated during rhs evaluation remain registered on the
        // function (their definitions live in `function.locals`), so
        // the codegen will allocate WASM locals for them normally.
        let saved_block = self
            .current_block
            .expect("short-circuit lowered without an active current_block");
        let scratch_block_id = BasicBlockId(context.function.next_block_id);
        context.function.next_block_id += 1;
        context.function.blocks.insert(
            scratch_block_id,
            MirBasicBlock {
                id: scratch_block_id,
                label: Some("logical_rhs_scratch".to_string()),
                instructions: Vec::new(),
                terminator: MirTerminator::Unreachable,
                predecessors: HashSet::new(),
                successors: HashSet::new(),
                location: location.clone(),
            },
        );
        self.current_block = Some(scratch_block_id);
        let rhs_id = self.build_expression(context, right)?;
        // If the rhs evaluation itself opened new control-flow blocks
        // (nested short-circuit chains compose without issue because
        // each one nests recursively; here we just need a single linear
        // scratch). For anything beyond a linear sequence the rhs
        // landed in a different block and we cannot inline-emit it.
        if self.current_block != Some(scratch_block_id) {
            return Err(vec![CompilerError::validation_error(
                "Right-hand operand of logical `and`/`or` introduced control flow \
                 that cannot be inlined into the short-circuit WASM if/else. \
                 Hoist the sub-expression to a local before the operator.",
                location.clone(),
            )]);
        }
        let scratch = context
            .function
            .blocks
            .remove(&scratch_block_id)
            .expect("scratch block disappeared during rhs build");
        let rhs_instructions = scratch.instructions;
        self.current_block = Some(saved_block);

        // Allocate the result local.
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, MirType::I32, location.clone());

        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result_id),
                operation: MirOperation::LogicalShortCircuit {
                    is_and,
                    lhs: MirOperand::Value(lhs_id),
                    rhs_instructions,
                    rhs_value: MirOperand::Value(rhs_id),
                },
                location: location.clone(),
            },
        );

        Ok(result_id)
    }
}
