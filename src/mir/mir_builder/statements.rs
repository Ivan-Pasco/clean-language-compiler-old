//! Statement lowering — lowers TAST statements to MIR instructions.

use super::*;

impl MirBuilder {
    pub(super) fn build_statement(
        &mut self,
        context: &mut FunctionBuildContext,
        statement: &TastStatement,
    ) -> Result<(), Vec<CompilerError>> {
        match statement {
            TastStatement::VariableDeclaration {
                symbol_id: _,
                name,
                var_type,
                initializer,
                is_mutable: _,
                location,
            } => {
                let value_id = if let Some(init_expr) = initializer {
                    // Type propagation: when the declared type is `Pairs<K, V>` and the
                    // initializer is an object literal (or list literal), the typechecker
                    // labels the literal's `expr_type` as `Any` rather than `Pairs<>`.
                    // Without propagating the declared type, ObjectLiteral codegen would
                    // produce the JSON-object layout (4-byte header) instead of the
                    // pairs layout (8-byte header with capacity), and `pairs.get/set`
                    // would scan the wrong offsets. Materialize a re-typed copy of the
                    // initializer so the literal codegen takes the typed-collection path.
                    let propagated;
                    let init_for_build = if matches!(var_type, ConcreteType::Pairs(_, _))
                        && matches!(
                            init_expr.kind,
                            TastExpressionKind::ObjectLiteral { .. }
                                | TastExpressionKind::ArrayLiteral { .. }
                        )
                        && !matches!(init_expr.expr_type, ConcreteType::Pairs(_, _))
                    {
                        propagated = TastExpression {
                            kind: init_expr.kind.clone(),
                            expr_type: var_type.clone(),
                            location: init_expr.location.clone(),
                        };
                        &propagated
                    } else {
                        init_expr
                    };

                    // Build initializer expression
                    let init_value_id = self.build_expression(context, init_for_build)?;

                    // NOTE: Check the ACTUAL MIR type of the initialized value,
                    // not just the typechecker's type. This is important because MIR builder
                    // may infer different types (e.g., list.get on Any receiver returns Any).
                    let actual_mir_type = context
                        .function
                        .locals
                        .get(&init_value_id)
                        .map(|local| local.local_type.clone());

                    // If the actual MIR type is Any, don't try to box - it's already Any format
                    let init_is_actually_any = matches!(actual_mir_type, Some(MirType::Any));

                    // Check if we need to unbox: initializer type is Any but variable type is not.
                    // After propagation above, `init_for_build.expr_type` may now match `var_type`
                    // (e.g. typed-pairs literal path) — in that case we have a raw value, not a
                    // boxed Any, so skip unboxing.
                    let needs_unboxing = (matches!(init_for_build.expr_type, ConcreteType::Any)
                        || init_is_actually_any)
                        && !matches!(var_type, ConcreteType::Any);

                    // Check if we need to box: variable type is Any but initializer is not Any
                    // NOTE: Also check actual MIR type - if already Any, no boxing needed
                    // NOTE 2: Don't box if init_expr.expr_type is Unknown and MIR type is Any
                    // This handles cases like json.tryTextToData() where TAST type may be Unknown
                    // but the actual function returns a boxed Any value
                    let needs_boxing = matches!(var_type, ConcreteType::Any)
                        && !matches!(
                            init_for_build.expr_type,
                            ConcreteType::Any | ConcreteType::Unknown
                        )
                        && !init_is_actually_any;

                    if needs_unboxing {
                        trace!(
                            var_name = %name,
                            init_type = ?init_expr.expr_type,
                            var_type = ?var_type,
                            "Unboxing any value to target type"
                        );
                        self.emit_unbox_any(context, init_value_id, var_type, location)
                    } else if needs_boxing {
                        trace!(
                            var_name = %name,
                            init_type = ?init_expr.expr_type,
                            var_type = ?var_type,
                            "Boxing value for any variable declaration"
                        );
                        self.emit_box_any(context, init_value_id, &init_expr.expr_type, location)
                    } else {
                        // NOTE: Check if we need type conversion between numeric types
                        // This handles cases like: integer y = math.abs(x) where math.abs returns f64
                        // but y is declared as integer (i32)
                        let declared_mir_type = self.convert_concrete_type(var_type);
                        let init_mir_type = actual_mir_type
                            .clone()
                            .unwrap_or_else(|| declared_mir_type.clone());

                        // Check if types differ and we need conversion
                        let needs_type_conversion = matches!(
                            (&init_mir_type, &declared_mir_type),
                            (MirType::F64, MirType::I32) | (MirType::I32, MirType::F64)
                        );

                        if needs_type_conversion {
                            trace!(
                                var_name = %name,
                                init_mir_type = ?init_mir_type,
                                declared_mir_type = ?declared_mir_type,
                                "Emitting type conversion for variable declaration"
                            );

                            // Create a new ValueId for the converted value
                            let converted_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the converted value with the DECLARED type
                            self.register_temp_local(
                                context,
                                converted_id,
                                declared_mir_type.clone(),
                                location.clone(),
                            );

                            // Emit Cast instruction for type conversion
                            let convert_instruction = MirInstruction {
                                dest: Some(converted_id),
                                operation: MirOperation::Cast {
                                    value: MirOperand::Value(init_value_id),
                                    target_type: declared_mir_type,
                                },
                                location: location.clone(),
                            };
                            self.add_instruction(context, convert_instruction);

                            converted_id
                        } else {
                            // NOTE: If the initializer ValueId is already in use as a
                            // parameter or another local variable, we must allocate a fresh
                            // ValueId for this new variable and emit a Copy instruction.
                            //
                            // Without this fix, `string remaining = content` would map `remaining`
                            // and `content` to the same ValueId → same WASM local. Then mutations
                            // to `remaining` would corrupt `content`, and `remaining = content`
                            // later becomes a no-op (same ValueId, self-copy is skipped).
                            let init_already_allocated =
                                context.function.locals.contains_key(&init_value_id)
                                    || context
                                        .function
                                        .parameters
                                        .iter()
                                        .any(|p| p.value_id == init_value_id);

                            if init_already_allocated {
                                // Allocate a fresh ValueId for the new variable
                                let copy_id = ValueId(context.function.next_value_id);
                                context.function.next_value_id += 1;

                                // Determine the type for the new local
                                let copy_type = actual_mir_type
                                    .clone()
                                    .unwrap_or_else(|| self.convert_concrete_type(var_type));

                                // Register the new local
                                let local = MirLocal {
                                    name: Some(name.clone()),
                                    local_type: copy_type,
                                    is_mutable: true,
                                    location: location.clone(),
                                };
                                context.function.locals.insert(copy_id, local);

                                // Emit Copy instruction so the new local gets the value
                                let copy_instruction = MirInstruction {
                                    dest: Some(copy_id),
                                    operation: MirOperation::Copy {
                                        source: MirOperand::Value(init_value_id),
                                    },
                                    location: location.clone(),
                                };
                                self.add_instruction(context, copy_instruction);

                                copy_id
                            } else {
                                init_value_id
                            }
                        }
                    }
                } else {
                    // Check if this is an Array type (list) - if so, allocate an empty list
                    match var_type {
                        ConcreteType::Array(_) => {
                            // Allocate an empty list using list.allocate
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Create local for the list pointer
                            let local = MirLocal {
                                name: Some(name.clone()),
                                local_type: MirType::I32, // Lists are pointers
                                is_mutable: true,
                                location: location.clone(),
                            };
                            context.function.locals.insert(result_id, local);

                            // Create size argument (0 for empty list)
                            let size_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            let size_local = MirLocal {
                                name: None,
                                local_type: MirType::I32,
                                is_mutable: false,
                                location: location.clone(),
                            };
                            context.function.locals.insert(size_id, size_local);

                            // Create size constant with default capacity of 8
                            // NOTE: Allocating with capacity 0 causes memory corruption
                            // because list.push writes beyond allocated space, and subsequent
                            // allocations overwrite the list elements.
                            let size_instruction = MirInstruction {
                                dest: Some(size_id),
                                operation: MirOperation::Copy {
                                    source: MirOperand::Constant(MirConstant::Integer(8)),
                                },
                                location: location.clone(),
                            };
                            self.add_instruction(context, size_instruction);

                            // Call list.allocate(8) via the synthetic builtin symbol.
                            let call_instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(SYM_BUILTIN_LIST_ALLOCATE),
                                    arguments: vec![MirOperand::Value(size_id)],
                                },
                                location: location.clone(),
                            };
                            self.add_instruction(context, call_instruction);

                            result_id
                        }
                        _ => {
                            // Create uninitialized value for non-list types
                            let value_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Add undefined constant instruction
                            let instruction = MirInstruction {
                                dest: Some(value_id),
                                operation: MirOperation::Copy {
                                    source: MirOperand::Constant(MirConstant::Undefined),
                                },
                                location: location.clone(),
                            };

                            self.add_instruction(context, instruction);
                            value_id
                        }
                    }
                };

                // Add to current scope
                if let Some(current_scope) = context.scope_stack.last_mut() {
                    current_scope.insert(name.clone(), value_id);
                }

                // Create local variable entry (skip for Array without initializer - already created)
                // NOTE: Don't overwrite if local already exists from initializer expression
                // The initializer's type may be more specific (e.g., F64 for float literals)
                let should_create_local =
                    !(initializer.is_none() && matches!(var_type, ConcreteType::Array(_)));
                if should_create_local && !context.function.locals.contains_key(&value_id) {
                    let local = MirLocal {
                        name: Some(name.clone()),
                        local_type: self.convert_concrete_type(var_type),
                        is_mutable: true, // All Clean Language variables are mutable by default
                        location: location.clone(),
                    };

                    context.function.locals.insert(value_id, local);
                } else if should_create_local {
                    // Local already exists from initializer - just update the name and mutability
                    if let Some(local) = context.function.locals.get_mut(&value_id) {
                        local.name = Some(name.clone());
                        local.is_mutable = true;
                    }
                }
            }

