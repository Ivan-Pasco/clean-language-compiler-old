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
            if_merges: Default::default(),
            attributes: MirFunctionAttributes {
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
            error_regions: Vec::new(),
            return_type: tast_function.return_type.clone(),
            later_bindings: HashMap::new(),
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

        // Handle automatic return for last expression in non-void functions.
        // Constructors are excluded: their return value is always `this`, set by
        // `ensure_function_termination` below. Auto-returning the body's last expression
        // would overwrite the implicit `return this` with whatever the body produced
        // (e.g. the void result of a `base(arg)` call, which leaves the caller with an
        // uninitialized instance pointer — CLASS001 inheritance bug).
        let is_constructor = context.class_context.is_some() && tast_function.name == "constructor";
        let has_non_void_return =
            !matches!(tast_function.return_type, ConcreteType::Undefined) && !is_constructor;
        if is_constructor && !tast_function.is_static {
            self.initialize_field_defaults(&mut context, &tast_function.location)?;
        }
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

        Ok(context.function)
    }

    /// Store every field's declared initializer (`integer count = 5`, grammar
    /// `class_field`) into the new instance before the constructor body runs,
    /// parent class fields first. The initializers were type-checked but never
    /// lowered, so such fields started at zero whatever they declared.
    fn initialize_field_defaults(
        &mut self,
        context: &mut FunctionBuildContext,
        location: &SourceLocation,
    ) -> Result<(), Vec<CompilerError>> {
        let Some(class) = context.class_context.clone() else {
            return Ok(());
        };
        let mut hierarchy = vec![class];
        while let Some(parent) = hierarchy.last().and_then(|c| c.parent_class) {
            match context.all_classes.iter().find(|c| c.symbol_id == parent) {
                Some(parent_class) => hierarchy.push(parent_class.clone()),
                None => break,
            }
        }
        let this_type = ConcreteType::Class {
            symbol_id: hierarchy[0].symbol_id,
            type_args: Vec::new(),
        };
        for class in hierarchy.iter().rev() {
            for field in &class.fields {
                let Some(initializer) = &field.default_value else {
                    continue;
                };
                // The type checker turns every empty `[]` / `{}` field
                // initializer into an empty list literal; a `pairs` field needs
                // an empty map (the pairs layout, not the list layout).
                let initializer = match (&field.field_type, &initializer.kind) {
                    (
                        ConcreteType::Pairs(_, _),
                        TastExpressionKind::ArrayLiteral { elements, .. },
                    ) if elements.is_empty() => TastExpression {
                        kind: TastExpressionKind::ObjectLiteral { fields: Vec::new() },
                        expr_type: field.field_type.clone(),
                        location: initializer.location.clone(),
                    },
                    _ => initializer.clone(),
                };
                let this = TastExpression {
                    kind: TastExpressionKind::Variable {
                        symbol_id: SymbolId(0),
                        name: "this".to_string(),
                    },
                    expr_type: this_type.clone(),
                    location: location.clone(),
                };
                let assignment = TastStatement::Assignment {
                    target: TastExpression {
                        kind: TastExpressionKind::PropertyAccess {
                            object: Box::new(this),
                            property_name: field.name.clone(),
                            property_symbol: field.symbol_id,
                        },
                        expr_type: field.field_type.clone(),
                        location: field.location.clone(),
                    },
                    value: initializer,
                    location: field.location.clone(),
                };
                self.build_statement(context, &assignment)?;
            }
        }
        Ok(())
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

        self.build_statement_list(context, &block.statements[..statements_to_process])?;

        // Handle last statement if it should be auto-returned (when the
        // statements before it can reach it)
        if last_is_auto_return && !self.current_block_terminated(context) {
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
    // Class JSON values
    // -------------------------------------------------------------------------

    /// Generate a `__json_tree_ClassName(this: i32) -> any` MIR function.
    ///
    /// It builds the boxed JSON object of a class instance — one member per
    /// field, parent fields first — in the JSON-tree layout of
    /// BOXED_ANY_ABI §3.3, so `json.encode(instance)` and the other JSON
    /// bridges serialize it like any other `any` object. Each member value
    /// is boxed by its field type: primitives directly, nested instances
    /// through `__json_tree_instance` (the builder of the class named by the
    /// instance header), lists and pairs of primitives through the
    /// typed-collection converters, lists of class instances through
    /// `__json_tree_list_*`, and an `any` field is its boxed value (the JSON
    /// bridges pass the whole tree through `__json_normalize`, which turns
    /// instances boxed in `any` values into objects). A field with no JSON
    /// representation is `null`.
    pub(super) fn build_class_json_tree(
        &mut self,
        symbol_id: SymbolId,
        class: &crate::typechecker::tast::TastClass,
        all_classes: &[crate::typechecker::tast::TastClass],
    ) -> MirFunction {
        use crate::typechecker::tast::TastField;

        let fn_name = format!("__json_tree_{}", class.name);
        let loc = SourceLocation::default();

        // Collect all fields, root-first, for correct byte-offset calculation.
        let fields: Vec<&TastField> = self.collect_serializer_fields(class, all_classes);

        let mut locals: HashMap<ValueId, MirLocal> = HashMap::new();
        let mut instructions: Vec<MirInstruction> = Vec::new();
        let mut next_vid: usize = 1; // ValueId(0) is the `this` parameter
        let mut new_local = |locals: &mut HashMap<ValueId, MirLocal>, name: String, ty: MirType| {
            let vid = ValueId(next_vid);
            next_vid += 1;
            locals.insert(
                vid,
                MirLocal {
                    name: Some(name),
                    local_type: ty,
                    is_mutable: false,
                    location: SourceLocation::default(),
                },
            );
            vid
        };
        let push = |instructions: &mut Vec<MirInstruction>, dest: Option<ValueId>, operation| {
            instructions.push(MirInstruction {
                dest,
                operation,
                location: SourceLocation::default(),
            });
        };

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

        // The object fragment: [count:i32][(key_ptr, val_ptr) × count].
        let object_vid = new_local(&mut locals, "object".to_string(), MirType::I32);
        push(
            &mut instructions,
            Some(object_vid),
            MirOperation::Call {
                function: MirOperand::NamedFunction {
                    name: "__malloc".to_string(),
                    symbol_id: SymbolId(0),
                },
                arguments: vec![MirOperand::Constant(MirConstant::Integer(
                    (4 + 8 * fields.len()) as i64,
                ))],
            },
        );
        push(
            &mut instructions,
            None,
            MirOperation::Store {
                destination: MirOperand::Value(object_vid),
                value: MirOperand::Constant(MirConstant::Integer(fields.len() as i64)),
            },
        );

        // Start past the per-instance object header (class-id at offset 0).
        // See `helpers.rs::OBJECT_HEADER_SIZE`.
        let mut byte_offset: usize = Self::OBJECT_HEADER_SIZE;

        for (index, field) in fields.iter().enumerate() {
            let field_size = self.get_type_byte_size(&field.field_type);

            // val = *(this + byte_offset)
            let ptr_vid = new_local(&mut locals, format!("ptr_{}", field.name), MirType::I32);
            push(
                &mut instructions,
                Some(ptr_vid),
                MirOperation::GetElementPtr {
                    base: MirOperand::Value(this_id),
                    indices: vec![MirOperand::Constant(MirConstant::Integer(
                        byte_offset as i64,
                    ))],
                    is_array: false,
                },
            );
            let field_mir_type = MirType::from_concrete_type(&field.field_type);
            let val_vid = new_local(
                &mut locals,
                format!("val_{}", field.name),
                field_mir_type.clone(),
            );
            push(
                &mut instructions,
                Some(val_vid),
                MirOperation::Load {
                    source: MirOperand::Value(ptr_vid),
                },
            );

            // The member's boxed JSON value.
            let member_vid = new_local(&mut locals, format!("json_{}", field.name), MirType::Any);
            let null_box = MirOperation::BoxAny {
                value: MirOperand::Constant(MirConstant::Integer(0)),
                type_tag: AnyTypeTag::Null,
                source_type: MirType::I32,
            };
            let is_primitive = |t: &ConcreteType| {
                matches!(
                    t,
                    ConcreteType::String
                        | ConcreteType::Boolean
                        | ConcreteType::Integer
                        | ConcreteType::IntegerSized { .. }
                        | ConcreteType::Number
                        | ConcreteType::NumberSized { .. }
                )
            };
            let nested_descriptor = Self::nested_collection_descriptor(&field.field_type);
            let member_op = match &field.field_type {
                // A list of collections, or a pairs map of collections,
                // `any` values or instances: boxed at every level, then the
                // instances inside become their objects.
                _ if nested_descriptor.is_some() => {
                    let descriptor_index =
                        self.get_string_index(nested_descriptor.unwrap_or_default());
                    let boxed_vid =
                        new_local(&mut locals, format!("boxed_{}", field.name), MirType::Any);
                    push(
                        &mut instructions,
                        Some(boxed_vid),
                        MirOperation::Call {
                            function: MirOperand::NamedFunction {
                                name: crate::stdlib::json_class::CLN_TO_ANY.to_string(),
                                symbol_id: SymbolId(0),
                            },
                            arguments: vec![
                                MirOperand::Value(val_vid),
                                MirOperand::Constant(MirConstant::String(descriptor_index)),
                                MirOperand::Constant(MirConstant::Integer(0)),
                            ],
                        },
                    );
                    MirOperation::Call {
                        function: MirOperand::NamedFunction {
                            name: "__json_normalize".to_string(),
                            symbol_id: SymbolId(0),
                        },
                        arguments: vec![MirOperand::Value(boxed_vid)],
                    }
                }
                // An `integer:64` field keeps its exact value: a tag-1 box
                // holds a full i64 (BOXED_ANY_ABI §2).
                ConcreteType::IntegerSized { bits: 64, .. } => MirOperation::BoxAny {
                    value: MirOperand::Value(val_vid),
                    type_tag: AnyTypeTag::Integer,
                    source_type: field_mir_type,
                },
                t if is_primitive(t) => MirOperation::BoxAny {
                    value: MirOperand::Value(val_vid),
                    type_tag: Self::get_any_type_tag(t),
                    source_type: field_mir_type,
                },
                // A nested instance (or null) is the object of its own class,
                // read from its header: a subclass keeps its fields.
                ConcreteType::Class { .. } | ConcreteType::Interface { .. } => MirOperation::Call {
                    function: MirOperand::NamedFunction {
                        name: "__json_tree_instance".to_string(),
                        symbol_id: SymbolId(0),
                    },
                    arguments: vec![MirOperand::Value(val_vid)],
                },
                // An `any` field already holds a boxed value, and an
                // optional primitive field its box (0 → null).
                t if matches!(t, ConcreteType::Any) || t.optional_primitive().is_some() => {
                    MirOperation::Call {
                        function: MirOperand::NamedFunction {
                            name: "__json_try_decode_v2".to_string(),
                            symbol_id: SymbolId(0),
                        },
                        arguments: vec![MirOperand::Value(val_vid)],
                    }
                }
                ConcreteType::Array(element)
                    if matches!(**element, ConcreteType::Any | ConcreteType::Class { .. })
                        || element.optional_primitive().is_some() =>
                {
                    let class_list_fn = match &**element {
                        ConcreteType::Class {
                            symbol_id: element_sym,
                            ..
                        } => all_classes
                            .iter()
                            .find(|c| c.symbol_id == *element_sym)
                            .and_then(|c| self.class_json_list_ids.get(&c.name).copied()),
                        _ => None,
                    };
                    let tree_vid =
                        new_local(&mut locals, format!("tree_{}", field.name), MirType::I32);
                    push(
                        &mut instructions,
                        Some(tree_vid),
                        MirOperation::Call {
                            function: match class_list_fn {
                                Some(list_fn) => MirOperand::Function(list_fn),
                                None => MirOperand::NamedFunction {
                                    name: "__json_from_cln_list_boxed".to_string(),
                                    symbol_id: SymbolId(0),
                                },
                            },
                            arguments: vec![MirOperand::Value(val_vid)],
                        },
                    );
                    MirOperation::BoxAny {
                        value: MirOperand::Value(tree_vid),
                        type_tag: AnyTypeTag::List,
                        source_type: MirType::I32,
                    }
                }
                ConcreteType::Array(element) if is_primitive(element) => {
                    let converter =
                        if matches!(**element, ConcreteType::IntegerSized { bits: 64, .. }) {
                            "__json_from_cln_list_i64"
                        } else {
                            "__json_from_cln_list"
                        };
                    let tree_vid =
                        new_local(&mut locals, format!("tree_{}", field.name), MirType::I32);
                    push(
                        &mut instructions,
                        Some(tree_vid),
                        Self::collection_tree_call(converter, val_vid, element),
                    );
                    MirOperation::BoxAny {
                        value: MirOperand::Value(tree_vid),
                        type_tag: AnyTypeTag::List,
                        source_type: MirType::I32,
                    }
                }
                ConcreteType::Pairs(_, value) if is_primitive(value) => {
                    let tree_vid =
                        new_local(&mut locals, format!("tree_{}", field.name), MirType::I32);
                    push(
                        &mut instructions,
                        Some(tree_vid),
                        Self::collection_tree_call("__json_from_cln_pairs", val_vid, value),
                    );
                    MirOperation::BoxAny {
                        value: MirOperand::Value(tree_vid),
                        type_tag: AnyTypeTag::Object,
                        source_type: MirType::I32,
                    }
                }
                _ => null_box,
            };
            push(&mut instructions, Some(member_vid), member_op);

            // key_ptr at object + 4 + 8·i, val_ptr at object + 8 + 8·i.
            let key_idx = self.get_string_index(field.name.clone());
            let key_vid = new_local(&mut locals, format!("key_{}", field.name), MirType::I32);
            push(
                &mut instructions,
                Some(key_vid),
                MirOperation::Copy {
                    source: MirOperand::Constant(MirConstant::String(key_idx)),
                },
            );
            for (slot, value_vid) in [(4, key_vid), (8, member_vid)] {
                let slot_vid = new_local(
                    &mut locals,
                    format!("slot_{}_{}", field.name, slot),
                    MirType::I32,
                );
                push(
                    &mut instructions,
                    Some(slot_vid),
                    MirOperation::BinaryOp {
                        op: MirBinaryOp::Add,
                        left: MirOperand::Value(object_vid),
                        right: MirOperand::Constant(MirConstant::Integer(
                            (slot + 8 * index) as i64,
                        )),
                    },
                );
                push(
                    &mut instructions,
                    None,
                    MirOperation::Store {
                        destination: MirOperand::Value(slot_vid),
                        value: MirOperand::Value(value_vid),
                    },
                );
            }

            byte_offset += field_size;
        }

        // Box the fragment as an object.
        let result_vid = new_local(&mut locals, "result".to_string(), MirType::Any);
        push(
            &mut instructions,
            Some(result_vid),
            MirOperation::BoxAny {
                value: MirOperand::Value(object_vid),
                type_tag: AnyTypeTag::Object,
                source_type: MirType::I32,
            },
        );

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
            return_type: MirType::Any,
            blocks,
            entry_block: entry_block_id,
            locals,
            next_value_id: next_vid,
            next_block_id: 1,
            if_merges: Default::default(),
            attributes: MirFunctionAttributes {
                entry_point: false,
                exported: false,
            },
            location: loc,
        }
    }

    /// `__json_tree_list_ClassName(list: i32) -> i32` — the JSON-tree array
    /// (`[count][boxed_elem_ptr]…`, BOXED_ANY_ABI §3.2) of a
    /// `list<ClassName>`: every element becomes the object of its own class
    /// (`__json_tree_instance`), or null.
    pub(super) fn build_class_json_list(symbol_id: SymbolId, class_name: &str) -> MirFunction {
        const LIST_DATA_OFFSET: i64 = 16;
        let loc = SourceLocation::default();
        let mut locals: HashMap<ValueId, MirLocal> = HashMap::new();
        let mut next_vid: usize = 0;
        let mut new_local = |name: &str| {
            let vid = ValueId(next_vid);
            next_vid += 1;
            locals.insert(
                vid,
                MirLocal {
                    name: Some(name.to_string()),
                    local_type: MirType::I32,
                    is_mutable: true,
                    location: SourceLocation::default(),
                },
            );
            vid
        };
        let list = new_local("list");
        let count = new_local("count");
        let bytes = new_local("bytes");
        let array = new_local("array");
        let index = new_local("i");
        let more = new_local("more");
        let offset = new_local("offset");
        let source = new_local("source");
        let element = new_local("element");
        let tree = new_local("tree");
        let dest = new_local("dest");
        let instruction = |dest: Option<ValueId>, operation| MirInstruction {
            dest,
            operation,
            location: SourceLocation::default(),
        };
        let value = MirOperand::Value;
        let int = |n: i64| MirOperand::Constant(MirConstant::Integer(n));
        let binary = |op, left, right| MirOperation::BinaryOp { op, left, right };

        let entry = vec![
            instruction(
                Some(count),
                MirOperation::Load {
                    source: value(list),
                },
            ),
            instruction(Some(bytes), binary(MirBinaryOp::Mul, value(count), int(4))),
            instruction(Some(bytes), binary(MirBinaryOp::Add, value(bytes), int(4))),
            instruction(
                Some(array),
                MirOperation::Call {
                    function: MirOperand::NamedFunction {
                        name: "__malloc".to_string(),
                        symbol_id: SymbolId(0),
                    },
                    arguments: vec![value(bytes)],
                },
            ),
            instruction(
                None,
                MirOperation::Store {
                    destination: value(array),
                    value: value(count),
                },
            ),
            instruction(Some(index), MirOperation::Copy { source: int(0) }),
        ];
        let header = vec![instruction(
            Some(more),
            binary(MirBinaryOp::Lt, value(index), value(count)),
        )];
        let body = vec![
            instruction(Some(offset), binary(MirBinaryOp::Mul, value(index), int(4))),
            instruction(
                Some(source),
                binary(MirBinaryOp::Add, value(list), value(offset)),
            ),
            instruction(
                Some(source),
                binary(MirBinaryOp::Add, value(source), int(LIST_DATA_OFFSET)),
            ),
            instruction(
                Some(element),
                MirOperation::Load {
                    source: value(source),
                },
            ),
            instruction(
                Some(tree),
                MirOperation::Call {
                    function: MirOperand::NamedFunction {
                        name: "__json_tree_instance".to_string(),
                        symbol_id: SymbolId(0),
                    },
                    arguments: vec![value(element)],
                },
            ),
            instruction(
                Some(dest),
                binary(MirBinaryOp::Add, value(array), value(offset)),
            ),
            instruction(Some(dest), binary(MirBinaryOp::Add, value(dest), int(4))),
            instruction(
                None,
                MirOperation::Store {
                    destination: value(dest),
                    value: value(tree),
                },
            ),
            instruction(Some(index), binary(MirBinaryOp::Add, value(index), int(1))),
        ];

        let (entry_id, header_id, body_id, exit_id) = (
            BasicBlockId(0),
            BasicBlockId(1),
            BasicBlockId(2),
            BasicBlockId(3),
        );
        let mut blocks: HashMap<BasicBlockId, MirBasicBlock> = HashMap::new();
        for (id, label, instructions, terminator) in [
            (
                entry_id,
                None,
                entry,
                MirTerminator::Jump { target: header_id },
            ),
            (
                header_id,
                Some("while_header"),
                header,
                MirTerminator::Branch {
                    condition: value(more),
                    true_block: body_id,
                    false_block: exit_id,
                },
            ),
            (
                body_id,
                Some("while_body"),
                body,
                MirTerminator::Jump { target: header_id },
            ),
            (
                exit_id,
                Some("while_exit"),
                Vec::new(),
                MirTerminator::Return {
                    value: Some(value(array)),
                },
            ),
        ] {
            blocks.insert(
                id,
                MirBasicBlock {
                    id,
                    label: label.map(str::to_string),
                    instructions,
                    terminator,
                    location: loc.clone(),
                },
            );
        }

        MirFunction {
            symbol_id,
            name: format!("__json_tree_list_{}", class_name),
            parameters: vec![MirParameter {
                value_id: list,
                name: "list".to_string(),
                param_type: MirType::I32,
                location: loc.clone(),
            }],
            return_type: MirType::I32,
            blocks,
            entry_block: entry_id,
            locals,
            next_value_id: next_vid,
            next_block_id: 4,
            if_merges: Default::default(),
            attributes: MirFunctionAttributes {
                entry_point: false,
                exported: false,
            },
            location: loc,
        }
    }

    /// The call that converts a typed list or pairs field into its JSON tree
    /// fragment (`__json_from_cln_list` / `__json_from_cln_pairs`).
    fn collection_tree_call(
        converter: &str,
        collection_vid: ValueId,
        element_type: &ConcreteType,
    ) -> MirOperation {
        MirOperation::Call {
            function: MirOperand::NamedFunction {
                name: converter.to_string(),
                symbol_id: SymbolId(0),
            },
            arguments: vec![
                MirOperand::Value(collection_vid),
                MirOperand::Constant(MirConstant::Integer(
                    Self::get_any_type_tag(element_type) as i64
                )),
            ],
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
