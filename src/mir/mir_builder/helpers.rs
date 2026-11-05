//! Helper utilities — instruction emission, block management, field/class helpers.

use super::*;

impl MirBuilder {
    /// Add instruction to current basic block
    pub(super) fn add_instruction(
        &mut self,
        context: &mut FunctionBuildContext,
        instruction: MirInstruction,
    ) {
        if matches!(instruction.operation, MirOperation::GetElementPtr { .. }) {
            trace!(current_block = ?self.current_block, "Adding GetElementPtr");
        }
        if matches!(instruction.operation, MirOperation::Load { .. })
            && self.current_block == Some(BasicBlockId(2))
        {
            trace!(current_block = ?self.current_block, "Adding Load to body block");
        }

        // A call to a Clean function (or `error(...)`) may raise: follow it
        // with the `onError` unwinding check (removed again when the program
        // has no `onError` region).
        let may_raise = match &instruction.operation {
            MirOperation::Call {
                function: MirOperand::Function(_),
                ..
            } => true,
            MirOperation::Call {
                function: MirOperand::NamedFunction { name, .. },
                ..
            } => name == "__err_raise",
            _ => false,
        };
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
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
        }
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
                    predecessors: HashSet::new(),
                    successors: HashSet::new(),
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

    /// Set terminator for current basic block
    pub(super) fn set_block_terminator(
        &mut self,
        context: &mut FunctionBuildContext,
        terminator: MirTerminator,
    ) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                if block_id == BasicBlockId(3) && context.function.name == "test" {
                    trace!(
                        old_terminator = ?block.terminator,
                        new_terminator = ?terminator,
                        "Setting terminator for BasicBlockId(3)"
                    );
                }
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
                    predecessors: HashSet::new(),
                    successors: HashSet::new(),
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
                    predecessors: HashSet::new(),
                    successors: HashSet::new(),
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
            ConcreteType::Class { .. } => 4,       // Pointer
            ConcreteType::Interface { .. } => 4,   // Pointer
            ConcreteType::Union(_) => 4,           // Pointer (boxed)
            ConcreteType::Generic { .. } => 4,     // Treat as pointer
            ConcreteType::Unknown => 4,            // Default
            ConcreteType::Never => 0,              // Never returns
            ConcreteType::Namespace => 4,          // Not really used in memory
            ConcreteType::Any => 12,               // Boxed: [tag:i32][value1:i32][value2:i32]
            // Optional<T> has the same runtime representation as T (none = 0/null)
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
                    if orig_id != new_list_id {
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
    /// 3. No bare method-call or function-call statement (a
    ///    `TastStatement::Expression` whose expression is a call). The
    ///    canonical escape is `outer_list.push(inner_string)` — the
    ///    receiver lives outside the body, the argument was allocated
    ///    this iteration, and the call stores the pointer into outer
    ///    state. The analyzer can't trace arbitrary call effects, so any
    ///    call statement is conservatively flagged as an escape. Calls
    ///    that appear as the RHS of a variable declaration or assignment
    ///    are still examined by the target rules in (1).
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
        for stmt in stmts {
            match stmt {
                TastStatement::Return { .. } => return true,
                TastStatement::Assignment { target, .. } => match &target.kind {
                    TastExpressionKind::Variable { symbol_id, .. } => {
                        if !inner.contains(symbol_id)
                            && Self::is_arena_alloc_type(&target.expr_type)
                        {
                            return true;
                        }
                    }
                    TastExpressionKind::PropertyAccess { .. }
                    | TastExpressionKind::ArrayAccess { .. }
                        if Self::is_arena_alloc_type(&target.expr_type) =>
                    {
                        return true;
                    }
                    _ => {}
                },
                TastStatement::LaterAssignment { symbol_id, .. } => {
                    if !inner.contains(symbol_id) {
                        return true;
                    }
                }
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
                    if Self::expr_contains_call(expression) {
                        return true;
                    }
                }
                // `background` fires off a closure that retains body-local
                // pointers across the pop.
                TastStatement::Background { .. } => return true,
                TastStatement::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    if Self::body_escapes_or_returns(&then_block.statements, inner) {
                        return true;
                    }
                    if let Some(eb) = else_block {
                        if Self::body_escapes_or_returns(&eb.statements, inner) {
                            return true;
                        }
                    }
                }
                TastStatement::While { body, .. } => {
                    if Self::body_escapes_or_returns(&body.statements, inner) {
                        return true;
                    }
                }
                TastStatement::For { body, .. } => {
                    if Self::body_escapes_or_returns(&body.statements, inner) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Returns true if the expression (or any sub-expression) is a
    /// method/function/static-method call. Used by
    /// `body_escapes_or_returns` to disqualify loop bodies that contain
    /// a discarded call statement — those can mutate outer state in
    /// ways the analyzer cannot trace.
    ///
    /// Walks through wrapper kinds (BinaryOperation, UnaryOperation,
    /// PropertyAccess, ArrayAccess, ArrayLiteral, Cast, etc.) because a
    /// statement like `outer.list.add(inner)` is parsed as a
    /// PropertyAccess whose inner expression is the MethodCall.
    fn expr_contains_call(e: &TastExpression) -> bool {
        match &e.kind {
            TastExpressionKind::FunctionCall { .. }
            | TastExpressionKind::MethodCall { .. }
            | TastExpressionKind::StaticMethodCall { .. } => true,
            TastExpressionKind::BinaryOperation { left, right, .. } => {
                Self::expr_contains_call(left) || Self::expr_contains_call(right)
            }
            TastExpressionKind::UnaryOperation { operand, .. } => Self::expr_contains_call(operand),
            TastExpressionKind::PropertyAccess { object, .. } => Self::expr_contains_call(object),
            TastExpressionKind::ArrayAccess { array, index, .. } => {
                Self::expr_contains_call(array) || Self::expr_contains_call(index)
            }
            TastExpressionKind::ArrayLiteral { elements, .. } => {
                elements.iter().any(Self::expr_contains_call)
            }
            _ => false,
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
}
