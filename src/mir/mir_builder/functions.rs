//! Function building — lowers TAST functions and classes to MIR functions.

use super::*;

impl MirBuilder {
    /// Build MIR function from TAST function
    pub(super) fn build_function(
        &mut self,
        tast_function: TastFunction,
    ) -> Result<MirFunction, Vec<CompilerError>> {
        self.build_function_with_class_context(tast_function, None)
    }

    /// Build MIR function from TAST function with optional class context for field access
    pub(super) fn build_function_with_class_context(
        &mut self,
        tast_function: TastFunction,
        class_context: Option<&TastClass>,
    ) -> Result<MirFunction, Vec<CompilerError>> {
        debug!(
            function = %tast_function.name,
            param_count = tast_function.parameters.len(),
            "Building MIR for function"
        );
        for (i, param) in tast_function.parameters.iter().enumerate() {
            trace!(
                index = i,
                name = %param.name,
                param_type = ?param.param_type,
                has_default = param.default_value.is_some(),
                "TAST parameter"
            );
        }

        // Create function structure
        let mir_function = MirFunction {
            symbol_id: tast_function.symbol_id,
            name: tast_function.name.clone(),
            parameters: Vec::new(),
            return_type: self.convert_concrete_type(&tast_function.return_type),
            blocks: HashMap::new(),
            entry_block: BasicBlockId(0),
            locals: HashMap::new(),
            next_value_id: 0,
            next_block_id: 1, // Block 0 is entry block
            attributes: MirFunctionAttributes {
                inline: false,
                pure: false,
                entry_point: tast_function.name == "start",
                exported: tast_function.name == "start",
            },
            location: tast_function.location.clone(),
        };

        // Seed ensure_conditions with any class always: conditions collected by build_class.
        // These are treated identically to `ensure` statements in the method body:
        // they are checked before every `return`.
        let initial_ensure_conditions = self.pending_class_invariants.clone();

        // Create function build context
        let mut context = FunctionBuildContext {
            function: mir_function,
            scope_stack: vec![HashMap::new()],
            loop_stack: Vec::new(),
            class_context: class_context.cloned(),
            all_classes: self.all_classes.clone(),
            all_functions: self.all_functions.clone(),
            // ensure_conditions: starts with any class always: conditions, then grows as
            // Ensure statements are encountered in the function body.
            ensure_conditions: initial_ensure_conditions,
        };

        // For class methods and constructors, add implicit 'this' parameter as the first parameter
        // EXCEPT for static methods which don't need 'this'
        trace!(
            function = %tast_function.name,
            has_class_context = class_context.is_some(),
            is_static = tast_function.is_static,
            "Checking this parameter requirement"
        );
        if let Some(_class_ctx) = class_context {
            if !tast_function.is_static {
                let this_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Create 'this' parameter with class type
                let this_param = MirParameter {
                    value_id: this_value_id,
                    name: "this".to_string(),
                    param_type: MirType::I32, // Instance pointer is i32 in WASM
                    location: tast_function.location.clone(),
                };

                context.function.parameters.push(this_param);

                // Add 'this' to scope
                if let Some(current_scope) = context.scope_stack.last_mut() {
                    current_scope.insert("this".to_string(), this_value_id);
                }
            }
        }

        // Process parameters
        for param in tast_function.parameters.iter() {
            let value_id = ValueId(context.function.next_value_id);
            context.function.next_value_id += 1;

            let mir_param = MirParameter {
                value_id,
                name: param.name.clone(),
                param_type: self.convert_concrete_type(&param.param_type),
                location: param.location.clone(),
            };

            context.function.parameters.push(mir_param);

            // Add parameter to current scope
            if let Some(current_scope) = context.scope_stack.last_mut() {
                current_scope.insert(param.name.clone(), value_id);
            }
        }

        trace!(
            function_name = %context.function.name,
            param_count = context.function.parameters.len(),
            "MIR parameters created for function"
        );
        for (i, mir_param) in context.function.parameters.iter().enumerate() {
            trace!(
                index = i,
                param_name = %mir_param.name,
                param_type = ?mir_param.param_type,
                "MIR parameter"
            );
        }

        // Create entry basic block
        let entry_block = MirBasicBlock {
            id: BasicBlockId(0),
            label: Some("entry".to_string()),
            instructions: Vec::new(),
            terminator: MirTerminator::Unreachable, // Will be replaced
            predecessors: HashSet::new(),
            successors: HashSet::new(),
            location: tast_function.location.clone(),
        };

        context.function.blocks.insert(BasicBlockId(0), entry_block);
        self.current_block = Some(BasicBlockId(0));

        // Lower function body
        debug!(
            function_name = %tast_function.name,
            statement_count = tast_function.body.statements.len(),
            has_class_context = context.class_context.is_some(),
            "Building function body"
        );
        if let Some(ref class) = context.class_context {
            trace!(field_count = class.fields.len(), "Class context fields");
        }

        // Handle automatic return for last expression in non-void functions
        let has_non_void_return = !matches!(tast_function.return_type, ConcreteType::Undefined);
        self.build_function_body(&mut context, &tast_function.body, has_non_void_return)?;

        // Inject state rules checking at operation boundaries (start:, frame:)
        if (tast_function.name == "start" || tast_function.name == "frame")
            && !self.state_rules.is_empty()
        {
            debug!(
                function_name = %tast_function.name,
                rule_count = self.state_rules.len(),
                "Injecting state rules checking at operation boundary"
            );
            self.inject_rules_checking(&mut context)?;
        }

        // Ensure function has proper termination
        self.ensure_function_termination(&mut context, &tast_function.return_type)?;

        // Resolve pending phi nodes
        self.resolve_phi_nodes(&mut context)?;

        self.stats.basic_blocks_created += context.function.blocks.len();
        self.stats.ssa_values_created += context.function.next_value_id;

        if context.function.name == "test" {
            trace!(
                function_name = %context.function.name,
                block_count = context.function.blocks.len(),
                "Final MIR function blocks"
            );
            for (block_id, block) in &context.function.blocks {
                trace!(block_id = ?block_id, terminator = ?block.terminator, "Block terminator");
            }
        }

        Ok(context.function)
    }

