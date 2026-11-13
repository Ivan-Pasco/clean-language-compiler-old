//! Helper utilities — instruction emission, block management, field/class helpers.

use super::*;

impl MirBuilder {
    /// Add instruction to current basic block
    pub(super) fn add_instruction(
        &mut self,
        context: &mut FunctionBuildContext,
        instruction: MirInstruction,
    ) {
        // A call to a Clean function (or `error(...)`, a conversion or
        // `.length()` of an `any`, a text conversion, a value narrowed into a
        // sized integer, or
        // `json.textToData`) and an integer division may raise: follow it
        // with the `onError` unwinding check (removed again when the program
        // has no `onError` region).
        let may_raise = match &instruction.operation {
            MirOperation::BinaryOp {
                op: MirBinaryOp::Div | MirBinaryOp::Rem,
                left,
                right,
            } => Self::may_divide_integer_by_zero(context, left, right),
            MirOperation::UnboxAnyToI32 { .. }
            | MirOperation::UnboxAnyToI64 { .. }
            | MirOperation::UnboxAnyToF64 { .. }
            | MirOperation::UnboxAnyToBoolean { .. }
            | MirOperation::OptionalConvert {
                conversion: crate::mir::mir_types::OptionalConversion::FromAny { .. },
                ..
            } => true,
            MirOperation::Call {
                function: MirOperand::Function(_),
                ..
            }
            | MirOperation::CallVirtual { .. } => true,
            MirOperation::Call {
                function: MirOperand::NamedFunction { name, .. },
                ..
            } => {
                matches!(
                    name.as_str(),
                    "__err_raise"
                        | "string.toInteger"
                        | "string.toNumber"
                        | "string.toBoolean"
                        | "__json_decode_v2_or_raise"
                        | crate::stdlib::json_class::JSON_TO_CLN_LIST
                        | crate::stdlib::json_class::ANY_ITERABLE
                        | crate::stdlib::json_class::ANY_LENGTH
                        | crate::stdlib::json_class::ANY_CONVERT
                ) || name.starts_with("__any_to_instance")
                    || name.starts_with(crate::typechecker::tast::INTEGER_FIT_PREFIX)
                    // a rule on a validator without a field
                    || crate::stdlib::validator::RULE_FUNCTIONS.contains(&name.as_str())
            }
            _ => false,
        };
        // Unreachable statements are never lowered (`build_statement_list`),
        // so there is always a block to add to.
        let Some(block) = self
            .current_block
            .and_then(|id| context.function.blocks.get_mut(&id))
        else {
            panic!(
                "internal compiler error: MIR instruction {:?} in function '{}' has no current block",
                instruction.operation, context.function.name
            );
        };
        let location = instruction.location.clone();
        block.instructions.push(instruction);
        if may_raise {
            block.instructions.push(MirInstruction {
                dest: None,
                operation: MirOperation::ErrorCheck {
                    region_exit: context.error_regions.last().map(|r| r.exit_block),
                },
                location,
            });
        }
    }

    /// Whether `left / right` (or `%`) may divide an integer by zero, which
    /// raises "division by zero" (codegen `emit_integer_division`): not for a
    /// number operand (IEEE division) and not for a non-zero constant divisor.
    fn may_divide_integer_by_zero(
        context: &FunctionBuildContext,
        left: &MirOperand,
        right: &MirOperand,
    ) -> bool {
        let is_number = |operand: &MirOperand| match operand {
            MirOperand::Constant(MirConstant::Float(_)) => true,
            MirOperand::Value(id) => context
                .function
                .locals
                .get(id)
                .is_some_and(|local| matches!(local.local_type, MirType::F64 | MirType::F32)),
            _ => false,
        };
        let non_zero_constant = matches!(
            right,
            MirOperand::Constant(MirConstant::Integer(n) | MirConstant::Integer64(n)) if *n != 0
        );
        !non_zero_constant && !is_number(left) && !is_number(right)
    }

    /// Emit a call to an error-handling runtime function
    /// (`codegen_registration.rs`, `register_error_handling_operations`).
    pub(super) fn emit_error_runtime_call(
        &mut self,
        context: &mut FunctionBuildContext,
        name: &str,
        dest: Option<ValueId>,
        location: &SourceLocation,
    ) {
        self.add_instruction(
            context,
            MirInstruction {
                dest,
                operation: MirOperation::Call {
                    function: MirOperand::NamedFunction {
                        name: name.to_string(),
                        symbol_id: SymbolId(0),
                    },
                    arguments: Vec::new(),
                },
                location: location.clone(),
            },
        );
    }

    /// Leave the `onError` regions a `return` (`keep = 0`) or a
    /// `break` / `continue` of the innermost loop (`keep` = that loop's
    /// depth) jumps out of: each one's depth count is released.
    pub(super) fn leave_error_regions(
        &mut self,
        context: &mut FunctionBuildContext,
        keep_loop_depth: Option<usize>,
        location: &SourceLocation,
    ) {
        let leaving = context
            .error_regions
            .iter()
            .filter(|r| keep_loop_depth.is_none_or(|depth| r.loop_depth >= depth))
            .count();
        for _ in 0..leaving {
            self.emit_error_runtime_call(context, "__err_leave", None, location);
        }
    }

    /// Start an `onError` region: the following statements run inside a loop
    /// that executes once, whose exit is where the region ends — normally or
    /// when an error check branches out (`MirOperation::ErrorCheck`).
    ///
    /// ```text
    /// current: __err_enter(); first = 1; jump header
    /// header:  branch first -> pre, exit
    /// pre:     once = first; first = 0; branch once -> body, back
    /// back:    jump header          (never taken; makes `header` a loop)
    /// body:    <region statements>; jump exit
    /// ```
    pub(super) fn begin_error_region(
        &mut self,
        context: &mut FunctionBuildContext,
        location: &SourceLocation,
    ) {
        self.uses_error_regions = true;
        self.emit_error_runtime_call(context, "__err_enter", None, location);

        let first = ValueId(context.function.next_value_id);
        let once = ValueId(context.function.next_value_id + 1);
        context.function.next_value_id += 2;
        self.register_temp_local(context, first, MirType::I32, location.clone());
        self.register_temp_local(context, once, MirType::I32, location.clone());

        let base = context.function.next_block_id;
        context.function.next_block_id += 5;
        let [header, pre, back, body, exit] =
            [0, 1, 2, 3, 4].map(|offset| BasicBlockId(base + offset));
        for (id, label) in [
            (header, "onerror_header"),
            (pre, "onerror_once"),
            (back, "else"),
            (body, "onerror_body"),
            (exit, "onerror_exit"),
        ] {
            context.function.blocks.insert(
                id,
                MirBasicBlock {
                    id,
                    label: Some(label.to_string()),
                    instructions: Vec::new(),
                    terminator: MirTerminator::Unreachable,
                    location: location.clone(),
                },
            );
        }
        let copy = |dest, source| MirInstruction {
            dest: Some(dest),
            operation: MirOperation::Copy { source },
            location: location.clone(),
        };

        self.add_instruction(
            context,
            copy(first, MirOperand::Constant(MirConstant::Integer(1))),
        );
        self.set_block_terminator(context, MirTerminator::Jump { target: header });

        self.current_block = Some(header);
        self.set_block_terminator(
            context,
            MirTerminator::Branch {
                condition: MirOperand::Value(first),
                true_block: pre,
                false_block: exit,
            },
        );

        self.current_block = Some(pre);
        self.add_instruction(context, copy(once, MirOperand::Value(first)));
        self.add_instruction(
            context,
            copy(first, MirOperand::Constant(MirConstant::Integer(0))),
        );
        self.set_block_terminator(
            context,
            MirTerminator::Branch {
                condition: MirOperand::Value(once),
                true_block: body,
                false_block: back,
            },
        );

        self.current_block = Some(back);
        self.set_block_terminator(context, MirTerminator::Jump { target: header });

        self.current_block = Some(body);
        context.error_regions.push(ErrorRegion {
            exit_block: exit,
            loop_depth: context.loop_stack.len(),
        });
    }