            TastStatement::Assignment {
                target,
                value,
                location,
            } => {
                // Build value expression
                let value_id = self.build_expression(context, value)?;

                // Handle assignment target - for now, only support simple variable assignments
                match &target.kind {
                    TastExpressionKind::Variable { symbol_id: _, name } => {
                        // NOTE: Check the ACTUAL MIR type of the value,
                        // not just the typechecker's type. This is important because MIR builder
                        // may infer different types (e.g., list.get on Any receiver returns Any).
                        let actual_mir_type = context
                            .function
                            .locals
                            .get(&value_id)
                            .map(|local| local.local_type.clone());

                        // If the actual MIR type is Any, don't try to box - it's already Any format
                        let value_is_actually_any = matches!(actual_mir_type, Some(MirType::Any));

                        // Check if we need to box: target type is Any but value type is not Any
                        // NOTE: Also check actual MIR type - if already Any, no boxing needed
                        let final_value_id = if matches!(target.expr_type, ConcreteType::Any)
                            && !matches!(value.expr_type, ConcreteType::Any)
                            && !value_is_actually_any
                        {
                            trace!(
                                var_name = %name,
                                value_type = ?value.expr_type,
                                target_type = ?target.expr_type,
                                "Boxing value for any variable assignment"
                            );
                            self.emit_box_any(context, value_id, &value.expr_type, &value.location)
                        } else {
                            value_id
                        };

                        // Check if this is a state variable assignment - emit GlobalStore
                        if let Some((symbol_id, mir_type)) = self.state_variables.get(name).cloned()
                        {
                            // Inject guard check before the assignment if this variable has a guard
                            if let Some((guard_condition, _value_symbol_id, _error_msg)) =
                                self.state_guards.get(name).cloned()
                            {
                                trace!(
                                    var_name = %name,
                                    "Injecting guard check before state variable assignment"
                                );
                                // Bind the proposed new value as 'value' in a new scope
                                let mut guard_scope = HashMap::new();
                                guard_scope.insert("value".to_string(), final_value_id);
                                context.scope_stack.push(guard_scope);

                                // Evaluate the guard condition
                                let condition_value =
                                    self.build_expression(context, &guard_condition)?;

                                // Remove the guard scope
                                context.scope_stack.pop();

                                // Create trap and continue blocks
                                let base_block_id = context.function.next_block_id;
                                let trap_block_id = BasicBlockId(base_block_id);
                                let continue_block_id = BasicBlockId(base_block_id + 1);
                                context.function.next_block_id = base_block_id + 2;

                                context.function.blocks.insert(
                                    trap_block_id,
                                    MirBasicBlock {
                                        id: trap_block_id,
                                        label: Some(format!("guard_{}_trap", name)),
                                        instructions: Vec::new(),
                                        terminator: MirTerminator::Trap,
                                        predecessors: HashSet::new(),
                                        successors: HashSet::new(),
                                        location: location.clone(),
                                    },
                                );

                                context.function.blocks.insert(
                                    continue_block_id,
                                    MirBasicBlock {
                                        id: continue_block_id,
                                        label: Some(format!("guard_{}_continue", name)),
                                        instructions: Vec::new(),
                                        terminator: MirTerminator::Unreachable,
                                        predecessors: HashSet::new(),
                                        successors: HashSet::new(),
                                        location: location.clone(),
                                    },
                                );

                                // Branch: if guard is true → continue, else → trap
                                self.set_block_terminator(
                                    context,
                                    MirTerminator::Branch {
                                        condition: MirOperand::Value(condition_value),
                                        true_block: continue_block_id,
                                        false_block: trap_block_id,
                                    },
                                );

                                // Continue execution in the continue block
                                self.current_block = Some(continue_block_id);
                            }

                            trace!(
                                var_name = %name,
                                symbol_id = ?symbol_id,
                                mir_type = ?mir_type,
                                "Emitting GlobalStore for state variable assignment"
                            );
                            let store_instruction = MirInstruction {
                                dest: None,
                                operation: MirOperation::GlobalStore {
                                    global_id: symbol_id,
                                    value: MirOperand::Value(final_value_id),
                                    global_type: mir_type,
                                },
                                location: location.clone(),
                            };
                            self.add_instruction(context, store_instruction);

                            // After the store, call any watch handlers registered for this
                            // state variable.  Watch handlers take no arguments and return
                            // void — they observe the new value by reading the global directly.
                            let watch_symbol_ids: Vec<SymbolId> =
                                self.watch_handlers.get(name).cloned().unwrap_or_default();

                            for handler_symbol_id in watch_symbol_ids {
                                trace!(
                                    var_name = %name,
                                    handler_symbol_id = handler_symbol_id.0,
                                    "Emitting watch handler call after state mutation"
                                );
                                let watch_call = MirInstruction {
                                    dest: None,
                                    operation: MirOperation::Call {
                                        function: MirOperand::Function(handler_symbol_id),
                                        arguments: Vec::new(),
                                    },
                                    location: location.clone(),
                                };
                                self.add_instruction(context, watch_call);
                            }
                        } else {
                            // NOTE: For re-assignment of existing local variables, we need to emit
                            // a Copy instruction to actually update the variable's value in the WASM local.
                            // Look up the original ValueId for this variable and emit a Copy to it.
                            let original_value_id = context
                                .scope_stack
                                .iter()
                                .rev()
                                .find_map(|scope| scope.get(name).copied());

                            if let Some(orig_id) = original_value_id {
                                // This is a re-assignment - emit Copy instruction to update the variable
                                // Only if the value IDs are different (avoid self-copy)
                                if orig_id != final_value_id {
                                    trace!(
                                        var_name = %name,
                                        orig_id = orig_id.0,
                                        new_value_id = final_value_id.0,
                                        "Emitting Copy for variable re-assignment"
                                    );
                                    let copy_instruction = MirInstruction {
                                        dest: Some(orig_id),
                                        operation: MirOperation::Copy {
                                            source: MirOperand::Value(final_value_id),
                                        },
                                        location: location.clone(),
                                    };
                                    self.add_instruction(context, copy_instruction);
                                }
                            }

                            // Update variable in current scope (for SSA-style tracking)
                            // Keep the original ValueId since we're copying to it
                            // This ensures subsequent uses of 'name' still refer to the original local
                            // (which now has the updated value)
                        }
                    }
                    TastExpressionKind::PropertyAccess {
                        object,
                        property_name,
                        property_symbol,
                    } => {
                        // Handle field assignments like obj.field = value or this.field = value
                        // Build the object expression first
                        let object_id = self.build_expression(context, object)?;

                        // Check if field type is 'any' and value type is not 'any' - need boxing
                        // The target.expr_type tells us the field type
                        let final_value_id = if matches!(target.expr_type, ConcreteType::Any)
                            && !matches!(value.expr_type, ConcreteType::Any)
                        {
                            // Box the value before storing
                            self.emit_box_any(context, value_id, &value.expr_type, &value.location)
                        } else {
                            value_id
                        };

                        // Get the class from the object's type
                        let object_class_symbol = match &object.expr_type {
                            ConcreteType::Class { symbol_id, .. } => Some(*symbol_id),
                            _ => None,
                        };

                        // Find the byte offset of the field in the class hierarchy
                        let field_byte_offset = if let Some(class_symbol) = object_class_symbol {
                            // Calculate byte offset accounting for different field sizes
                            self.calculate_field_byte_offset(context, class_symbol, property_symbol)
                                .ok_or_else(|| {
                                    vec![CompilerError::validation_error(
                                        format!(
                                            "Field '{}' not found in class or parent classes",
                                            property_name
                                        ),
                                        target.location.clone(),
                                    )]
                                })? as i64
                        } else {
                            // Object doesn't have a class type - this shouldn't happen for field access
                            return Err(vec![CompilerError::validation_error(
                                format!(
                                    "Cannot assign to field '{}' on non-class type: {:?}",
                                    property_name, object.expr_type
                                ),
                                target.location.clone(),
                            )]);
                        };

                        // Generate GetElementPtr to get the field address
                        let field_ptr_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Add GetElementPtr result to locals
                        let gep_local = MirLocal {
                            name: None,               // Temporary value
                            local_type: MirType::I32, // Pointer type
                            is_mutable: false,
                            location: target.location.clone(),
                        };
                        context.function.locals.insert(field_ptr_id, gep_local);

                        let field_offset =
                            MirOperand::Constant(MirConstant::Integer(field_byte_offset));
                        let gep_instruction = MirInstruction {
                            dest: Some(field_ptr_id),
                            operation: MirOperation::GetElementPtr {
                                base: MirOperand::Value(object_id),
                                indices: vec![field_offset],
                                is_array: false, // Class field access - byte offset, no header
                            },
                            location: target.location.clone(),
                        };
                        self.add_instruction(context, gep_instruction);

                        // Generate Store instruction to write the value to the field
                        let store_instruction = MirInstruction {
                            dest: None, // Store doesn't produce a value
                            operation: MirOperation::Store {
                                destination: MirOperand::Value(field_ptr_id),
                                value: MirOperand::Value(final_value_id),
                            },
                            location: target.location.clone(),
                        };
                        self.add_instruction(context, store_instruction);

                        // Also update the scope for simple field references (for optimization)
                        if let Some(current_scope) = context.scope_stack.last_mut() {
                            current_scope.insert(property_name.clone(), final_value_id);
                        }
                    }
                    TastExpressionKind::ArrayAccess { array, index } => {
                        // Handle array index assignments like arr[i] = value
                        let _array_value = self.build_expression(context, array)?;
                        let _index_value = self.build_expression(context, index)?;

                        // Array element assignment handled at runtime via list_set
                        tracing::warn!("Array index assignment not fully implemented");
                    }
                    _ => {
                        // Handle any other complex assignment targets
                        tracing::warn!(
                            target_kind = ?target.kind,
                            "Complex assignment target not implemented"
                        );
                        // Don't error out, just ignore the assignment for now
                    }
                }
            }