    /// Build MIR class functions (constructor and methods).
    /// Class invariants from `always:` blocks are injected after each public method's
    /// return value is computed (debug / `--contracts` builds only).
    pub(super) fn build_class(
        &mut self,
        tast_class: TastClass,
    ) -> Result<Vec<MirFunction>, Vec<CompilerError>> {
        let mut functions = Vec::new();
        let mut errors = Vec::new();

        // Clone class for passing to methods (needed to avoid borrow checker issues)
        let class_for_methods = tast_class.clone();
        let class_invariants = tast_class.invariants.clone();

        // Build all constructors with class context (constructors can use 'this')
        for constructor in tast_class.constructors {
            match self.build_function_with_class_context(constructor, Some(&class_for_methods)) {
                Ok(ctor_function) => functions.push(ctor_function),
                Err(ctor_errors) => errors.extend(ctor_errors),
            }
        }

        // Build all methods with class context.
        // After each method body is built, inject class always: condition checks immediately
        // before every Return terminator (debug / --contracts builds).
        // In release mode, class_invariants are suppressed entirely.
        for method in tast_class.methods {
            // Seed the context with the class always: conditions as pre-return ensure conditions.
            // `build_function_with_class_context` picks them up from `ensure_conditions`.
            // We store them on the builder and thread them into the context there.
            // In release mode: leave pending_class_invariants empty so no checks are emitted.
            self.pending_class_invariants = if self.release_mode {
                Vec::new()
            } else {
                class_invariants.clone()
            };
            match self.build_function_with_class_context(method, Some(&class_for_methods)) {
                Ok(method_function) => functions.push(method_function),
                Err(method_errors) => errors.extend(method_errors),
            }
            self.pending_class_invariants.clear();
        }

        if errors.is_empty() {
            Ok(functions)
        } else {
            Err(errors)
        }
    }