    /// End the innermost `onError` region: continue at its exit and release
    /// its depth count. The caller then runs the handler when `__err_take()`
    /// reports an error.
    pub(super) fn end_error_region(
        &mut self,
        context: &mut FunctionBuildContext,
        location: &SourceLocation,
    ) -> Result<(), Vec<CompilerError>> {
        let region = context.error_regions.pop().ok_or_else(|| {
            vec![CompilerError::validation_error(
                "onError region ends without a matching start",
                location.clone(),
            )]
        })?;
        let open = self.current_block.is_some_and(|id| {
            context
                .function
                .blocks
                .get(&id)
                .is_some_and(|b| matches!(b.terminator, MirTerminator::Unreachable))
        });
        if open {
            self.set_block_terminator(
                context,
                MirTerminator::Jump {
                    target: region.exit_block,
                },
            );
        }
        self.current_block = Some(region.exit_block);
        self.emit_error_runtime_call(context, "__err_leave", None, location);
        Ok(())
    }

    /// Control does not continue in the current block: it already ends in a
    /// return, break, continue or other jump (its terminator is no longer the
    /// `Unreachable` placeholder), or there is no current block because every
    /// path before it returned.
    pub(super) fn current_block_terminated(&self, context: &FunctionBuildContext) -> bool {
        self.current_block
            .and_then(|id| context.function.blocks.get(&id))
            .is_none_or(|b| !matches!(b.terminator, MirTerminator::Unreachable))
    }