            TastStatement::Expression { expression, .. } => {
                // Build expression for side effects
                // For void expressions, the ValueId won't be used, but we still need to
                // generate the instructions for side effects (like method calls)

                let _value_id = self.build_expression(context, expression)?;

                // Note: For void expressions (ConcreteType::Null), the ValueId is created
                // but never registered in function.locals or used. This is intentional -
                // the expression is executed for its side effects only.
            }

            TastStatement::Return {
                value,
                return_type: _,
                location,
            } => {
                trace!(
                    function_name = %context.function.name,
                    current_block = ?self.current_block,
                    "Processing return statement"
                );

                // Return type validation already done in type checking phase
                let return_value = if let Some(expr) = value {
                    let mut value_id = self.build_expression(context, expr)?;
                    trace!(value_id = ?value_id, "Built return expression");

                    // If the function returns `any` but the expression has a
                    // concrete type, box the value with the appropriate type
                    // tag. Mirrors the local-assignment path (`any v = expr`)
                    // so that `json.encode`, `any_to_string`, and the rest of
                    // the runtime see a tagged box rather than a raw pointer.
                    //
                    // Note: the `return_type` field on TastStatement::Return is
                    // the *expression*'s type (per type_inference.rs:2557), not
                    // the function's declared return type. The function's
                    // declared return type lives on `context.function.return_type`.
                    //
                    // `null` is excluded from boxing: the language contract is
                    // that `null` round-trips as a raw 0 at the WASM boundary
                    // so callers can write `if value != 0` against an any-
                    // returning function. Boxing null produced a non-zero
                    // heap pointer and silently broke every auto-generated
                    // guard handler in frame.ui (page-render returned 200 +
                    // empty body — CODEGEN-ANY-RETURN-NULL-BOXED-AS-NONZERO-BREAKS-GUARD-CHECK).
                    if matches!(context.function.return_type, MirType::Any)
                        && !matches!(expr.expr_type, ConcreteType::Any | ConcreteType::Null)
                    {
                        value_id =
                            self.emit_box_any(context, value_id, &expr.expr_type, &expr.location);
                    }

                    Some(MirOperand::Value(value_id))
                } else {
                    trace!("Void return (no value)");
                    None
                };

                // If the function has any `ensure` postconditions (or class invariants),
                // inject the checks before the return:
                //   1. Store the return value under the name `result` in the current scope.
                //   2. Evaluate each ensure condition (which may reference `result`).
                //   3. Trap if any condition is false.
                //   4. Return the stored value.
                if !context.ensure_conditions.is_empty() {
                    if let Some(MirOperand::Value(result_value_id)) = return_value {
                        // Bind `result` in current scope so ensure conditions can reference it
                        if let Some(scope) = context.scope_stack.last_mut() {
                            scope.insert("result".to_string(), result_value_id);
                        }

                        // Emit a check for each ensure condition
                        let ensure_exprs: Vec<TastExpression> = context.ensure_conditions.clone();
                        for ensure_expr in &ensure_exprs {
                            let cond_value_id = self.build_expression(context, ensure_expr)?;

                            // Allocate trap and continue blocks
                            let base_block_id = context.function.next_block_id;
                            let trap_block_id = BasicBlockId(base_block_id);
                            let continue_block_id = BasicBlockId(base_block_id + 1);
                            context.function.next_block_id = base_block_id + 2;

                            context.function.blocks.insert(
                                trap_block_id,
                                MirBasicBlock {
                                    id: trap_block_id,
                                    label: Some("ensure_trap".to_string()),
                                    instructions: Vec::new(),
                                    terminator: MirTerminator::Trap,
                                    predecessors: HashSet::new(),
                                    successors: HashSet::new(),
                                    location: location.clone(),
                                },
                            );

                            context.function.blocks.insert(
                                continue_block_id,
                                MirBasicBlock {
                                    id: continue_block_id,
                                    label: Some("ensure_continue".to_string()),
                                    instructions: Vec::new(),
                                    terminator: MirTerminator::Unreachable,
                                    predecessors: HashSet::new(),
                                    successors: HashSet::new(),
                                    location: location.clone(),
                                },
                            );

                            // Branch: condition true → continue, false → trap
                            self.set_block_terminator(
                                context,
                                MirTerminator::Branch {
                                    condition: MirOperand::Value(cond_value_id),
                                    true_block: continue_block_id,
                                    false_block: trap_block_id,
                                },
                            );

                            // Continue emitting in the continue block
                            self.current_block = Some(continue_block_id);
                        }

                        // Remove `result` from scope so it doesn't leak
                        if let Some(scope) = context.scope_stack.last_mut() {
                            scope.remove("result");
                        }
                    }
                }

                // Create return terminator
                let terminator = MirTerminator::Return {
                    value: return_value.clone(),
                };
                trace!(terminator = ?terminator, "Setting return terminator");
                self.set_block_terminator(context, terminator);
                trace!(current_block = ?self.current_block, "After return terminator");
            }

            // Ensure statement — collect postcondition for injection before returns.
            // The actual condition evaluation happens inside the Return handler above.
            // This statement simply records the condition for deferred checking.
            TastStatement::Ensure { condition, .. } => {
                context.ensure_conditions.push(condition.clone());
            }

