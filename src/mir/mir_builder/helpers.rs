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

        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                block.instructions.push(instruction);
            }
        }
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

    /// Calculate the total byte size of all fields in a class hierarchy
    ///
    /// This traverses the class hierarchy and sums up the byte sizes of all fields,
    /// accounting for different type sizes (i32=4, i64/f64=8, etc.)
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

        // Sum up byte sizes for all fields
        let mut total_bytes = 0usize;
        for class in &hierarchy {
            for field in &class.fields {
                total_bytes += self.get_type_byte_size(&field.field_type);
            }
        }

        tracing::debug!(
            "calculate_instance_byte_size: class {:?} needs {} bytes",
            class_symbol,
            total_bytes
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
            ConcreteType::Tuple(_) => 4,           // Pointer
            ConcreteType::Union(_) => 4,           // Pointer (boxed)
            ConcreteType::Intersection(_) => 4,    // Pointer (boxed)
            ConcreteType::Generic { .. } => 4,     // Treat as pointer
            ConcreteType::Unknown => 4,            // Default
            ConcreteType::Never => 0,              // Never returns
            ConcreteType::Namespace => 4,          // Not really used in memory
            ConcreteType::Any => 12,               // Boxed: [tag:i32][value1:i32][value2:i32]
            // Optional<T> has the same runtime representation as T (none = 0/null)
            ConcreteType::Optional(inner) => self.get_type_byte_size(inner),
        }
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

        // Calculate byte offset
        let mut byte_offset = 0usize;

        for class in &hierarchy {
            for field in &class.fields {
                if field.symbol_id == *property_symbol {
                    tracing::debug!(
                        "calculate_field_byte_offset: field {:?} at byte offset {}",
                        property_symbol,
                        byte_offset
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
                TastStatement::Try {
                    body,
                    catch_clause,
                    finally_clause,
                    ..
                } => {
                    Self::collect_inner_decls(&body.statements, out);
                    if let Some(c) = catch_clause {
                        Self::collect_inner_decls(&c.body.statements, out);
                    }
                    if let Some(f) = finally_clause {
                        Self::collect_inner_decls(&f.statements, out);
                    }
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
                // `throw` unwinds past the body-end pop the same way
                // `return` does. `background` fires off a closure that
                // retains body-local pointers across the pop.
                TastStatement::Throw { .. } | TastStatement::Background { .. } => return true,
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
                TastStatement::Try {
                    body,
                    catch_clause,
                    finally_clause,
                    ..
                } => {
                    if Self::body_escapes_or_returns(&body.statements, inner) {
                        return true;
                    }
                    if let Some(c) = catch_clause {
                        if Self::body_escapes_or_returns(&c.body.statements, inner) {
                            return true;
                        }
                    }
                    if let Some(f) = finally_clause {
                        if Self::body_escapes_or_returns(&f.statements, inner) {
                            return true;
                        }
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
            | ConcreteType::Tuple(_)
            | ConcreteType::Any => true,
            ConcreteType::Optional(inner) => Self::is_arena_alloc_type(inner),
            ConcreteType::Union(parts) | ConcreteType::Intersection(parts) => {
                parts.iter().any(Self::is_arena_alloc_type)
            }
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