    /// Set terminator for current basic block
    pub(super) fn set_block_terminator(
        &mut self,
        context: &mut FunctionBuildContext,
        terminator: MirTerminator,
    ) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                block.terminator = terminator;
            }
        }
    }

    /// Inject state rules checking at the end of operation boundary functions (start, frame)
    /// Each rule must evaluate to true; if any is false, execution traps
    pub(super) fn inject_rules_checking(
        &mut self,
        context: &mut FunctionBuildContext,
    ) -> Result<(), Vec<CompilerError>> {
        // Clone rules to avoid borrow issues
        let rules = self.state_rules.clone();

        for (rule_idx, rule_expr) in rules.iter().enumerate() {
            trace!(rule_index = rule_idx, "Building state rule check");

            // Build the rule expression - evaluate it to get a boolean
            let condition_value = self.build_expression(context, rule_expr)?;

            // Reserve block IDs for trap and continue blocks
            let base_block_id = context.function.next_block_id;
            let trap_block_id = BasicBlockId(base_block_id);
            let continue_block_id = BasicBlockId(base_block_id + 1);
            context.function.next_block_id = base_block_id + 2;

            // Trap block: executes unreachable if rule is false
            context.function.blocks.insert(
                trap_block_id,
                MirBasicBlock {
                    id: trap_block_id,
                    label: Some(format!("rule_{}_trap", rule_idx)),
                    instructions: Vec::new(),
                    terminator: MirTerminator::Trap, // WASM unreachable trap for contract violation
                    location: rule_expr.location.clone(),
                },
            );

            // Continue block: normal execution if rule is true
            context.function.blocks.insert(
                continue_block_id,
                MirBasicBlock {
                    id: continue_block_id,
                    label: Some(format!("rule_{}_continue", rule_idx)),
                    instructions: Vec::new(),
                    terminator: MirTerminator::Unreachable, // Will be replaced
                    location: rule_expr.location.clone(),
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

            // Switch to continue block for next rule or function termination
            self.current_block = Some(continue_block_id);
        }

        trace!(rule_count = rules.len(), "State rules checking injected");

        Ok(())
    }

    /// Ensure function has proper termination
    pub(super) fn ensure_function_termination(
        &mut self,
        context: &mut FunctionBuildContext,
        return_type: &ConcreteType,
    ) -> Result<(), Vec<CompilerError>> {
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                // Check if block already has a terminator other than Unreachable
                if matches!(block.terminator, MirTerminator::Unreachable) {
                    // If this block is the current_block at function termination time,
                    // it means it's reachable (execution reaches here), so add implicit return.
                    // Add implicit return
                    let return_value = if matches!(return_type, ConcreteType::Undefined) {
                        None
                    } else if context.class_context.is_some()
                        && matches!(return_type, ConcreteType::Class { .. })
                    {
                        // Constructor: return 'this' (first parameter - instance pointer)
                        if let Some(first_param) = context.function.parameters.first() {
                            Some(MirOperand::Value(first_param.value_id))
                        } else {
                            // Fallback to undefined if no parameters (shouldn't happen)
                            Some(MirOperand::Constant(MirConstant::Undefined))
                        }
                    } else {
                        // Return undefined for non-void functions without explicit return
                        Some(MirOperand::Constant(MirConstant::Undefined))
                    };

                    block.terminator = MirTerminator::Return {
                        value: return_value,
                    };
                }
            }
        }

        Ok(())
    }

    /// Resolve pending phi nodes
    pub(super) fn resolve_phi_nodes(
        &mut self,
        _context: &mut FunctionBuildContext,
    ) -> Result<(), Vec<CompilerError>> {
        Ok(())
    }

    /// Count total number of fields in a class including all inherited fields
    ///
    /// This traverses the class hierarchy from the root (most distant ancestor) to the leaf
    /// and sums up all field counts.
    pub(super) fn count_all_fields_in_hierarchy(
        &self,
        context: &FunctionBuildContext,
        class_symbol: SymbolId,
    ) -> usize {
        // Find the starting class
        let mut current_class_opt = context
            .all_classes
            .iter()
            .find(|c| c.symbol_id == class_symbol);

        if current_class_opt.is_none() {
            return 0;
        }

        // Collect all classes in the hierarchy from current to root
        let mut hierarchy = Vec::new();

        while let Some(current_class) = current_class_opt {
            hierarchy.push(current_class.clone());

            // Move to parent
            if let Some(ref parent_symbol) = current_class.parent_class {
                current_class_opt = context
                    .all_classes
                    .iter()
                    .find(|c| c.symbol_id == *parent_symbol);
            } else {
                break;
            }
        }

        // Sum up all field counts from all classes in the hierarchy
        let total_fields: usize = hierarchy.iter().map(|c| c.fields.len()).sum();
        tracing::debug!(
            "count_all_fields_in_hierarchy: class {:?} has {} classes in hierarchy, total {} fields",
            class_symbol,
            hierarchy.len(),
            total_fields
        );
        total_fields
    }

    /// Size in bytes of the per-instance object header prepended to every
    /// class instance. It holds one `i32`: `CLASS_HEADER_MARK | class_id`
    /// (`mir_types::class_header_word`, MEMORY_MODEL.md "Class instance").
    /// The constructor call writes it immediately after allocation. It is
    /// read by dynamic dispatch on capability-typed receivers
    /// (`shape.draw()` where `shape: Drawable`) and by the JSON bridges
    /// (`__json_tree_instance`, `__json_normalize`), which turn an instance
    /// boxed in an `any` into the JSON object of its own class.
    ///
    /// All field offsets and instance sizes account for this header — see
    /// `calculate_field_byte_offset` and `calculate_instance_byte_size`.
    pub(super) const OBJECT_HEADER_SIZE: usize = 4;

    /// Calculate the total byte size of all fields in a class hierarchy,
    /// **including the per-instance object header** (class-id).
    ///
    /// This traverses the class hierarchy and sums up the byte sizes of all fields,
    /// accounting for different type sizes (i32=4, i64/f64=8, etc.), then adds
    /// `OBJECT_HEADER_SIZE` bytes for the header.
    pub(super) fn calculate_instance_byte_size(
        &self,
        context: &FunctionBuildContext,
        class_symbol: SymbolId,
    ) -> usize {
        // Find the starting class
        let mut current_class_opt = context
            .all_classes
            .iter()
            .find(|c| c.symbol_id == class_symbol);

        if current_class_opt.is_none() {
            return 0;
        }

        // Collect all classes in the hierarchy from current to root
        let mut hierarchy = Vec::new();

        while let Some(current_class) = current_class_opt {
            hierarchy.push(current_class.clone());

            // Move to parent
            if let Some(ref parent_symbol) = current_class.parent_class {
                current_class_opt = context
                    .all_classes
                    .iter()
                    .find(|c| c.symbol_id == *parent_symbol);
            } else {
                break;
            }
        }

        // Reverse to get root-to-leaf order
        hierarchy.reverse();

        // Sum up byte sizes for all fields, plus the per-instance object header.
        let mut total_bytes = Self::OBJECT_HEADER_SIZE;
        for class in &hierarchy {
            for field in &class.fields {
                total_bytes += self.get_type_byte_size(&field.field_type);
            }
        }

        tracing::debug!(
            "calculate_instance_byte_size: class {:?} needs {} bytes (incl. {}-byte header)",
            class_symbol,
            total_bytes,
            Self::OBJECT_HEADER_SIZE
        );
        total_bytes
    }

    /// Get the byte size of a ConcreteType
    pub(super) fn get_type_byte_size(&self, concrete_type: &ConcreteType) -> usize {
        match concrete_type {
            ConcreteType::Integer => 4,
            ConcreteType::Number => 8, // f64
            ConcreteType::IntegerSized { bits: 8, .. } => 1,
            ConcreteType::IntegerSized { bits: 16, .. } => 2,
            ConcreteType::IntegerSized { bits: 32, .. } => 4,
            ConcreteType::IntegerSized { bits: 64, .. } => 8,
            ConcreteType::IntegerSized { .. } => 4, // fallback
            ConcreteType::NumberSized { bits: 32 } => 4,
            ConcreteType::NumberSized { bits: 64 } => 8,
            ConcreteType::NumberSized { .. } => 8, // fallback
            ConcreteType::Boolean => 4,            // Stored as i32
            ConcreteType::String => 4,             // Pointer (i32)
            ConcreteType::Null => 4,               // Stored as i32
            ConcreteType::Undefined => 4,          // Stored as i32
            ConcreteType::Array(_) => 4,           // Pointer
            ConcreteType::Matrix(_) => 4,          // Pointer
            ConcreteType::Pairs(_, _) => 4,        // Pointer
            ConcreteType::Function { .. } => 4,    // Pointer
            ConcreteType::Handler => 4,            // Function table index
            ConcreteType::Class { .. } => 4,       // Pointer
            ConcreteType::Interface { .. } => 4,   // Pointer
            ConcreteType::Union(_) => 4,           // Pointer (boxed)
            ConcreteType::Generic { .. } => 4,     // Treat as pointer
            ConcreteType::Unknown => 4,            // Default
            ConcreteType::Namespace => 4,          // Not really used in memory
            ConcreteType::Any => 12,               // Boxed: [tag:i32][value1:i32][value2:i32]
            // An optional primitive is a pointer to its box; every other
            // `T?` has the representation of `T` (none = null pointer).
            ConcreteType::Optional(inner) if inner.is_primitive_value() => 4,
            ConcreteType::Optional(inner) => self.get_type_byte_size(inner),
        }
    }

    /// List methods whose namespace form `list.<method>(items, ...)` takes
    /// the list as its first argument and lowers exactly like the method
    /// call `items.<method>(...)`.
    pub(super) fn is_list_receiver_method(method: &str) -> bool {
        matches!(
            method,
            "get"
                | "set"
                | "add"
                | "push"
                | "insert"
                | "remove"
                | "pop"
                | "removeLast"
                | "peek"
                | "first"
                | "last"
                | "contains"
                | "indexOf"
                | "lastIndexOf"
                | "sort"
                | "reverse"
                | "slice"
                | "concat"
        )
    }

    /// Name of the element-typed implementation of a list method registered
    /// in `src/stdlib/list_class.rs`, or `None` when the method keeps its
    /// default dispatch (i32 `indexOf`/`contains` natives, behavior-aware
    /// `peek`, and the string `indexOf`/`contains` variants).
    pub(super) fn list_method_variant(method: &str, element: &ConcreteType) -> Option<String> {
        match element {
            ConcreteType::Number => Some(match method {
                "first" | "last" | "peek" | "get" | "remove" | "removeAt" | "removeLast" => {
                    format!("__list_{}_f64", method)
                }
                _ => format!("list.{}.f64", method),
            }),
            // `list<integer:64>`: 8-byte i64 elements.
            ConcreteType::IntegerSized { bits: 64, .. } => Some(match method {
                "first" | "last" | "peek" | "get" | "remove" | "removeAt" | "removeLast" => {
                    format!("__list_{}_i64", method)
                }
                _ => format!("list.{}.i64", method),
            }),
            _ => match method {
                "indexOf" | "contains" | "peek" => None,
                "sort" | "lastIndexOf" if matches!(element, ConcreteType::String) => {
                    Some(format!("list.{}.string", method))
                }
                // Optional primitive elements are boxes, compared by value.
                "sort" | "lastIndexOf" if element.optional_primitive().is_some() => {
                    Some(format!("list.{}.boxed", method))
                }
                _ => Some(format!("list.{}", method)),
            },
        }
    }

    /// The `list.add` implementation for a list of `element`: 8-byte
    /// `number` and `integer:64` elements have their own bodies, and strings
    /// compare by bytes on a `.unique` list.
    pub(super) fn list_add_symbol(element: &ConcreteType) -> SymbolId {
        match element {
            ConcreteType::Number => SYM_BUILTIN_LIST_ADD_F64,
            ConcreteType::IntegerSized { bits: 64, .. } => SYM_BUILTIN_LIST_ADD_I64,
            ConcreteType::String => SYM_BUILTIN_LIST_ADD_STRING,
            t if t.optional_primitive().is_some() => SYM_BUILTIN_LIST_ADD_BOXED,
            _ => SYM_BUILTIN_LIST_ADD,
        }
    }

    /// True for the `list.add` symbols of [`Self::list_add_symbol`].
    pub(super) fn is_list_add_symbol(symbol: SymbolId) -> bool {
        [
            SYM_BUILTIN_LIST_ADD,
            SYM_BUILTIN_LIST_ADD_F64,
            SYM_BUILTIN_LIST_ADD_I64,
            SYM_BUILTIN_LIST_ADD_STRING,
            SYM_BUILTIN_LIST_ADD_BOXED,
        ]
        .contains(&symbol)
    }

    /// Position of the element-value argument of a list method, if it has
    /// one (`add(value)`, `set(index, value)`, ...).
    pub(super) fn list_element_arg_index(method: &str) -> Option<usize> {
        match method {
            "add" | "push" | "indexOf" | "lastIndexOf" | "contains" => Some(0),
            "set" | "insert" => Some(1),
            _ => None,
        }
    }

    /// Widen an `integer` value passed as an element of a `list<integer:64>`
    /// or `list<number>` (`items.add(3)`), as an assignment to that type does,
    /// and box a typed value stored into a `list<any>`, so every element of a
    /// `list<any>` is a boxed value.
    pub(super) fn coerce_list_element_arg(
        &mut self,
        context: &mut FunctionBuildContext,
        arg_id: ValueId,
        arg_type: &ConcreteType,
        element: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        if matches!(element, ConcreteType::Any) {
            if matches!(arg_type, ConcreteType::Any) {
                return arg_id;
            }
            return self.emit_box_any(context, arg_id, arg_type, location);
        }
        let target = self.convert_concrete_type(element);
        if !matches!(target, MirType::I64 | MirType::U64 | MirType::F64) {
            return arg_id;
        }
        let source = context
            .function
            .locals
            .get(&arg_id)
            .map(|local| local.local_type.clone());
        if !matches!(
            source,
            Some(MirType::I32 | MirType::I8 | MirType::I16 | MirType::U8 | MirType::U16)
        ) {
            return arg_id;
        }
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, target.clone(), location.clone());
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result_id),
                operation: MirOperation::Cast {
                    value: MirOperand::Value(arg_id),
                    target_type: target,
                },
                location: location.clone(),
            },
        );
        result_id
    }

    /// Store the list pointer returned by `list.add` back into the receiver.
    ///
    /// `list.add` moves a full list to a larger block and returns the new
    /// address. Writing it back keeps the variable or field pointing at the
    /// live list. Supported receivers: local variables, state variables and
    /// fields of a variable (`this.items`, `obj.items`). Other receivers
    /// (e.g. a list returned by a call) are temporaries, so there is nothing
    /// to update.
    pub(super) fn write_back_list_receiver(
        &mut self,
        context: &mut FunctionBuildContext,
        receiver: &TastExpression,
        new_list_id: ValueId,
        location: &SourceLocation,
    ) -> Result<(), Vec<CompilerError>> {
        match &receiver.kind {
            TastExpressionKind::Variable { name, .. } => {
                if let Some((symbol_id, mir_type)) = self.state_variables.get(name).cloned() {
                    self.add_instruction(
                        context,
                        MirInstruction {
                            dest: None,
                            operation: MirOperation::GlobalStore {
                                global_id: symbol_id,
                                value: MirOperand::Value(new_list_id),
                                global_type: mir_type,
                            },
                            location: location.clone(),
                        },
                    );
                } else if let Some(orig_id) = context
                    .scope_stack
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(name).copied())
                {
                    if let Some(binding) = Self::later_binding(context, orig_id) {
                        self.emit_later_write(context, orig_id, &binding, new_list_id, location);
                    } else if orig_id != new_list_id {
                        self.add_instruction(
                            context,
                            MirInstruction {
                                dest: Some(orig_id),
                                operation: MirOperation::Copy {
                                    source: MirOperand::Value(new_list_id),
                                },
                                location: location.clone(),
                            },
                        );
                    }
                }
                Ok(())
            }
            TastExpressionKind::PropertyAccess {
                object,
                property_symbol,
                ..
            } if matches!(object.kind, TastExpressionKind::Variable { .. }) => {
                let class_symbol = match &object.expr_type {
                    ConcreteType::Class { symbol_id, .. } => *symbol_id,
                    _ => return Ok(()),
                };
                let Some(field_byte_offset) =
                    self.calculate_field_byte_offset(context, class_symbol, property_symbol)
                else {
                    return Ok(());
                };
                let object_id = self.build_expression(context, object)?;

                let field_ptr_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                context.function.locals.insert(
                    field_ptr_id,
                    MirLocal {
                        name: None,
                        local_type: MirType::I32,
                        is_mutable: false,
                        location: location.clone(),
                    },
                );
                self.add_instruction(
                    context,
                    MirInstruction {
                        dest: Some(field_ptr_id),
                        operation: MirOperation::GetElementPtr {
                            base: MirOperand::Value(object_id),
                            indices: vec![MirOperand::Constant(MirConstant::Integer(
                                field_byte_offset as i64,
                            ))],
                            is_array: false,
                        },
                        location: location.clone(),
                    },
                );
                self.add_instruction(
                    context,
                    MirInstruction {
                        dest: None,
                        operation: MirOperation::Store {
                            destination: MirOperand::Value(field_ptr_id),
                            value: MirOperand::Value(new_list_id),
                        },
                        location: location.clone(),
                    },
                );
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Emit `pairs.set(map, key, value)` and write the map pointer it returns
    /// back into `receiver`. Like `list.add`, `pairs.set` moves a full map to
    /// a larger block, so the receiver must follow the returned address.
    pub(super) fn emit_pairs_set(
        &mut self,
        context: &mut FunctionBuildContext,
        receiver: &TastExpression,
        map_id: ValueId,
        key_id: ValueId,
        value_id: ValueId,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, MirType::I32, location.clone());
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result_id),
                operation: MirOperation::Call {
                    function: MirOperand::NamedFunction {
                        name: "pairs.set".to_string(),
                        symbol_id: SymbolId(0),
                    },
                    arguments: vec![
                        MirOperand::Value(map_id),
                        MirOperand::Value(key_id),
                        MirOperand::Value(value_id),
                    ],
                },
                location: location.clone(),
            },
        );
        self.write_back_list_receiver(context, receiver, result_id, location)?;
        Ok(result_id)
    }

    /// The declared type of a field, looked up through the class hierarchy.
    ///
    /// The field's declaration decides its size in the instance (see
    /// `calculate_field_byte_offset`), so loads and stores must use it: the
    /// type of an `obj.field` expression can be the unsized `integer` for an
    /// `integer:8` or `integer:64` field.
    pub(super) fn field_declared_type(
        &self,
        context: &FunctionBuildContext,
        class_symbol: SymbolId,
        property_symbol: &SymbolId,
    ) -> Option<ConcreteType> {
        let mut class = context
            .all_classes
            .iter()
            .find(|c| c.symbol_id == class_symbol);
        while let Some(current) = class {
            if let Some(field) = current
                .fields
                .iter()
                .find(|f| f.symbol_id == *property_symbol)
            {
                return Some(field.field_type.clone());
            }
            class = current
                .parent_class
                .and_then(|parent| context.all_classes.iter().find(|c| c.symbol_id == parent));
        }
        None
    }

    /// Calculate the byte offset of a field within a class hierarchy
    ///
    /// This returns the byte offset from the start of the object to the field,
    /// accounting for different field sizes.
    pub(super) fn calculate_field_byte_offset(
        &self,
        context: &FunctionBuildContext,
        class_symbol: SymbolId,
        property_symbol: &SymbolId,
    ) -> Option<usize> {
        // Find the starting class
        let mut current_class_opt = context
            .all_classes
            .iter()
            .find(|c| c.symbol_id == class_symbol);

        current_class_opt?;

        // Collect all classes in the hierarchy from current to root
        let mut hierarchy = Vec::new();

        while let Some(current_class) = current_class_opt {
            hierarchy.push(current_class.clone());

            // Move to parent
            if let Some(ref parent_symbol) = current_class.parent_class {
                current_class_opt = context
                    .all_classes
                    .iter()
                    .find(|c| c.symbol_id == *parent_symbol);
            } else {
                break;
            }
        }

        // Reverse to get root-to-leaf order
        hierarchy.reverse();

        // Calculate byte offset — every field is shifted by OBJECT_HEADER_SIZE
        // to leave room for the class-id at the start of the instance.
        let mut byte_offset = Self::OBJECT_HEADER_SIZE;

        for class in &hierarchy {
            for field in &class.fields {
                if field.symbol_id == *property_symbol {
                    tracing::debug!(
                        "calculate_field_byte_offset: field {:?} at byte offset {} (past {}-byte header)",
                        property_symbol,
                        byte_offset,
                        Self::OBJECT_HEADER_SIZE
                    );
                    return Some(byte_offset);
                }
                byte_offset += self.get_type_byte_size(&field.field_type);
            }
        }

        tracing::debug!(
            "calculate_field_byte_offset: field {:?} NOT FOUND",
            property_symbol
        );
        None
    }

    /// Decide whether wrapping the loop body in a per-iteration
    /// `mem_scope_push` / `mem_scope_pop` pair is sound.
    ///
    /// Returns `true` when the body cannot leak a pointer into outer scope
    /// across a `mem_scope_pop`. That requires both:
    ///
    /// 1. No assignment whose target lives outside the body and whose value
    ///    type allocates on the bump arena (string, list, matrix, pairs,
    ///    class, interface, tuple, any, or an optional of any of those).
    ///    Such an assignment is the exact escape that
    ///    RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED tripped on — the outer
    ///    accumulator would point into the per-iter region after `pop`,
    ///    and the next iteration's allocator would clobber it.
    ///
    /// 2. No `return`, `throw`, or `background` inside the body. `return`
    ///    and `throw` jump straight past the paired `mem_scope_pop`, and
    ///    `background` fires off a closure that retains body-local
    ///    pointers beyond the iteration. `break` and `continue` are fine
    ///    — they go through the dedicated handlers, which emit the
    ///    matching `mem_scope_pop` when the enclosing
    ///    `LoopContext::has_iter_scope` flag is set.
    ///
    /// 3. No call that can store a pointer into outer state, in any
    ///    expression of the body (declaration initializers, assigned
    ///    values, `print`, conditions, `require`, iterables). The
    ///    canonical escape is `outer_list.push(inner_string)`; a call to a
    ///    Clean function can do the same with its arguments
    ///    (`integer k = store(outer, inner)`). Only calls that cannot store
    ///    are allowed (`call_may_store`). A call statement (a
    ///    `TastStatement::Expression` whose expression contains a call) is
    ///    always flagged.
    ///
    /// Property/array-index targets and `LaterAssignment` are treated as
    /// escapes too (the receiver is by construction declared outside the
    /// body).
    ///
    /// Called once per loop builder; result drives whether the loop emits
    /// the push/pop pair and what flag it pushes onto `loop_stack`.
    ///
    /// Restores the per-iter scope hygiene needed for
    /// FRAME-UI-ASSEMBLE-PAGE-COMPANION-NO-ROUTES-MOUNTED (frame.ui's
    /// `find_unescaped_quote` allocates a fresh `substring` every inner
    /// while iteration and never escapes it) without re-introducing the
    /// regression `8c25d971` was guarding against.
    pub(super) fn body_is_iter_scope_safe(&self, body: &TastBlock) -> bool {
        let mut inner = HashSet::new();
        Self::collect_inner_decls(&body.statements, &mut inner);
        !Self::body_escapes_or_returns(&body.statements, &inner)
    }

    fn collect_inner_decls(stmts: &[TastStatement], out: &mut HashSet<SymbolId>) {
        for stmt in stmts {
            match stmt {
                TastStatement::VariableDeclaration { symbol_id, .. } => {
                    out.insert(*symbol_id);
                }
                TastStatement::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    Self::collect_inner_decls(&then_block.statements, out);
                    if let Some(eb) = else_block {
                        Self::collect_inner_decls(&eb.statements, out);
                    }
                }
                TastStatement::While { body, .. } => {
                    Self::collect_inner_decls(&body.statements, out);
                }
                TastStatement::For { iterator, body, .. } => {
                    out.insert(*iterator);
                    Self::collect_inner_decls(&body.statements, out);
                }
                _ => {}
            }
        }
    }

    fn body_escapes_or_returns(stmts: &[TastStatement], inner: &HashSet<SymbolId>) -> bool {
        let may_store = |e: &TastExpression| Self::expr_any(e, &Self::call_may_store);
        for stmt in stmts {
            let escapes = match stmt {
                TastStatement::Return { .. } => true,
                TastStatement::Assignment { target, value, .. } => {
                    let target_escapes = match &target.kind {
                        TastExpressionKind::Variable { symbol_id, .. } => {
                            !inner.contains(symbol_id)
                                && Self::is_arena_alloc_type(&target.expr_type)
                        }
                        TastExpressionKind::PropertyAccess { .. }
                        | TastExpressionKind::ArrayAccess { .. } => {
                            Self::is_arena_alloc_type(&target.expr_type) || may_store(target)
                        }
                        _ => false,
                    };
                    target_escapes || may_store(value)
                }
                TastStatement::VariableDeclaration { initializer, .. } => {
                    initializer.as_ref().is_some_and(may_store)
                }
                // A `later` task can run after the iteration, so the values
                // it captured must outlive the per-iteration pop.
                TastStatement::LaterAssignment { .. } => true,
                // A call statement (`outer_list.push(inner)`, `Foo.bar(x)`, etc.)
                // can mutate any receiver or pass arguments to outer-scope
                // state without going through a TastStatement::Assignment.
                // The canonical shape from frame.ui's `read_block_body` —
                // `block_lines.push(line)` inside a while body, where
                // `block_lines` is declared outside the loop — stores the
                // iter-allocated `line` pointer into outer state. The
                // matching `mem_scope_pop` then frees the pointer's target,
                // and the next iteration's allocator overlaps it. The
                // resulting garbage `block_lines` entries surface as
                // `wasm unreachable` traps once the corrupted list is
                // walked by `string.split` / `iterate` / `substring`
                // (COMPILER-0-30-342-CLASS-METHOD-STRING-OPS-WASM-UNREACHABLE).
                TastStatement::Expression { expression, .. } => {
                    Self::expr_any(expression, &Self::is_call)
                }
                TastStatement::Print { expression, .. } => may_store(expression),
                TastStatement::Require { condition, .. }
                | TastStatement::Ensure { condition, .. } => may_store(condition),
                // `background` hands the host a task that keeps body-local
                // pointers past the pop.
                TastStatement::Background { .. } => true,
                TastStatement::If {
                    condition,
                    then_block,
                    else_block,
                    ..
                } => {
                    may_store(condition)
                        || Self::body_escapes_or_returns(&then_block.statements, inner)
                        || else_block
                            .as_ref()
                            .is_some_and(|eb| Self::body_escapes_or_returns(&eb.statements, inner))
                }
                TastStatement::While {
                    condition, body, ..
                } => may_store(condition) || Self::body_escapes_or_returns(&body.statements, inner),
                TastStatement::For { iterable, body, .. } => {
                    may_store(iterable) || Self::body_escapes_or_returns(&body.statements, inner)
                }
                TastStatement::Break { .. } | TastStatement::Continue { .. } => false,
            };
            if escapes {
                return true;
            }
        }
        false
    }

    fn is_call(e: &TastExpression) -> bool {
        matches!(
            e.kind,
            TastExpressionKind::FunctionCall { .. }
                | TastExpressionKind::MethodCall { .. }
                | TastExpressionKind::StaticMethodCall { .. }
                | TastExpressionKind::BaseCall { .. }
        )
    }

    /// A call that can store one of its arguments (or a value computed this
    /// iteration) somewhere that outlives the iteration. A Clean function or
    /// class method can store anywhere. Methods of an immutable value (a
    /// string, a number, a boolean) and the reading methods of a collection
    /// only return values, which the declaration and assignment rules check.
    fn call_may_store(e: &TastExpression) -> bool {
        match &e.kind {
            TastExpressionKind::FunctionCall { function, .. } => !matches!(
                &function.kind,
                TastExpressionKind::Variable { name, .. }
                    if name == crate::typechecker::tast::OPTIONAL_BOX
                        || name.starts_with(crate::typechecker::tast::INTEGER_FIT_PREFIX)
            ),
            TastExpressionKind::MethodCall {
                receiver,
                method_name,
                ..
            } => match &receiver.expr_type {
                ConcreteType::String => false,
                t if t.is_primitive_value() => false,
                ConcreteType::Array(_) | ConcreteType::Pairs(_, _) | ConcreteType::Any => {
                    !matches!(
                        method_name.as_str(),
                        "length"
                            | "size"
                            | "get"
                            | "contains"
                            | "has"
                            | "indexOf"
                            | "lastIndexOf"
                            | "isEmpty"
                            | "isNotEmpty"
                            | "first"
                            | "last"
                            | "join"
                            | "toString"
                    )
                }
                _ => true,
            },
            TastExpressionKind::StaticMethodCall { .. } | TastExpressionKind::BaseCall { .. } => {
                true
            }
            _ => false,
        }
    }

    /// `pred` holds for `e` or one of its sub-expressions.
    fn expr_any(e: &TastExpression, pred: &dyn Fn(&TastExpression) -> bool) -> bool {
        if pred(e) {
            return true;
        }
        let any = |es: &[TastExpression]| es.iter().any(|x| Self::expr_any(x, pred));
        match &e.kind {
            TastExpressionKind::Literal { .. } | TastExpressionKind::Variable { .. } => false,
            TastExpressionKind::BinaryOperation { left, right, .. } => {
                Self::expr_any(left, pred) || Self::expr_any(right, pred)
            }
            TastExpressionKind::UnaryOperation { operand, .. } => Self::expr_any(operand, pred),
            TastExpressionKind::FunctionCall {
                function,
                arguments,
                ..
            } => Self::expr_any(function, pred) || any(arguments),
            TastExpressionKind::MethodCall {
                receiver,
                arguments,
                ..
            } => Self::expr_any(receiver, pred) || any(arguments),
            TastExpressionKind::StaticMethodCall { arguments, .. }
            | TastExpressionKind::BaseCall { arguments, .. } => any(arguments),
            TastExpressionKind::PropertyAccess { object, .. } => Self::expr_any(object, pred),
            TastExpressionKind::ArrayLiteral { elements, .. } => any(elements),
            TastExpressionKind::ArrayAccess { array, index } => {
                Self::expr_any(array, pred) || Self::expr_any(index, pred)
            }
            TastExpressionKind::ObjectLiteral { fields } => {
                fields.iter().any(|f| Self::expr_any(&f.value, pred))
            }
            TastExpressionKind::OnError {
                expression,
                fallback,
            } => Self::expr_any(expression, pred) || Self::expr_any(fallback, pred),
            TastExpressionKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                Self::expr_any(condition, pred)
                    || Self::expr_any(then_expr, pred)
                    || Self::expr_any(else_expr, pred)
            }
            TastExpressionKind::Range {
                start, end, step, ..
            } => {
                Self::expr_any(start, pred)
                    || Self::expr_any(end, pred)
                    || step.as_ref().is_some_and(|s| Self::expr_any(s, pred))
            }
        }
    }

    fn is_arena_alloc_type(t: &ConcreteType) -> bool {
        match t {
            ConcreteType::String
            | ConcreteType::Array(_)
            | ConcreteType::Matrix(_)
            | ConcreteType::Pairs(_, _)
            | ConcreteType::Class { .. }
            | ConcreteType::Interface { .. }
            | ConcreteType::Any => true,
            // An optional primitive points to a heap box.
            ConcreteType::Optional(inner) if inner.is_primitive_value() => true,
            ConcreteType::Optional(inner) => Self::is_arena_alloc_type(inner),
            ConcreteType::Union(parts) => parts.iter().any(Self::is_arena_alloc_type),
            _ => false,
        }
    }

    /// Emit a `mem_scope_push` call on the current block. No return value.
    /// Pair with `emit_mem_scope_pop` at every exit from the iteration.
    pub(super) fn emit_mem_scope_push(
        &mut self,
        context: &mut FunctionBuildContext,
        location: SourceLocation,
    ) {
        let instr = MirInstruction {
            dest: None,
            operation: MirOperation::Call {
                function: MirOperand::NamedFunction {
                    name: "mem_scope_push".to_string(),
                    symbol_id: SymbolId(0),
                },
                arguments: vec![],
            },
            location,
        };
        self.add_instruction(context, instr);
    }

    /// Emit a `mem_scope_pop` call on the current block. No return value.
    pub(super) fn emit_mem_scope_pop(
        &mut self,
        context: &mut FunctionBuildContext,
        location: SourceLocation,
    ) {
        let instr = MirInstruction {
            dest: None,
            operation: MirOperation::Call {
                function: MirOperand::NamedFunction {
                    name: "mem_scope_pop".to_string(),
                    symbol_id: SymbolId(0),
                },
                arguments: vec![],
            },
            location,
        };
        self.add_instruction(context, instr);
    }

    /// Lower a value chosen by `condition`: the `then` arm when it is
    /// non-zero, the `else` arm otherwise. Only the chosen arm runs: each arm
    /// gets its own block (labelled like an `if`/`else` so the structured
    /// codegen emits an `if ... else ... end`), and both store into one
    /// result local of `result_type` before joining at the merge block.
    pub(super) fn build_two_way_value(
        &mut self,
        context: &mut FunctionBuildContext,
        condition: ValueId,
        then_arm: BranchArm<'_>,
        else_arm: BranchArm<'_>,
        result_type: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        let result_mir_type = self.convert_concrete_type(result_type);
        self.register_temp_local(context, result_id, result_mir_type, location.clone());

        let base = context.function.next_block_id;
        context.function.next_block_id += 3;
        let then_block = BasicBlockId(base);
        let else_block = BasicBlockId(base + 1);
        let merge_block = BasicBlockId(base + 2);
        for (id, label) in [
            (then_block, "then"),
            (else_block, "else"),
            (merge_block, "continue"),
        ] {
            context.function.blocks.insert(
                id,
                MirBasicBlock {
                    id,
                    label: Some(label.to_string()),
                    instructions: Vec::new(),
                    terminator: MirTerminator::Unreachable,
                    location: location.clone(),
                },
            );
        }
        if let Some(branch_block) = self.current_block {
            context.function.if_merges.insert(branch_block, merge_block);
        }
        self.set_block_terminator(
            context,
            MirTerminator::Branch {
                condition: MirOperand::Value(condition),
                true_block: then_block,
                false_block: else_block,
            },
        );

        for (block, arm) in [(then_block, then_arm), (else_block, else_arm)] {
            self.current_block = Some(block);
            let (value_id, expression) = match arm {
                BranchArm::Built(value_id, expression) => (value_id, expression),
                BranchArm::Lazy(expression) => {
                    (self.build_expression(context, expression)?, expression)
                }
            };
            let value_id = self.coerce_on_error_branch(context, value_id, expression, result_type);
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
            self.set_block_terminator(
                context,
                MirTerminator::Jump {
                    target: merge_block,
                },
            );
        }

        self.current_block = Some(merge_block);
        Ok(result_id)
    }
}