            TastStatement::Print {
                expression,
                newline,
                location,
            } => {
                // Build the expression to print
                let value_id = self.build_expression(context, expression)?;

                // AUTO-CONVERSION: Convert non-string types to string before printing
                use crate::typechecker::tast::ConcreteType;
                let string_value_id = match &expression.expr_type {
                    ConcreteType::String => {
                        // Already a string, use directly
                        value_id
                    }
                    ConcreteType::Integer => {
                        // Convert integer to string using int_to_string
                        let converted_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Register the converted_id as Ptr(U8) — int_to_string returns a string
                        // pointer, not an integer. Using I32 here causes load_string_argument_for_print
                        // to call int_to_string AGAIN on the pointer address, producing garbage output.
                        self.register_temp_local(
                            context,
                            converted_id,
                            MirType::Ptr(Box::new(MirType::U8)),
                            location.clone(),
                        );

                        let symbol_id = self
                            .symbol_table
                            .lookup_symbol("int_to_string")
                            .unwrap_or_else(|| {
                                warn!(
                                    "int_to_string not found in symbol table, using SymbolId(166)"
                                );
                                SymbolId(166)
                            });

                        let conversion_instruction = MirInstruction {
                            dest: Some(converted_id),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(symbol_id),
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    ConcreteType::Number => {
                        // Convert float to string using float_to_string
                        let converted_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Register as Ptr(U8) - float_to_string returns a string pointer
                        self.register_temp_local(
                            context,
                            converted_id,
                            MirType::Ptr(Box::new(MirType::U8)),
                            location.clone(),
                        );

                        let symbol_id = self.symbol_table.lookup_symbol("float_to_string")
                            .unwrap_or_else(|| {
                                warn!("float_to_string not found in symbol table, using SymbolId(167)");
                                SymbolId(167)
                            });

                        let conversion_instruction = MirInstruction {
                            dest: Some(converted_id),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(symbol_id),
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    ConcreteType::Boolean => {
                        // Convert boolean to string using bool_to_string
                        let converted_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Register the converted_id as Ptr(U8) — bool_to_string returns a string
                        // pointer, not an integer. Using I32 here causes load_string_argument_for_print
                        // to call int_to_string on the pointer address, producing garbage output.
                        self.register_temp_local(
                            context,
                            converted_id,
                            MirType::Ptr(Box::new(MirType::U8)),
                            location.clone(),
                        );

                        let symbol_id = self
                            .symbol_table
                            .lookup_symbol("bool_to_string")
                            .unwrap_or_else(|| {
                                warn!(
                                    "bool_to_string not found in symbol table, using SymbolId(165)"
                                );
                                SymbolId(165)
                            });

                        let conversion_instruction = MirInstruction {
                            dest: Some(converted_id),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(symbol_id),
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    _ => {
                        // For other types (objects, arrays, etc.), use the value as-is for now
                        // In a complete implementation, these would also have toString() methods
                        value_id
                    }
                };

                // NOTE: Use correct function based on newline flag
                // SymbolId(0) = print (no newline), SymbolId(1) = printl (with newline)
                let function_symbol = if *newline {
                    SymbolId(1) // printl
                } else {
                    SymbolId(0) // print
                };

                let instruction = MirInstruction {
                    dest: None, // Print doesn't return a value
                    operation: MirOperation::Call {
                        function: MirOperand::Function(function_symbol),
                        arguments: vec![MirOperand::Value(string_value_id)],
                    },
                    location: location.clone(),
                };

                self.add_instruction(context, instruction);
            }

            TastStatement::If {
                condition,
                then_block,
                else_block,
                result_type: _,
                location,
            } => {
                // Build condition expression
                let condition_id = self.build_expression(context, condition)?;

                // NOTE: Use next_block_id counter instead of blocks.len()
                // This prevents block ID collisions when nested statements create new blocks.
                // The issue is that blocks.len() can be incorrect when blocks are pre-allocated
                // (like the continue block) and nested control flow tries to allocate more blocks.
                let base_block_id = context.function.next_block_id;
                let then_block_id = BasicBlockId(base_block_id);
                let else_block_id = if else_block.is_some() {
                    Some(BasicBlockId(base_block_id + 1))
                } else {
                    None
                };
                let continue_block_id =
                    BasicBlockId(base_block_id + if else_block.is_some() { 2 } else { 1 });
                // Update the counter to reserve all these block IDs
                context.function.next_block_id =
                    base_block_id + if else_block.is_some() { 3 } else { 2 };

                // Pre-allocate continue block to reserve its ID
                context.function.blocks.insert(
                    continue_block_id,
                    MirBasicBlock {
                        id: continue_block_id,
                        label: Some("continue".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable, // Will be replaced
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );

                // Create conditional branch in current block
                let branch = if let Some(else_id) = else_block_id {
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_id),
                        true_block: then_block_id,
                        false_block: else_id,
                    }
                } else {
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_id),
                        true_block: then_block_id,
                        false_block: continue_block_id,
                    }
                };
                self.set_block_terminator(context, branch);

                // Build then block
                context.function.blocks.insert(
                    then_block_id,
                    MirBasicBlock {
                        id: then_block_id,
                        label: Some("then".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable, // Will be replaced
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(then_block_id);

                // Process then block statements
                for stmt in &then_block.statements {
                    self.build_statement(context, stmt)?;
                }

                // NOTE: Check the THEN BLOCK's terminator, not current_block
                // After processing nested statements (like nested If), current_block may point
                // to a different block. We need to check the then block we just built.
                let then_terminator = context
                    .function
                    .blocks
                    .get(&then_block_id)
                    .map(|b| &b.terminator);

                trace!(
                    then_block_id = ?then_block_id,
                    terminator = ?then_terminator,
                    "Then block check"
                );

                // FIX: Check if block effectively returns (direct Return OR Branch where both branches return)
                // Unreachable here is just a placeholder that should be replaced with Jump
                let has_return = self.block_effectively_returns(context, then_block_id);
                trace!(
                    then_block_id = ?then_block_id,
                    has_return = has_return,
                    "Then block effective return check"
                );

                // NOTE: Handle Jump to continuation for nested control flow.
                //
                // Two cases to handle:
                // 1. Simple then block (no nested control flow): then_block_id needs Jump if it doesn't return
                // 2. Nested control flow: current_block after processing is NOT then_block_id, and THAT
                //    block needs Jump to the OUTER continuation
                //
                // We need to add Jump on blocks that have Unreachable placeholder terminators.
                // We should NOT overwrite meaningful terminators (Branch, Return, Jump).

                // Check if then_block_id itself needs a Jump (simple case, no nested control flow)
                let then_has_meaningful_terminator = context
                    .function
                    .blocks
                    .get(&then_block_id)
                    .map(|b| {
                        matches!(
                            b.terminator,
                            MirTerminator::Return { .. }
                                | MirTerminator::Branch { .. }
                                | MirTerminator::Jump { .. }
                        )
                    })
                    .unwrap_or(false);

                // Get the actual exit block (might be different from then_block_id if nested control flow)
                let actual_exit_block = self.current_block;

                if !has_return {
                    // Need to ensure proper control flow to continuation

                    // Case 1: Simple then block - add Jump on then_block_id
                    if !then_has_meaningful_terminator {
                        trace!("Adding Jump to continue block from then branch (simple case)");
                        let saved_current = self.current_block;
                        self.current_block = Some(then_block_id);
                        self.set_block_terminator(
                            context,
                            MirTerminator::Jump {
                                target: continue_block_id,
                            },
                        );
                        self.current_block = saved_current;
                    }

                    // Case 2: Nested control flow - add Jump on the exit block if it needs one
                    if let Some(exit_block_id) = actual_exit_block {
                        if exit_block_id != then_block_id {
                            // Nested control flow happened - check if exit block needs Jump
                            let exit_has_meaningful_terminator = context
                                .function
                                .blocks
                                .get(&exit_block_id)
                                .map(|b| {
                                    matches!(
                                        b.terminator,
                                        MirTerminator::Return { .. }
                                            | MirTerminator::Branch { .. }
                                            | MirTerminator::Jump { .. }
                                    )
                                })
                                .unwrap_or(false);

                            if !exit_has_meaningful_terminator {
                                trace!(
                                    exit_block_id = ?exit_block_id,
                                    "Adding Jump to continue block from nested control flow exit"
                                );
                                self.current_block = Some(exit_block_id);
                                self.set_block_terminator(
                                    context,
                                    MirTerminator::Jump {
                                        target: continue_block_id,
                                    },
                                );
                            }
                        }
                    }
                } else {
                    trace!(
                        then_block_id = ?then_block_id,
                        has_return = has_return,
                        "Then branch effectively returns, no Jump needed"
                    );
                }

                // Track whether the else branch returns (all paths)
                let else_returns_all_paths = if let Some(else_stmt_block) = else_block {
                    // Safe: else_block_id is always Some when else_block is Some (set together above)
                    let else_id =
                        else_block_id.expect("else_block_id must be set when else_block exists");
                    context.function.blocks.insert(
                        else_id,
                        MirBasicBlock {
                            id: else_id,
                            label: Some("else".to_string()),
                            instructions: Vec::new(),
                            terminator: MirTerminator::Unreachable, // Will be replaced
                            predecessors: HashSet::new(),
                            successors: HashSet::new(),
                            location: location.clone(),
                        },
                    );
                    self.current_block = Some(else_id);

                    // Save current_block before processing else block
                    let before_else = self.current_block;

                    // Process else block statements
                    for stmt in &else_stmt_block.statements {
                        self.build_statement(context, stmt)?;
                    }

                    // NOTE: Check if current_block is None after processing else block
                    // If current_block is None, it means all paths in the else block returned
                    // This handles nested if-else-if chains correctly
                    let after_else = self.current_block;

                    // Check if all paths return:
                    // 1. current_block is None (nested if set it to None because both branches returned)
                    // 2. OR current_block points to a block with a Return terminator
                    // FIX: Only Return counts, Unreachable is just a placeholder
                    let else_returns = if after_else.is_none() {
                        true
                    } else if let Some(final_block_id) = after_else {
                        context
                            .function
                            .blocks
                            .get(&final_block_id)
                            .map(|b| matches!(b.terminator, MirTerminator::Return { .. }))
                            .unwrap_or(false)
                    } else {
                        false
                    };

                    trace!(
                        before_else = ?before_else,
                        after_else = ?after_else,
                        else_returns = else_returns,
                        "Else return check"
                    );

                    if !else_returns {
                        // At least one path doesn't return - add jump to continuation
                        // BUT: Only if the block doesn't already have a meaningful terminator
                        let saved_current = self.current_block;
                        if let Some(curr) = saved_current {
                            // NOTE: Check if the block already has a meaningful terminator
                            // Include Jump in the check - an existing Jump is meaningful
                            let has_meaningful_terminator = context
                                .function
                                .blocks
                                .get(&curr)
                                .map(|b| {
                                    matches!(
                                        b.terminator,
                                        MirTerminator::Return { .. }
                                            | MirTerminator::Branch { .. }
                                            | MirTerminator::Jump { .. }
                                    )
                                })
                                .unwrap_or(false);

                            if !has_meaningful_terminator {
                                self.current_block = Some(curr);
                                self.set_block_terminator(
                                    context,
                                    MirTerminator::Jump {
                                        target: continue_block_id,
                                    },
                                );
                            } else {
                                trace!(
                                    block_id = ?curr,
                                    "Else block already has proper terminator, skipping Jump"
                                );
                            }
                        }
                        self.current_block = saved_current;
                    } else {
                        trace!("Else branch: all paths return");
                    }

                    else_returns
                } else {
                    false
                };

                // Continue block was already created above to reserve its ID
                // No need to create it again here

                // Check if both branches have return terminators
                let then_has_return = context
                    .function
                    .blocks
                    .get(&then_block_id)
                    .map(|b| matches!(b.terminator, MirTerminator::Return { .. }))
                    .unwrap_or(false);

                trace!(
                    then_has_return = then_has_return,
                    else_returns_all_paths = else_returns_all_paths,
                    current_block = ?self.current_block,
                    "If statement final check"
                );

                // NOTE: Handle continue block based on whether branches return
                // Use else_returns_all_paths instead of checking the else block's entry terminator
                if then_has_return && else_returns_all_paths && else_block.is_some() {
                    // Both branches return - continue block is truly unreachable
                    // Set current_block to None to prevent ensure_function_termination from adding a return
                    trace!("Both branches return, setting current_block to None (unreachable)");
                    self.current_block = None;
                } else {
                    // At least one branch doesn't return - continue block is reachable
                    // Set current_block to continue block so execution can proceed
                    trace!(continue_block_id = ?continue_block_id, "At least one branch continues");
                    self.current_block = Some(continue_block_id);
                }
            }

            TastStatement::For {
                iterator: _,
                iterator_name,
                iterable,
                body,
                location,
            } => {
                // OPTIMIZATION: Check if iterable is a Range expression
                // If so, generate optimized loop code directly instead of building an array
                if let TastExpressionKind::Range {
                    start,
                    end,
                    step,
                    inclusive,
                } = &iterable.kind
                {
                    trace!("Detected Range expression in For loop, generating optimized code");

                    // Generate optimized range loop directly
                    return self.build_range_for_loop(
                        context,
                        iterator_name,
                        start,
                        end,
                        step.as_deref(),
                        *inclusive,
                        body,
                        location,
                    );
                }

                // Build the iterable expression (e.g., array, range)
                let iterable_value = self.build_expression(context, iterable)?;

                // Create loop blocks
                // NOTE: Use next_block_id counter instead of blocks.len() to prevent
                // block ID collisions when nested control flow creates its own blocks.
                let base_block_id = context.function.next_block_id;
                let header_block_id = BasicBlockId(base_block_id);
                let body_block_id = BasicBlockId(base_block_id + 1);
                let increment_block_id = BasicBlockId(base_block_id + 2);
                let exit_block_id = BasicBlockId(base_block_id + 3);
                // Reserve all 4 block IDs
                context.function.next_block_id = base_block_id + 4;

                // CRITICAL: Pre-insert placeholder blocks to reserve their IDs
                // This prevents nested IF statements from creating blocks with the same IDs
                for (block_id, label) in [
                    (header_block_id, "for_header"),
                    (body_block_id, "for_body"),
                    (increment_block_id, "for_increment"),
                    (exit_block_id, "for_exit"),
                ] {
                    context.function.blocks.insert(
                        block_id,
                        MirBasicBlock {
                            id: block_id,
                            label: Some(label.to_string()),
                            instructions: Vec::new(),
                            terminator: MirTerminator::Unreachable, // Will be replaced
                            predecessors: HashSet::new(),
                            successors: HashSet::new(),
                            location: location.clone(),
                        },
                    );
                }

                // Create iterator index variable (starts at 0)
                let index_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // NOTE: Register index ValueId as a local
                self.register_temp_local(context, index_value_id, MirType::I32, location.clone());

                // Initialize index to 0
                let init_instruction = MirInstruction {
                    dest: Some(index_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::Integer(0)),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, init_instruction);

                // Save the init block ID (current block) for Phi node
                let init_block_id = self.current_block.expect("No current block for loop init");

                // We'll create current_index_value_id here so we can set it in init block
                // (before creating header block)
                let current_index_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    current_index_value_id,
                    MirType::I32,
                    location.clone(),
                );

                // Set current_index to initial value (0) in init block
                let init_current_instruction = MirInstruction {
                    dest: Some(current_index_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Value(index_value_id),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, init_current_instruction);

                // Create iterator value variable for current element
                let iterator_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Add iterator variable to scope (this becomes the loop variable)
                // Use the actual iterator name from the TAST so variable lookups work correctly
                if let Some(current_scope) = context.scope_stack.last_mut() {
                    current_scope.insert(iterator_name.clone(), iterator_value_id);
                }

                // Create local for iterator variable.
                //
                // Derive the MIR type from the iterable's element type. The element is
                // loaded out of the list and bound to this local; downstream codegen
                // (e.g. print, method dispatch) reads the local's type to pick the
                // right runtime path. Hardcoding I32 here breaks list<string> iteration
                // because the string-pointer element ends up routed through the integer
                // print path (RUNTIME_ITERATE_LIST_STRING_CRASH).
                use crate::typechecker::tast::ConcreteType;
                let element_mir_type = match &iterable.expr_type {
                    ConcreteType::Array(elem) => MirType::from_concrete_type(elem),
                    _ => MirType::I32,
                };
                let iterator_local = MirLocal {
                    name: Some(iterator_name.clone()),
                    local_type: element_mir_type.clone(),
                    is_mutable: false,
                    location: location.clone(),
                };
                context
                    .function
                    .locals
                    .insert(iterator_value_id, iterator_local);

                // Jump to header block
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: header_block_id,
                    },
                );

                // Switch to header block (already pre-allocated)
                self.current_block = Some(header_block_id);

                // SSA FIX: Create Phi node to merge index values from different predecessors
                // - From init block: initial 0
                // - From body block: incremented value
                // current_index_value_id was already created and set in init block
                // Create Phi node with init block predecessor
                // Body block predecessor will be added later after we know the incremented ValueId
                let phi_instruction = MirInstruction {
                    dest: Some(current_index_value_id),
                    operation: MirOperation::Phi {
                        incoming: vec![(init_block_id, MirOperand::Value(index_value_id))],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, phi_instruction);

                // Get array length (for bounds checking)
                let length_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // NOTE: Register length ValueId as a local
                self.register_temp_local(context, length_value_id, MirType::I32, location.clone());

                // Load array length directly from memory (length is at offset 0)
                let length_instruction = MirInstruction {
                    dest: Some(length_value_id),
                    operation: MirOperation::Load {
                        source: MirOperand::Value(iterable_value),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, length_instruction);

                // Compare index < length (use the reloaded current_index_value_id)
                let condition_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // NOTE: Register condition ValueId as a local
                self.register_temp_local(
                    context,
                    condition_value_id,
                    MirType::I32, // Boolean represented as I32
                    location.clone(),
                );

                let compare_instruction = MirInstruction {
                    dest: Some(condition_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Lt,
                        left: MirOperand::Value(current_index_value_id),
                        right: MirOperand::Value(length_value_id),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, compare_instruction);

                // Conditional branch: if index < length goto body else goto exit
                self.set_block_terminator(
                    context,
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_value_id),
                        true_block: body_block_id,
                        false_block: exit_block_id,
                    },
                );

                // Switch to body block (already pre-allocated)
                self.current_block = Some(body_block_id);

                // MEMORY MANAGEMENT: Per-iteration mem_scope_push/pop was
                // removed (RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED). The
                // intent was to free transient allocations made inside the
                // body, but any allocation assigned to an OUTER variable
                // (e.g. an accumulator `errs = errs + ...` or a function
                // call result `errs = add_err(errs, ...)`) gets freed at
                // iteration end, leaving the outer variable pointing into
                // memory that the next iteration's allocator overwrites.
                // Two consecutive loops that mutate an outer string then
                // observe the prior iterations' results as garbage, then
                // the second loop's allocations clobber the first loop's
                // entries entirely. The host's per-request scope_pop
                // (COMPILER-NO-FREE-EXPORT-LEAKS-WASM-MEMORY) handles
                // reclamation at request boundaries — that is the correct
                // granularity for an arena-allocator design.

                // NOTE: Get the address of the element, then LOAD the value
                // GetElementPtr returns a pointer, not the value itself!

                // Step 1: Get element pointer.
                // The pointer carries the element MIR type as its inner T — GEP codegen
                // reads it to pick the right per-element stride (4 bytes for i32-class,
                // 8 bytes for f64/i64). Hardcoding Ptr<I32> here made list<number>
                // iteration decode every element after the first as garbage
                // (RUNTIME_ITERATE_LIST_NUMBER_WRONG_LOAD).
                let element_ptr_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    element_ptr_value_id,
                    MirType::Ptr(Box::new(element_mir_type.clone())),
                    location.clone(),
                );

                trace!(current_block = ?self.current_block, "Before GetElementPtr");
                let get_ptr_instruction = MirInstruction {
                    dest: Some(element_ptr_value_id),
                    operation: MirOperation::GetElementPtr {
                        base: MirOperand::Value(iterable_value),
                        indices: vec![MirOperand::Value(current_index_value_id)],
                        is_array: true, // Array iteration - needs header offset
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, get_ptr_instruction);
                trace!(
                    instruction_count = context
                        .function
                        .blocks
                        .get(&body_block_id)
                        .map(|b| b.instructions.len())
                        .unwrap_or(0),
                    "After GetElementPtr"
                );

                // Step 2: Load the actual value from that pointer
                let load_element_instruction = MirInstruction {
                    dest: Some(iterator_value_id),
                    operation: MirOperation::Load {
                        source: MirOperand::Value(element_ptr_value_id),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, load_element_instruction);

                trace!(
                    function_name = %context.function.name,
                    instruction_count = context.function.blocks.get(&body_block_id).map(|b| b.instructions.len()).unwrap_or(0),
                    "Before processing iterate body statements"
                );

                // Process loop body statements
                for stmt in &body.statements {
                    self.build_statement(context, stmt)?;
                }

                trace!(
                    instruction_count = context
                        .function
                        .blocks
                        .get(&body_block_id)
                        .map(|b| b.instructions.len())
                        .unwrap_or(0),
                    "After processing iterate body statements"
                );

                // MEMORY MANAGEMENT: Per-iteration mem_scope_pop removed —
                // see RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED comment at the
                // loop-body entry above. Reclamation is handled by the
                // host's per-request scope, not per-iteration.

                // NOTE: Check if body block already has a terminator
                // (IF statements may have set one). If not, jump to increment block.
                let body_has_terminator = context
                    .function
                    .blocks
                    .get(&body_block_id)
                    .map(|b| !matches!(b.terminator, MirTerminator::Unreachable))
                    .unwrap_or(false);

                if !body_has_terminator {
                    // Body block needs a terminator - jump to increment block
                    self.current_block = Some(body_block_id);
                    self.set_block_terminator(
                        context,
                        MirTerminator::Jump {
                            target: increment_block_id,
                        },
                    );
                } else {
                    // Body block already has terminator (from IF/etc)
                    // Need to redirect it to increment block instead of header
                    // For now, we'll handle this in the increment block by having
                    // continuation blocks jump to increment instead
                    trace!(current_block = ?self.current_block, "Body block already has terminator");

                    // Set current block to wherever we ended up after processing statements
                    // and make it jump to increment block
                    if let Some(_curr_block) = self.current_block {
                        self.set_block_terminator(
                            context,
                            MirTerminator::Jump {
                                target: increment_block_id,
                            },
                        );
                    }
                }

                // Switch to increment block (already pre-allocated)
                self.current_block = Some(increment_block_id);

                // Increment index: index = index + 1
                let incremented_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // NOTE: Register incremented ValueId as a local
                self.register_temp_local(
                    context,
                    incremented_value_id,
                    MirType::I32,
                    location.clone(),
                );

                let increment_instruction = MirInstruction {
                    dest: Some(incremented_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Add,
                        left: MirOperand::Value(current_index_value_id),
                        right: MirOperand::Constant(MirConstant::Integer(1)),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, increment_instruction);

                // SSA FIX: Copy the incremented value to the Phi result local
                // This ensures the Phi node sees the updated value on the next iteration
                let update_phi_instruction = MirInstruction {
                    dest: Some(current_index_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Value(incremented_value_id),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, update_phi_instruction);

                // SSA FIX: Update the Phi node in the header block with the INCREMENT BLOCK's incremented value
                // The Phi node merges:
                // - init_block → index_value_id (0)
                // - increment_block → incremented_value_id (updated counter)
                if let Some(header_block) = context.function.blocks.get_mut(&header_block_id) {
                    if let Some(first_instr) = header_block.instructions.first_mut() {
                        if let MirOperation::Phi { incoming } = &mut first_instr.operation {
                            incoming.push((
                                increment_block_id,
                                MirOperand::Value(incremented_value_id),
                            ));
                        }
                    }
                }

                // Jump back to header from increment block
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: header_block_id,
                    },
                );

                // Switch to exit block (already pre-allocated)
                self.current_block = Some(exit_block_id);
            }

            TastStatement::While {
                condition,
                body,
                location,
            } => {
                // While loop structure:
                // entry block -> header (condition check) -> body -> header
                //                                        -> exit

                // NOTE: Use next_block_id counter instead of blocks.len() to prevent
                // block ID collisions when nested control flow creates its own blocks.
                let base_block_id = context.function.next_block_id;
                let header_block_id = BasicBlockId(base_block_id);
                let body_block_id = BasicBlockId(base_block_id + 1);
                let exit_block_id = BasicBlockId(base_block_id + 2);
                // Reserve all 3 block IDs
                context.function.next_block_id = base_block_id + 3;

                // Pre-insert placeholder blocks to reserve their IDs
                for (block_id, label) in [
                    (header_block_id, "while_header"),
                    (body_block_id, "while_body"),
                    (exit_block_id, "while_exit"),
                ] {
                    context.function.blocks.insert(
                        block_id,
                        MirBasicBlock {
                            id: block_id,
                            label: Some(label.to_string()),
                            instructions: Vec::new(),
                            terminator: MirTerminator::Unreachable, // Will be replaced
                            predecessors: HashSet::new(),
                            successors: HashSet::new(),
                            location: location.clone(),
                        },
                    );
                }

                // Jump from current (entry) block to header
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: header_block_id,
                    },
                );

                // Switch to header block (condition check)
                self.current_block = Some(header_block_id);

                // Build condition expression in header block
                let condition_id = self.build_expression(context, condition)?;

                // Conditional branch: if condition is true goto body, else goto exit
                self.set_block_terminator(
                    context,
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_id),
                        true_block: body_block_id,
                        false_block: exit_block_id,
                    },
                );

                // Switch to body block
                self.current_block = Some(body_block_id);

                // Push loop context for break/continue statements
                context.loop_stack.push(LoopContext {
                    continue_block: header_block_id,
                    break_block: exit_block_id,
                });

                // MEMORY MANAGEMENT: Per-iteration mem_scope_push/pop was
                // removed (RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED). See the
                // detailed comment in the For-loop body above. The same
                // unsoundness applies to while loops: any body allocation
                // assigned to an outer accumulator gets freed at the end of
                // the iteration. Reclamation is handled by the host's
                // per-request scope, not per-iteration.

                // Process body statements
                for stmt in &body.statements {
                    self.build_statement(context, stmt)?;
                }

                // Pop loop context after processing body
                context.loop_stack.pop();

                // After body, check if we still have a current block (not terminated by return)
                // If so, jump back to header for next iteration
                if let Some(current) = self.current_block {
                    // Check if the current block already has a non-placeholder terminator
                    let has_terminator = context
                        .function
                        .blocks
                        .get(&current)
                        .map(|b| !matches!(b.terminator, MirTerminator::Unreachable))
                        .unwrap_or(false);

                    if !has_terminator {
                        self.set_block_terminator(
                            context,
                            MirTerminator::Jump {
                                target: header_block_id,
                            },
                        );
                    }
                }

                // Switch to exit block for subsequent statements
                self.current_block = Some(exit_block_id);
            }

            TastStatement::LaterAssignment {
                variable,
                symbol_id: _,
                expression,
                location,
            } => {
                // Create a value ID for the result variable.
                let variable_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                let mir_type = self.convert_concrete_type(&expression.expr_type);
                self.register_temp_local(
                    context,
                    variable_value_id,
                    mir_type.clone(),
                    location.clone(),
                );

                // If the expression is a simple function call, emit AsyncAwaitCall.
                // Otherwise fall back to synchronous AsyncAssign.
                if let TastExpressionKind::FunctionCall {
                    function: func_expr,
                    arguments,
                    ..
                } = &expression.kind
                {
                    if let TastExpressionKind::Variable { name: fn_name, .. } = &func_expr.kind {
                        let mut arg_values = Vec::new();
                        for arg in arguments {
                            let arg_id = self.build_expression(context, arg)?;
                            // Auto-serialize class instances to JSON strings.
                            // `later` calls target bridge functions which only accept primitives;
                            // a class argument must become its JSON representation.
                            let final_id = if let ConcreteType::Class {
                                symbol_id: class_sym,
                                ..
                            } = &arg.expr_type
                            {
                                let class_sym = *class_sym;
                                let class_name = context
                                    .all_classes
                                    .iter()
                                    .find(|c| c.symbol_id == class_sym)
                                    .map(|c| c.name.clone());
                                if let Some(ref name) = class_name {
                                    if let Some(&serializer_id) =
                                        self.class_serializer_ids.get(name)
                                    {
                                        let result_id = ValueId(context.function.next_value_id);
                                        context.function.next_value_id += 1;
                                        self.register_temp_local(
                                            context,
                                            result_id,
                                            MirType::Ptr(Box::new(MirType::I8)),
                                            arg.location.clone(),
                                        );
                                        self.add_instruction(
                                            context,
                                            MirInstruction {
                                                dest: Some(result_id),
                                                operation: MirOperation::Call {
                                                    function: MirOperand::Function(serializer_id),
                                                    arguments: vec![MirOperand::Value(arg_id)],
                                                },
                                                location: arg.location.clone(),
                                            },
                                        );
                                        result_id
                                    } else {
                                        arg_id
                                    }
                                } else {
                                    arg_id
                                }
                            } else {
                                arg_id
                            };
                            arg_values.push(MirOperand::Value(final_id));
                        }
                        let await_instruction = MirInstruction {
                            dest: Some(variable_value_id),
                            operation: MirOperation::AsyncAwaitCall {
                                fn_name: fn_name.clone(),
                                arguments: arg_values,
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, await_instruction);
                    } else {
                        // Complex callee — synchronous fallback.
                        tracing::warn!(
                            "later: expression callee is not a simple function name — executing synchronously"
                        );
                        let value_id = self.build_expression(context, expression)?;
                        let async_instruction = MirInstruction {
                            dest: Some(variable_value_id),
                            operation: MirOperation::AsyncAssign {
                                source: MirOperand::Value(value_id),
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, async_instruction);
                    }
                } else {
                    // Not a function call — synchronous fallback.
                    let value_id = self.build_expression(context, expression)?;
                    let async_instruction = MirInstruction {
                        dest: Some(variable_value_id),
                        operation: MirOperation::AsyncAssign {
                            source: MirOperand::Value(value_id),
                        },
                        location: location.clone(),
                    };
                    self.add_instruction(context, async_instruction);
                }

                // Add variable to current scope.
                if let Some(current_scope) = context.scope_stack.last_mut() {
                    current_scope.insert(variable.clone(), variable_value_id);
                }
            }

            TastStatement::Background {
                expression,
                location,
            } => {
                // Attempt to extract a simple function call: background someFunc(a, b)
                // If the expression is a function call with a known name, emit AsyncFireCall.
                // Otherwise, fall back to evaluating the expression synchronously and discarding the result.
                if let TastExpressionKind::FunctionCall {
                    function: func_expr,
                    arguments,
                    ..
                } = &expression.kind
                {
                    if let TastExpressionKind::Variable { name: fn_name, .. } = &func_expr.kind {
                        // Simple named function call — emit _async_fire.
                        let mut arg_values = Vec::new();
                        for arg in arguments {
                            let arg_id = self.build_expression(context, arg)?;
                            arg_values.push(MirOperand::Value(arg_id));
                        }
                        let instruction = MirInstruction {
                            dest: None,
                            operation: MirOperation::AsyncFireCall {
                                fn_name: fn_name.clone(),
                                arguments: arg_values,
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, instruction);
                    } else {
                        // Complex callee — synchronous fallback.
                        tracing::warn!(
                            "background: expression callee is not a simple function name — executing synchronously"
                        );
                        // Evaluate the expression; the MIR value is stored to a local.
                        // Dead-code elimination will remove the unused local if it has no side effects.
                        let _ = self.build_expression(context, expression)?;
                    }
                } else {
                    // Not a function call — evaluate and drop result synchronously.
                    tracing::warn!(
                        "background: expression is not a function call — executing synchronously"
                    );
                    // Evaluate the expression; the MIR value is stored to a local.
                    let _ = self.build_expression(context, expression)?;
                }
            }

            TastStatement::Break { location } => {
                // Break jumps to the exit block of the innermost loop.
                // The matching per-iteration mem_scope_pop was removed when
                // the body-level push/pop was removed
                // (RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED). Calling
                // mem_scope_pop here without a matching push would pop the
                // outer (request-level) scope and corrupt memory.
                let break_block = context.loop_stack.last().map(|ctx| ctx.break_block);

                if let Some(target_block) = break_block {
                    self.set_block_terminator(
                        context,
                        MirTerminator::Jump {
                            target: target_block,
                        },
                    );
                } else {
                    return Err(vec![CompilerError::validation_error(
                        "break statement used outside of a loop",
                        location.clone(),
                    )]);
                }
            }

            TastStatement::Continue { location } => {
                // Continue jumps to the header block of the innermost loop.
                // See Break above for why the prior mem_scope_pop call was
                // removed (RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED).
                let continue_block = context.loop_stack.last().map(|ctx| ctx.continue_block);

                if let Some(target_block) = continue_block {
                    self.set_block_terminator(
                        context,
                        MirTerminator::Jump {
                            target: target_block,
                        },
                    );
                } else {
                    return Err(vec![CompilerError::validation_error(
                        "continue statement used outside of a loop",
                        location.clone(),
                    )]);
                }
            }

            TastStatement::Require {
                condition,
                location,
            } => {
                // Generate code for require precondition
                // require <condition> traps if condition is false

                // Evaluate the condition
                let condition_value = self.build_expression(context, condition)?;

                // Reserve block IDs for trap and continue blocks
                let base_block_id = context.function.next_block_id;
                let trap_block_id = BasicBlockId(base_block_id);
                let continue_block_id = BasicBlockId(base_block_id + 1);
                context.function.next_block_id = base_block_id + 2;

                // Pre-insert blocks to reserve their IDs
                // Trap block: executes unreachable if condition is false
                context.function.blocks.insert(
                    trap_block_id,
                    MirBasicBlock {
                        id: trap_block_id,
                        label: Some("require_trap".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Trap, // WASM unreachable trap for contract violation
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );

                // Continue block: normal execution if condition is true
                context.function.blocks.insert(
                    continue_block_id,
                    MirBasicBlock {
                        id: continue_block_id,
                        label: Some("require_continue".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable, // Will be replaced
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );

                // Set terminator: branch based on condition
                // If condition is true, continue; if false, trap
                self.set_block_terminator(
                    context,
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_value),
                        true_block: continue_block_id, // Condition true -> continue
                        false_block: trap_block_id,    // Condition false -> trap
                    },
                );

                // Switch to continue block for subsequent statements
                self.current_block = Some(continue_block_id);
            }

            _ => {
                // Unsupported statement type - return error with details
                return Err(vec![CompilerError::validation_error(
                    format!("Statement type not yet implemented: {:?}", statement),
                    SourceLocation::default(),
                )]);
            }
        }

        Ok(())
    }
    pub(super) fn build_range_for_loop(
        &mut self,
        context: &mut FunctionBuildContext,
        iterator_name: &str,
        start: &TastExpression,
        end: &TastExpression,
        step: Option<&TastExpression>,
        inclusive: bool,
        body: &TastBlock,
        location: &SourceLocation,
    ) -> Result<(), Vec<CompilerError>> {
        trace!(
            iterator_name = %iterator_name,
            inclusive = inclusive,
            has_step = step.is_some(),
            "Building optimized range for loop"
        );

        // Evaluate start, end, and optionally step expressions
        let start_value_id = self.build_expression(context, start)?;
        let end_value_id = self.build_expression(context, end)?;
        let step_value_id = if let Some(step_expr) = step {
            self.build_expression(context, step_expr)?
        } else {
            // Default step is 1
            let default_step_id = ValueId(context.function.next_value_id);
            context.function.next_value_id += 1;
            self.register_temp_local(context, default_step_id, MirType::I32, location.clone());
            let default_step_instruction = MirInstruction {
                dest: Some(default_step_id),
                operation: MirOperation::Copy {
                    source: MirOperand::Constant(MirConstant::Integer(1)),
                },
                location: location.clone(),
            };
            self.add_instruction(context, default_step_instruction);
            default_step_id
        };

        // Determine if we're counting down (negative step)
        // We'll detect this at runtime by checking if step < 0
        let is_negative_step_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, is_negative_step_id, MirType::I32, location.clone());
        let zero_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, zero_id, MirType::I32, location.clone());
        let zero_instruction = MirInstruction {
            dest: Some(zero_id),
            operation: MirOperation::Copy {
                source: MirOperand::Constant(MirConstant::Integer(0)),
            },
            location: location.clone(),
        };
        self.add_instruction(context, zero_instruction);
        let is_negative_instruction = MirInstruction {
            dest: Some(is_negative_step_id),
            operation: MirOperation::BinaryOp {
                op: MirBinaryOp::Lt, // step < 0
                left: MirOperand::Value(step_value_id),
                right: MirOperand::Value(zero_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, is_negative_instruction);

        // NOTE: Use next_block_id counter instead of blocks.len() to prevent
        // block ID collisions when nested control flow creates its own blocks.
        let base_block_id = context.function.next_block_id;
        let header_block_id = BasicBlockId(base_block_id);
        let body_block_id = BasicBlockId(base_block_id + 1);
        let increment_block_id = BasicBlockId(base_block_id + 2);
        let exit_block_id = BasicBlockId(base_block_id + 3);
        // Reserve all 4 block IDs
        context.function.next_block_id = base_block_id + 4;

        // Pre-insert placeholder blocks to reserve their IDs
        // This prevents nested IF statements from creating blocks with the same IDs
        for (block_id, label) in [
            (header_block_id, "range_for_header"),
            (body_block_id, "range_for_body"),
            (increment_block_id, "range_for_increment"),
            (exit_block_id, "range_for_exit"),
        ] {
            context.function.blocks.insert(
                block_id,
                MirBasicBlock {
                    id: block_id,
                    label: Some(label.to_string()),
                    instructions: Vec::new(),
                    terminator: MirTerminator::Unreachable, // Will be replaced
                    predecessors: HashSet::new(),
                    successors: HashSet::new(),
                    location: location.clone(),
                },
            );
        }

        // Create iterator variable (this is the loop counter, starts at start value)
        let iterator_value_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;

        // Register iterator as local
        self.register_temp_local(context, iterator_value_id, MirType::I32, location.clone());

        // Initialize iterator to start value
        let init_instruction = MirInstruction {
            dest: Some(iterator_value_id),
            operation: MirOperation::Copy {
                source: MirOperand::Value(start_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, init_instruction);

        // Add iterator variable to scope so it's accessible in the loop body
        if let Some(current_scope) = context.scope_stack.last_mut() {
            current_scope.insert(iterator_name.to_string(), iterator_value_id);
        }

        // Create local for iterator variable
        let iterator_local = MirLocal {
            name: Some(iterator_name.to_string()),
            local_type: MirType::I32,
            is_mutable: true, // Mutable because it gets incremented
            location: location.clone(),
        };
        context
            .function
            .locals
            .insert(iterator_value_id, iterator_local);

        // Save init block ID for Phi node
        let init_block_id = self
            .current_block
            .expect("No current block for range loop init");

        // Create current_iterator for SSA Phi node
        let current_iterator_value_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(
            context,
            current_iterator_value_id,
            MirType::I32,
            location.clone(),
        );

        // Set current_iterator to initial value in init block
        let init_current_instruction = MirInstruction {
            dest: Some(current_iterator_value_id),
            operation: MirOperation::Copy {
                source: MirOperand::Value(iterator_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, init_current_instruction);

        // Jump to header block
        self.set_block_terminator(
            context,
            MirTerminator::Jump {
                target: header_block_id,
            },
        );

        // Switch to header block (already pre-allocated)
        self.current_block = Some(header_block_id);

        // SSA Phi node to merge iterator values
        let phi_instruction = MirInstruction {
            dest: Some(current_iterator_value_id),
            operation: MirOperation::Phi {
                incoming: vec![(init_block_id, MirOperand::Value(iterator_value_id))],
            },
            location: location.clone(),
        };
        self.add_instruction(context, phi_instruction);

        // Compare: For direction-aware comparison, we need to handle both positive and negative steps
        // Positive step: iterator <= end (or < end if not inclusive)
        // Negative step: iterator >= end (or > end if not inclusive)
        //
        // Runtime logic: condition = (is_negative_step AND (iter >= end)) OR (!is_negative_step AND (iter <= end))
        // For simplicity, we generate both comparisons and select using the is_negative flag

        // Positive comparison: iter <= end (or < end)
        let pos_cond_value_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, pos_cond_value_id, MirType::I32, location.clone());
        let pos_comparison_op = if inclusive {
            MirBinaryOp::Le
        } else {
            MirBinaryOp::Lt
        };
        let pos_compare_instruction = MirInstruction {
            dest: Some(pos_cond_value_id),
            operation: MirOperation::BinaryOp {
                op: pos_comparison_op,
                left: MirOperand::Value(current_iterator_value_id),
                right: MirOperand::Value(end_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, pos_compare_instruction);

        // Negative comparison: iter >= end (or > end)
        let neg_cond_value_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, neg_cond_value_id, MirType::I32, location.clone());
        let neg_comparison_op = if inclusive {
            MirBinaryOp::Ge
        } else {
            MirBinaryOp::Gt
        };
        let neg_compare_instruction = MirInstruction {
            dest: Some(neg_cond_value_id),
            operation: MirOperation::BinaryOp {
                op: neg_comparison_op,
                left: MirOperand::Value(current_iterator_value_id),
                right: MirOperand::Value(end_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, neg_compare_instruction);

        // Select condition based on step direction
        // condition = is_negative ? neg_cond : pos_cond
        let condition_value_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, condition_value_id, MirType::I32, location.clone());
        let select_instruction = MirInstruction {
            dest: Some(condition_value_id),
            operation: MirOperation::Select {
                condition: MirOperand::Value(is_negative_step_id),
                true_value: MirOperand::Value(neg_cond_value_id),
                false_value: MirOperand::Value(pos_cond_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, select_instruction);

        // NOTE: We'll set the header's terminator later (exit_block_id is now pre-allocated)

        // Switch to body block (already pre-allocated)
        self.current_block = Some(body_block_id);

        // MEMORY MANAGEMENT: Per-iteration mem_scope_push/pop was removed
        // (RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED). See the For-loop body
        // comment in this file for the full explanation. Range-based for
        // loops have the same unsoundness: any body allocation assigned to
        // an outer accumulator gets freed at iteration end.

        // Push a new scope for the loop body
        context.scope_stack.push(HashMap::new());

        // Update the scope's iterator variable to point to current_iterator_value_id
        // so the body uses the Phi-merged value
        if let Some(current_scope) = context.scope_stack.last_mut() {
            current_scope.insert(iterator_name.to_string(), current_iterator_value_id);
        }

        // Build loop body
        trace!(
            statement_count = body.statements.len(),
            current_block = ?self.current_block,
            "Building range for loop body"
        );
        for (idx, stmt) in body.statements.iter().enumerate() {
            trace!(statement_index = idx, current_block = ?self.current_block, "Processing body statement");
            self.build_statement(context, stmt)?;
            trace!(statement_index = idx, current_block = ?self.current_block, "After body statement");
        }
        trace!(current_block = ?self.current_block, "After all range for body statements");

        // Pop the loop body scope
        context.scope_stack.pop();

        // MEMORY MANAGEMENT: Per-iteration mem_scope_pop removed — see
        // the loop-body entry comment above
        // (RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED).

        // NOTE: increment_block_id and exit_block_id were pre-allocated at the start
        // of this function to prevent block ID collisions with nested control flow.
        trace!(
            increment_block_id = ?increment_block_id,
            exit_block_id = ?exit_block_id,
            "Using pre-allocated increment and exit block IDs"
        );

        // Now set the header block's terminator with the correct exit_block_id
        if let Some(header_block) = context.function.blocks.get_mut(&header_block_id) {
            header_block.terminator = MirTerminator::Branch {
                condition: MirOperand::Value(condition_value_id),
                true_block: body_block_id,
                false_block: exit_block_id,
            };
        }

        // Check if current block has a terminator (from return/break/continue)
        // After processing the body statements, current_block might have changed
        // (e.g., to an IF statement's continue block)
        let current_has_terminator = if let Some(current_block_id) = self.current_block {
            if let Some(current_block) = context.function.blocks.get(&current_block_id) {
                !matches!(current_block.terminator, MirTerminator::Unreachable)
            } else {
                false
            }
        } else {
            false
        };

        if !current_has_terminator {
            // Jump to increment block
            self.set_block_terminator(
                context,
                MirTerminator::Jump {
                    target: increment_block_id,
                },
            );
        } else {
            trace!("Current block already has terminator, skipping jump to increment");
        }

        // Switch to increment block (already pre-allocated)
        self.current_block = Some(increment_block_id);

        // Increment: iterator = iterator + step (using the step value from earlier)
        // NOTE: Write increment result to current_iterator_value_id
        // Since Phi nodes are NO-OPs in WASM, we must update the variable that
        // the condition check uses directly. Otherwise the loop counter never changes.
        let inc_instruction = MirInstruction {
            dest: Some(current_iterator_value_id),
            operation: MirOperation::BinaryOp {
                op: MirBinaryOp::Add,
                left: MirOperand::Value(current_iterator_value_id),
                right: MirOperand::Value(step_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, inc_instruction);

        // Update Phi node with increment block predecessor
        // Note: Phi is NO-OP in WASM, but we update it for IR correctness
        if let Some(header_block) = context.function.blocks.get_mut(&header_block_id) {
            if let Some(phi_instr) = header_block.instructions.first_mut() {
                if let MirOperation::Phi { incoming } = &mut phi_instr.operation {
                    incoming.push((
                        increment_block_id,
                        MirOperand::Value(current_iterator_value_id),
                    ));
                }
            }
        }

        // Jump back to header
        self.set_block_terminator(
            context,
            MirTerminator::Jump {
                target: header_block_id,
            },
        );

        // Switch to exit block (already pre-allocated)
        trace!(exit_block_id = ?exit_block_id, "Setting current_block to exit_block");
        self.current_block = Some(exit_block_id);

        trace!(current_block = ?self.current_block, "Range for loop completed");
        Ok(())
    }
}