    /// Build function body with automatic return handling
    pub(super) fn build_function_body(
        &mut self,
        context: &mut FunctionBuildContext,
        block: &TastBlock,
        has_non_void_return: bool,
    ) -> Result<(), Vec<CompilerError>> {
        // Enter new scope
        context.scope_stack.push(HashMap::new());

        let num_statements = block.statements.len();

        // Check if the last statement is an expression that should be auto-returned
        let last_is_auto_return = if has_non_void_return && num_statements > 0 {
            matches!(
                block.statements.last(),
                Some(TastStatement::Expression { .. })
            )
        } else {
            false
        };

        // Process all statements except possibly the last
        let statements_to_process = if last_is_auto_return {
            num_statements - 1
        } else {
            num_statements
        };

        for (i, statement) in block
            .statements
            .iter()
            .take(statements_to_process)
            .enumerate()
        {
            trace!(
                statement_index = i,
                statement_type = ?std::mem::discriminant(statement),
                current_block = ?self.current_block,
                "Processing statement"
            );
            self.build_statement(context, statement)?;
            trace!(
                statement_index = i,
                current_block = ?self.current_block,
                "After statement"
            );
        }

        // Handle last statement if it should be auto-returned
        if last_is_auto_return {
            if let Some(TastStatement::Expression {
                expression,
                location: _,
            }) = block.statements.last()
            {
                trace!("Auto-returning last expression");
                // Build expression and convert to return
                let value_id = self.build_expression(context, expression)?;

                // Add return instruction
                if let Some(block_id) = self.current_block {
                    if let Some(current_block) = context.function.blocks.get_mut(&block_id) {
                        current_block.terminator = MirTerminator::Return {
                            value: Some(MirOperand::Value(value_id)),
                        };
                    }
                }
            }
        }

        // Exit scope
        context.scope_stack.pop();

        Ok(())
    }

    /// Helper function to check if a block effectively returns (has Return OR Branch where both branches return)
    pub(super) fn block_effectively_returns(
        &self,
        context: &FunctionBuildContext,
        block_id: BasicBlockId,
    ) -> bool {
        let mut visited = std::collections::HashSet::new();
        self.block_effectively_returns_recursive(context, block_id, &mut visited)
    }

    pub(super) fn block_effectively_returns_recursive(
        &self,
        context: &FunctionBuildContext,
        block_id: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
    ) -> bool {
        // Prevent infinite loops
        if visited.contains(&block_id) {
            return false;
        }
        visited.insert(block_id);

        let block = match context.function.blocks.get(&block_id) {
            Some(b) => b,
            None => return false,
        };

        match &block.terminator {
            MirTerminator::Return { .. } => true,
            MirTerminator::Branch {
                true_block,
                false_block,
                ..
            } => {
                // Both branches must effectively return
                self.block_effectively_returns_recursive(context, *true_block, visited)
                    && self.block_effectively_returns_recursive(context, *false_block, visited)
            }
            MirTerminator::Unreachable | MirTerminator::Jump { .. } => false,
            MirTerminator::Trap => true, // Trap terminates execution
        }
    }

    // -------------------------------------------------------------------------
    // Class JSON serialization
    // -------------------------------------------------------------------------