impl MirBuilder {
    /// A plain `integer`, `number` or `boolean` (of any width): only the
    /// nullable form `T?` (`ConcreteType::Optional`) can hold null.
    pub(super) fn is_non_nullable_value_type(t: &ConcreteType) -> bool {
        matches!(
            t,
            ConcreteType::Integer
                | ConcreteType::Number
                | ConcreteType::Boolean
                | ConcreteType::IntegerSized { .. }
                | ConcreteType::NumberSized { .. }
        )
    }

    /// Emit an i32 that is non-zero when `value_id` (of type `value_type`)
    /// is not null. Null is the zero value of the representation (a null
    /// pointer, or `none` of a `T?`); an `any` is also null when it boxes
    /// the null tag (what a JSON `null` decodes to).
    pub(super) fn emit_non_null_test(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        value_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        let value_mir_type = context
            .function
            .locals
            .get(&value_id)
            .map(|l| l.local_type.clone())
            .unwrap_or_else(|| self.convert_concrete_type(value_type));
        let new_i32 = |builder: &mut Self, context: &mut FunctionBuildContext| {
            let id = ValueId(context.function.next_value_id);
            context.function.next_value_id += 1;
            builder.register_temp_local(context, id, MirType::I32, location.clone());
            id
        };
        if matches!(value_mir_type, MirType::Any) {
            // tag = ptr != 0 ? load(ptr) : 0 — reading the tag word of a
            // null pointer reads address 0, which is never trapped; the
            // select discards it.
            let tag = new_i32(self, context);
            self.add_instruction(
                context,
                MirInstruction {
                    dest: Some(tag),
                    operation: MirOperation::Load {
                        source: MirOperand::Value(value_id),
                    },
                    location: location.clone(),
                },
            );
            let result = new_i32(self, context);
            self.add_instruction(
                context,
                MirInstruction {
                    dest: Some(result),
                    operation: MirOperation::Select {
                        condition: MirOperand::Value(value_id),
                        true_value: MirOperand::Value(tag),
                        false_value: MirOperand::Constant(MirConstant::Integer(
                            AnyTypeTag::Null.as_i32() as i64,
                        )),
                    },
                    location: location.clone(),
                },
            );
            return result;
        }
        let zero = match value_mir_type {
            MirType::F32 | MirType::F64 => MirConstant::Float(0.0),
            MirType::I64 | MirType::U64 => MirConstant::Integer64(0),
            // An i32 value (a pointer, or a `T?` of an i32 type) is its own
            // test: a branch on it is taken when it is non-zero. Comparing
            // a string pointer with 0 would compare string contents.
            _ => return value_id,
        };
        let result = new_i32(self, context);
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result),
                operation: MirOperation::BinaryOp {
                    op: MirBinaryOp::Ne,
                    left: MirOperand::Value(value_id),
                    right: MirOperand::Constant(zero),
                },
                location: location.clone(),
            },
        );
        result
    }
}