    /// Generate a `__serialize_ClassName(this: i32) -> Ptr(I8)` MIR function.
    ///
    /// The function walks every field in the class hierarchy (parent fields first)
    /// and builds a JSON object string by concatenating key/value fragments via
    /// `string.concat` (SymbolId 1000).  String values are quoted via
    /// `__json_quote_string` (SymbolId 1011).  Integer/Boolean values are
    /// converted via `int_to_string` (SymbolId 1009).  Number values are
    /// converted via `number_to_string` (SymbolId 1010).
    pub(super) fn build_class_serializer(
        &mut self,
        symbol_id: SymbolId,
        class: &crate::typechecker::tast::TastClass,
        all_classes: &[crate::typechecker::tast::TastClass],
    ) -> MirFunction {
        use crate::typechecker::tast::{ConcreteType, TastField};

        let fn_name = format!("__serialize_{}", class.name);
        let loc = SourceLocation::default();

        // Collect all fields, root-first, for correct byte-offset calculation.
        let fields: Vec<&TastField> = self.collect_serializer_fields(class, all_classes);

        // We build the instruction list and locals directly — no closure needed.
        let mut locals: HashMap<ValueId, MirLocal> = HashMap::new();
        let mut instructions: Vec<MirInstruction> = Vec::new();
        let mut next_vid: usize = 1; // ValueId(0) is the `this` parameter

        // `this` parameter — i32 pointer to the class instance.
        let this_id = ValueId(0);
        locals.insert(
            this_id,
            MirLocal {
                name: Some("this".to_string()),
                local_type: MirType::I32,
                is_mutable: false,
                location: loc.clone(),
            },
        );

        // --- Emit GEP + Load + conversion for each field ---

        struct FieldFragment {
            frag_vid: ValueId,
            field_name: String,
        }

        let mut fragments: Vec<FieldFragment> = Vec::new();
        let mut byte_offset: usize = 0;

        for field in &fields {
            let field_size = self.get_type_byte_size(&field.field_type);

            // GEP: ptr = this + byte_offset
            let ptr_vid = ValueId(next_vid);
            next_vid += 1;
            locals.insert(
                ptr_vid,
                MirLocal {
                    name: Some(format!("ptr_{}", field.name)),
                    local_type: MirType::I32,
                    is_mutable: false,
                    location: loc.clone(),
                },
            );
            instructions.push(MirInstruction {
                dest: Some(ptr_vid),
                operation: MirOperation::GetElementPtr {
                    base: MirOperand::Value(this_id),
                    indices: vec![MirOperand::Constant(MirConstant::Integer(
                        byte_offset as i64,
                    ))],
                    is_array: false,
                },
                location: loc.clone(),
            });

            // Load: val = *ptr
            let field_mir_type = MirType::from_concrete_type(&field.field_type);
            let val_vid = ValueId(next_vid);
            next_vid += 1;
            locals.insert(
                val_vid,
                MirLocal {
                    name: Some(format!("val_{}", field.name)),
                    local_type: field_mir_type.clone(),
                    is_mutable: false,
                    location: loc.clone(),
                },
            );
            instructions.push(MirInstruction {
                dest: Some(val_vid),
                operation: MirOperation::Load {
                    source: MirOperand::Value(ptr_vid),
                },
                location: loc.clone(),
            });

            // Convert field value to a JSON fragment string.
            let frag_vid = ValueId(next_vid);
            next_vid += 1;
            let frag_type = MirType::Ptr(Box::new(MirType::I8));
            locals.insert(
                frag_vid,
                MirLocal {
                    name: Some(format!("frag_{}", field.name)),
                    local_type: frag_type,
                    is_mutable: false,
                    location: loc.clone(),
                },
            );

            let convert_op: MirOperation = match &field.field_type {
                ConcreteType::String => MirOperation::Call {
                    function: MirOperand::Function(SymbolId(1011)), // __json_quote_string
                    arguments: vec![MirOperand::Value(val_vid)],
                },
                ConcreteType::Integer
                | ConcreteType::IntegerSized { .. }
                | ConcreteType::Boolean => MirOperation::Call {
                    function: MirOperand::Function(SymbolId(1009)), // int_to_string
                    arguments: vec![MirOperand::Value(val_vid)],
                },
                ConcreteType::Number | ConcreteType::NumberSized { .. } => MirOperation::Call {
                    function: MirOperand::Function(SymbolId(1010)), // number_to_string
                    arguments: vec![MirOperand::Value(val_vid)],
                },
                ConcreteType::Class {
                    symbol_id: nested_sym,
                    ..
                } => {
                    // Look up the nested class by SymbolId then find its serializer.
                    let maybe_serializer = all_classes
                        .iter()
                        .find(|c| c.symbol_id == *nested_sym)
                        .and_then(|c| self.class_serializer_ids.get(&c.name).copied());
                    if let Some(nested_sid) = maybe_serializer {
                        MirOperation::Call {
                            function: MirOperand::Function(nested_sid),
                            arguments: vec![MirOperand::Value(val_vid)],
                        }
                    } else {
                        // Nested class serializer not available — emit "null".
                        let null_idx = self.get_string_index("null".to_string());
                        MirOperation::Copy {
                            source: MirOperand::Constant(MirConstant::String(null_idx)),
                        }
                    }
                }
                _ => {
                    // Array and other complex types — emit "null" as a safe fallback.
                    let null_idx = self.get_string_index("null".to_string());
                    MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::String(null_idx)),
                    }
                }
            };

            instructions.push(MirInstruction {
                dest: Some(frag_vid),
                operation: convert_op,
                location: loc.clone(),
            });

            fragments.push(FieldFragment {
                frag_vid,
                field_name: field.name.clone(),
            });
            byte_offset += field_size;
        }

        // --- Build the JSON string by concatenating key/value pairs ---

        let result_vid: ValueId = if fragments.is_empty() {
            // No fields → return "{}"
            let empty_idx = self.get_string_index("{}".to_string());
            let vid = ValueId(next_vid);
            next_vid += 1;
            locals.insert(
                vid,
                MirLocal {
                    name: Some("result".to_string()),
                    local_type: MirType::Ptr(Box::new(MirType::I8)),
                    is_mutable: false,
                    location: loc.clone(),
                },
            );
            instructions.push(MirInstruction {
                dest: Some(vid),
                operation: MirOperation::Copy {
                    source: MirOperand::Constant(MirConstant::String(empty_idx)),
                },
                location: loc.clone(),
            });
            vid
        } else {
            // Iteratively build the JSON string: "{" + "\"key\":" + frag + "," + ...  + "}"
            let mut current_vid: Option<ValueId> = None;

            for (i, frag) in fragments.iter().enumerate() {
                let escaped = frag.field_name.replace('\\', "\\\\").replace('"', "\\\"");
                let key_str = if i == 0 {
                    format!("{{\"{}\":", escaped)
                } else {
                    format!(",\"{}\":", escaped)
                };
                let key_idx = self.get_string_index(key_str);

                // Prepend the key to whatever we have so far.
                let before_frag_vid = if let Some(prev_vid) = current_vid {
                    // concat(prev, key)
                    let vid = ValueId(next_vid);
                    next_vid += 1;
                    locals.insert(
                        vid,
                        MirLocal {
                            name: Some(format!("concat_key_{}", i)),
                            local_type: MirType::Ptr(Box::new(MirType::I8)),
                            is_mutable: false,
                            location: loc.clone(),
                        },
                    );
                    instructions.push(MirInstruction {
                        dest: Some(vid),
                        operation: MirOperation::Call {
                            function: MirOperand::Function(SymbolId(1000)), // string.concat
                            arguments: vec![
                                MirOperand::Value(prev_vid),
                                MirOperand::Constant(MirConstant::String(key_idx)),
                            ],
                        },
                        location: loc.clone(),
                    });
                    vid
                } else {
                    // First field: copy the key constant directly.
                    let vid = ValueId(next_vid);
                    next_vid += 1;
                    locals.insert(
                        vid,
                        MirLocal {
                            name: Some("open_brace".to_string()),
                            local_type: MirType::Ptr(Box::new(MirType::I8)),
                            is_mutable: false,
                            location: loc.clone(),
                        },
                    );
                    instructions.push(MirInstruction {
                        dest: Some(vid),
                        operation: MirOperation::Copy {
                            source: MirOperand::Constant(MirConstant::String(key_idx)),
                        },
                        location: loc.clone(),
                    });
                    vid
                };

                // concat(before_frag, frag_value)
                let after_frag_vid = ValueId(next_vid);
                next_vid += 1;
                locals.insert(
                    after_frag_vid,
                    MirLocal {
                        name: Some(format!("after_frag_{}", i)),
                        local_type: MirType::Ptr(Box::new(MirType::I8)),
                        is_mutable: false,
                        location: loc.clone(),
                    },
                );
                instructions.push(MirInstruction {
                    dest: Some(after_frag_vid),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(SymbolId(1000)),
                        arguments: vec![
                            MirOperand::Value(before_frag_vid),
                            MirOperand::Value(frag.frag_vid),
                        ],
                    },
                    location: loc.clone(),
                });

                current_vid = Some(after_frag_vid);
            }

            // Append closing "}"
            let close_idx = self.get_string_index("}".to_string());
            let result_vid = ValueId(next_vid);
            next_vid += 1;
            locals.insert(
                result_vid,
                MirLocal {
                    name: Some("result".to_string()),
                    local_type: MirType::Ptr(Box::new(MirType::I8)),
                    is_mutable: false,
                    location: loc.clone(),
                },
            );
            instructions.push(MirInstruction {
                dest: Some(result_vid),
                operation: MirOperation::Call {
                    function: MirOperand::Function(SymbolId(1000)),
                    arguments: vec![
                        MirOperand::Value(current_vid.unwrap()),
                        MirOperand::Constant(MirConstant::String(close_idx)),
                    ],
                },
                location: loc.clone(),
            });
            result_vid
        };

        // --- Assemble the single entry basic block ---
        let entry_block_id = BasicBlockId(0);
        let mut blocks: HashMap<BasicBlockId, MirBasicBlock> = HashMap::new();
        blocks.insert(
            entry_block_id,
            MirBasicBlock {
                id: entry_block_id,
                label: None,
                instructions,
                terminator: MirTerminator::Return {
                    value: Some(MirOperand::Value(result_vid)),
                },
                predecessors: HashSet::new(),
                successors: HashSet::new(),
                location: loc.clone(),
            },
        );

        MirFunction {
            symbol_id,
            name: fn_name,
            parameters: vec![MirParameter {
                value_id: this_id,
                name: "this".to_string(),
                param_type: MirType::I32,
                location: loc.clone(),
            }],
            return_type: MirType::Ptr(Box::new(MirType::I8)),
            blocks,
            entry_block: entry_block_id,
            locals,
            next_value_id: next_vid,
            next_block_id: 1,
            attributes: MirFunctionAttributes {
                inline: false,
                pure: true,
                entry_point: false,
                exported: false,
            },
            location: loc,
        }
    }

    /// Collect all fields for a class, traversing the hierarchy root-first so
    /// field byte offsets match what the constructor and field-access codegen compute.
    fn collect_serializer_fields<'a>(
        &self,
        class: &'a crate::typechecker::tast::TastClass,
        all_classes: &'a [crate::typechecker::tast::TastClass],
    ) -> Vec<&'a crate::typechecker::tast::TastField> {
        // Walk from the given class up to the root, collecting classes.
        let mut hierarchy: Vec<&crate::typechecker::tast::TastClass> = Vec::new();
        let mut current = Some(class);
        while let Some(cls) = current {
            hierarchy.push(cls);
            current = cls
                .parent_class
                .as_ref()
                .and_then(|pid| all_classes.iter().find(|c| c.symbol_id == *pid));
        }
        // Reverse so root (most distant ancestor) comes first.
        hierarchy.reverse();
        hierarchy
            .into_iter()
            .flat_map(|c| c.fields.iter())
            .collect()
    }
}