impl MirBuilder {
    /// Emit an i32 boolean: whether two `any` values are equal. They are
    /// equal when they box the same kind of value (same type tag; a null
    /// pointer counts as the null tag) with the same text — numbers,
    /// booleans and strings by value, lists and objects by their JSON text.
    pub(super) fn emit_any_values_equal(
        &mut self,
        context: &mut FunctionBuildContext,
        left: ValueId,
        right: ValueId,
        location: &SourceLocation,
    ) -> ValueId {
        let left_tag = self.emit_non_null_test(context, left, &ConcreteType::Any, location);
        let right_tag = self.emit_non_null_test(context, right, &ConcreteType::Any, location);
        let left_text = self.emit_any_to_string(context, left, location);
        let right_text = self.emit_any_to_string(context, right, location);
        let mut binary = |builder: &mut Self, op: MirBinaryOp, a: ValueId, b: ValueId| {
            let id = ValueId(context.function.next_value_id);
            context.function.next_value_id += 1;
            builder.register_temp_local(context, id, MirType::I32, location.clone());
            builder.add_instruction(
                context,
                MirInstruction {
                    dest: Some(id),
                    operation: MirOperation::BinaryOp {
                        op,
                        left: MirOperand::Value(a),
                        right: MirOperand::Value(b),
                    },
                    location: location.clone(),
                },
            );
            id
        };
        let same_tag = binary(self, MirBinaryOp::Eq, left_tag, right_tag);
        let same_text = binary(self, MirBinaryOp::Eq, left_text, right_text);
        binary(self, MirBinaryOp::And, same_tag, same_text)
    }
}

impl MirBuilder {
    /// SymbolId of a compiler built-in (`int_to_string`, `string.length`, ...)
    /// as registered in the symbol table.
    pub(super) fn builtin_symbol(
        &self,
        name: &str,
        location: &SourceLocation,
    ) -> Result<SymbolId, Vec<CompilerError>> {
        self.symbol_table.lookup_symbol(name).ok_or_else(|| {
            vec![CompilerError::validation_error(
                format!("Built-in function '{}' is not registered", name),
                location.clone(),
            )]
        })
    }

    /// Emit `result = name(arguments)` for the built-in `name`, whose result
    /// has type `result_type`.
    pub(super) fn emit_builtin_call(
        &mut self,
        context: &mut FunctionBuildContext,
        name: &str,
        arguments: Vec<MirOperand>,
        result_type: MirType,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        let function = MirOperand::Function(self.builtin_symbol(name, location)?);
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, result_type, location.clone());
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result_id),
                operation: MirOperation::Call {
                    function,
                    arguments,
                },
                location: location.clone(),
            },
        );
        Ok(result_id)
    }
}

impl MirBuilder {
    /// The 8-byte cell functions of a pairs value type that does not fit
    /// the entry's 4 bytes: (store, load, value type).
    fn pairs_value_cell(
        &self,
        value_type: &ConcreteType,
    ) -> Option<(&'static str, &'static str, MirType)> {
        match self.convert_concrete_type(value_type) {
            MirType::F64 => Some(("__pairs_cell_f64", "__pairs_cell_load_f64", MirType::F64)),
            MirType::I64 | MirType::U64 => {
                Some(("__pairs_cell_i64", "__pairs_cell_load_i64", MirType::I64))
            }
            _ => None,
        }
    }

    /// Whether a value stored into a `pairs<K, V>` entry is converted
    /// (`pairs_store_value`): `V` is `any`, `number` or `integer:64`.
    pub(super) fn pairs_value_converts(&self, value_type: &ConcreteType) -> bool {
        matches!(value_type, ConcreteType::Any) || self.pairs_value_cell(value_type).is_some()
    }

    /// The 4-byte entry value `pairs.set` stores for `value_id` (of type
    /// `source_type`) in a pairs map of `value_type` values: an `any` value
    /// is boxed, a `number` or `integer:64` value is stored in an 8-byte
    /// cell the entry points to, and any other value is stored as is.
    pub(super) fn pairs_store_value(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        source_type: &ConcreteType,
        value_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        if matches!(value_type, ConcreteType::Any) {
            if matches!(source_type, ConcreteType::Any | ConcreteType::Null) {
                return value_id;
            }
            return self.emit_box_any(context, value_id, source_type, location);
        }
        let Some((store, _, cell_type)) = self.pairs_value_cell(value_type) else {
            return value_id;
        };
        let value_id = self.coerce_to_slot_type(context, value_id, &cell_type, location);
        self.emit_named_call(
            context,
            store,
            vec![MirOperand::Value(value_id)],
            MirType::I32,
            location,
        )
    }

    /// The value of a pairs entry `pairs.get` returned (`raw_id`): the
    /// value read from its cell for a `number` or `integer:64` map, the
    /// entry itself otherwise.
    pub(super) fn pairs_load_value(
        &mut self,
        context: &mut FunctionBuildContext,
        raw_id: ValueId,
        value_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        match self.pairs_value_cell(value_type) {
            Some((_, load, cell_type)) => self.emit_named_call(
                context,
                load,
                vec![MirOperand::Value(raw_id)],
                cell_type,
                location,
            ),
            None => raw_id,
        }
    }

    /// The key a pairs lookup or store passes to `pairs.get` / `pairs.set`.
    /// Pairs keys are stored as text — a literal `{1: "one"}` stores the
    /// key "1" — so a key of another type is converted to its text.
    pub(super) fn pairs_key(
        &mut self,
        context: &mut FunctionBuildContext,
        key_id: ValueId,
        key_type: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        match key_type {
            ConcreteType::String => Ok(key_id),
            ConcreteType::Any => Ok(self.emit_any_to_string(context, key_id, location)),
            _ => self.convert_value_to_string(context, key_id, key_type, location),
        }
    }

    /// The methods that override `method` for an instance of a subclass of
    /// `static_class`, each with the class-ids of the classes whose
    /// instances run it (15-classes-and-objects.md "Method Overriding").
    /// Empty when the call can stay a direct call: no subclass redefines
    /// the method, or the method is private (a private method is not
    /// inherited, so a subclass method of the same name does not override
    /// it). An override whose parameters or result lower to other WASM
    /// types than `method`'s cannot take the same call and is not
    /// dispatched to.
    pub(super) fn method_overrides(
        &self,
        static_class: SymbolId,
        method: SymbolId,
    ) -> Vec<(SymbolId, Vec<u32>)> {
        let class_by_id = |id: SymbolId| self.all_classes.iter().find(|c| c.symbol_id == id);
        // The nearest declaration of `name` from `class` up its parents.
        let resolve = |class: SymbolId, name: &str| -> Option<&TastFunction> {
            let mut current = class_by_id(class);
            while let Some(c) = current {
                if let Some(m) = c.methods.iter().find(|m| m.name == name) {
                    return Some(m);
                }
                current = c.parent_class.and_then(class_by_id);
            }
            None
        };
        let mut declared = None;
        let mut current = class_by_id(static_class);
        while let Some(c) = current {
            if let Some(m) = c.methods.iter().find(|m| m.symbol_id == method) {
                declared = Some(m);
                break;
            }
            current = c.parent_class.and_then(class_by_id);
        }
        let Some(declared) = declared else {
            return Vec::new();
        };
        if declared.is_static || declared.visibility == Visibility::Private {
            return Vec::new();
        }
        let signature = |m: &TastFunction| {
            (
                m.parameters
                    .iter()
                    .map(|p| self.convert_concrete_type(&p.param_type))
                    .collect::<Vec<_>>(),
                self.convert_concrete_type(&m.return_type),
            )
        };
        let declared_signature = signature(declared);
        let is_subclass = |class: &TastClass| {
            let mut parent = class.parent_class;
            while let Some(p) = parent {
                if p == static_class {
                    return true;
                }
                parent = class_by_id(p).and_then(|c| c.parent_class);
            }
            false
        };
        let mut overrides: Vec<(SymbolId, Vec<u32>)> = Vec::new();
        for class in self.all_classes.iter().filter(|c| is_subclass(c)) {
            let Some(target) = resolve(class.symbol_id, &declared.name) else {
                continue;
            };
            if target.symbol_id == method
                || target.is_static
                || signature(target) != declared_signature
            {
                continue;
            }
            let Some(&class_id) = self.class_ids.get(&class.symbol_id) else {
                continue;
            };
            match overrides.iter_mut().find(|(m, _)| *m == target.symbol_id) {
                Some((_, ids)) => ids.push(class_id),
                None => overrides.push((target.symbol_id, vec![class_id])),
            }
        }
        for (_, ids) in &mut overrides {
            ids.sort_unstable();
        }
        overrides.sort_by_key(|(m, _)| *m);
        overrides
    }
}

/// One arm of [`MirBuilder::build_two_way_value`].
pub(super) enum BranchArm<'e> {
    /// A value computed before the branch, typed as the given expression.
    Built(ValueId, &'e TastExpression),
    /// An expression evaluated only when its arm is taken.
    Lazy(&'e TastExpression),
}
