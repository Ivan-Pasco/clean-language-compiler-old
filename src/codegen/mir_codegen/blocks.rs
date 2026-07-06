//! Basic-block and structured control-flow emission for MIR code generation.
//!
//! Contains:
//! * `generate_function`        — top-level per-function driver
//! * `generate_basic_block`     — linear block emission (used by legacy path)
//! * `generate_branch_block`    — branch body emission (no Jump-following)
//! * `generate_structured_blocks` — recursive structured-CF traversal

use super::*;
use wasm_encoder::{Function as WasmFunction, Instruction};

impl MirCodeGenerator<'_> {
    /// Push a zero constant of the given return type onto the instruction stream.
    ///
    /// Used to satisfy the WASM validator's stack-shape requirement when emitting
    /// a bare `return` for a non-void function where no value has been produced
    /// (implicit fall-through returns from `ensure_function_termination`, or
    /// `Return { value: None }` in synthesized blocks). For void return types
    /// this is a no-op.
    pub(super) fn push_zero_for_return_type(&mut self, return_type: &Option<MirType>) {
        match return_type {
            Some(MirType::F64) => {
                self.current_instructions.push(Instruction::F64Const(0.0));
            }
            Some(
                MirType::I32
                | MirType::I8
                | MirType::I16
                | MirType::U8
                | MirType::U16
                | MirType::U32
                | MirType::Ptr(_),
            ) => {
                self.current_instructions.push(Instruction::I32Const(0));
            }
            Some(MirType::I64 | MirType::U64) => {
                self.current_instructions.push(Instruction::I64Const(0));
            }
            _ => {}
        }
    }

    /// Generate WASM function from MIR function.
    pub(super) fn generate_function(
        &mut self,
        function: MirFunction,
    ) -> Result<FunctionStats, CompilerError> {
        tracing::debug!(
            name = %function.name,
            blocks = function.blocks.len(),
            "Starting generate_function"
        );
        let mut stats = FunctionStats::default();

        // Reset per-function state
        self.value_to_local.clear();
        self.block_labels.clear();
        self.value_to_string_index.clear();
        self.value_to_type.clear();
        self.temp_local_types.clear();
        self.next_local_index = 0;
        self.next_block_label = 0;
        self.current_instructions.clear();
        self.current_function = Some(function.clone());
        self.loop_context_stack.clear();
        self.current_block_depth = 0;
        self.outer_if_continuations.clear();

        // Populate value_to_type from function parameters
        debug_mir!(
            function_name = %function.name,
            "Populating value_to_type for function"
        );
        for param in &function.parameters {
            debug_mir!(
                value_id = param.value_id.0,
                param_type = ?param.param_type,
                "Parameter type mapping"
            );
            self.value_to_type
                .insert(param.value_id, param.param_type.clone());
        }

        // Populate value_to_type from function locals
        for (value_id, local) in &function.locals {
            debug_mir!(
                value_id = value_id.0,
                local_type = ?local.local_type,
                "Local type mapping"
            );
            self.value_to_type
                .insert(*value_id, local.local_type.clone());
        }

        // Convert MIR function signature to WASM
        let wasm_signature = self.convert_function_signature(&function)?;

        // Allocate locals for function parameters
        debug_mir!(
            function_name = %function.name,
            parameters = function.parameters.len(),
            "Allocating parameter locals"
        );
        for param in &function.parameters {
            let local_index = self.next_local_index;
            debug_mir!(
                param_name = %param.name,
                value_id = param.value_id.0,
                local_index = local_index,
                "Adding parameter to local"
            );
            self.value_to_local.insert(param.value_id, local_index);
            self.next_local_index += 1;
        }
        debug_mir!(
            entries = self.value_to_local.len(),
            "value_to_local entries after parameters"
        );

        // NOTE: Allocate locals for function local variables (excluding parameters)
        // Parameters are already in function.locals, so we must skip them to avoid duplication
        // IMPORTANT: Sort by ValueId to ensure consistent allocation order!
        debug_mir!(
            function_name = %function.name,
            locals_count = function.locals.len(),
            parameters = function.parameters.len(),
            "Function locals allocation"
        );

        // Collect and sort locals by ValueId for deterministic allocation
        let mut sorted_locals: Vec<_> = function.locals.iter().collect();
        sorted_locals.sort_by_key(|(value_id, _)| value_id.0);

        for (value_id, _local) in sorted_locals {
            // Skip if this ValueId was already allocated (i.e., it's a parameter)
            if self.value_to_local.contains_key(value_id) {
                debug_mir!(
                    value_id = value_id.0,
                    "Skipping ValueId - already allocated as parameter"
                );
                continue;
            }

            let local_index = self.next_local_index;
            debug_mir!(
                value_id = value_id.0,
                local_index = local_index,
                "Adding ValueId to local"
            );
            self.value_to_local.insert(*value_id, local_index);
            self.next_local_index += 1;
        }
        debug_mir!(
            entries = self.value_to_local.len(),
            "After locals allocation, value_to_local entries"
        );

        // Pre-assign block labels
        for &block_id in function.blocks.keys() {
            self.block_labels.insert(block_id, self.next_block_label);
            self.next_block_label += 1;
        }

        // NOTE: Use function.entry_block instead of hardcoded BasicBlockId(0)
        // Functions whose entry block was renumbered will now emit code correctly
        let entry_block_id = function.entry_block;
        tracing::debug!(
            entry_block = ?entry_block_id,
            name = %function.name,
            "Starting code generation from entry block"
        );

        let mut generated_blocks = std::collections::HashSet::new();
        self.generate_structured_blocks(&function, entry_block_id, &mut generated_blocks)?;
        debug_mir!(
            function_name = %function.name,
            instructions = self.current_instructions.len(),
            "After generate_structured_blocks"
        );

        stats.blocks_generated = generated_blocks.len();
        debug_mir!(
            "DEBUG MIR: Generated {} blocks using structured control flow",
            stats.blocks_generated
        );

        // Create WASM function with generated instructions
        tracing::debug!(
            name = %function.name,
            "Computing local types for function"
        );
        let local_types = self.compute_local_types(&function);
        tracing::debug!(
            local_types = local_types.len(),
            instructions = self.current_instructions.len(),
            "Creating WASM function"
        );
        // NOTE: For void functions, check if we need to drop a value
        // This prevents "type mismatch at end of function, expected [] but got [X]" errors
        // NOTE: Ptr(Void) represents the "any" type and DOES return a value (i32), so don't treat it as void
        let is_void_function = matches!(function.return_type, MirType::Void);

        debug_mir!(
            function_name = %function.name,
            is_void = is_void_function,
            instructions = self.current_instructions.len(),
            "Void function check"
        );
        if is_void_function && self.current_instructions.len() >= 10 {
            debug_mir!("Last 10 instructions:");
            for (i, inst) in self.current_instructions.iter().rev().take(10).enumerate() {
                debug_mir!(index = -(i as i32 + 1), instruction = ?inst, "Instruction");
            }
        }

        debug_mir!(
            function_name = %function.name,
            instructions = self.current_instructions.len(),
            local_types = local_types.len(),
            "Before copy to WASM function"
        );
        let mut wasm_function = WasmFunction::new(local_types);
        let mut instruction_count = 0;
        debug_mir!(
            instructions = self.current_instructions.len(),
            function_name = %function.name,
            "Copying instructions"
        );
        for (idx, instruction) in self.current_instructions.iter().enumerate() {
            if matches!(instruction, Instruction::Drop) {
                debug_mir!(idx = idx, "Instruction: DROP");
            }
            if let Instruction::Call(func_idx) = instruction {
                debug_mir!(idx = idx, func_idx = func_idx, "Instruction: Call");
                if *func_idx == u32::MAX {
                    panic!(
                        "BUG: Call(u32::MAX) in function '{}' at instruction index {} — tree-shaken import index leaked into MIR function body",
                        function.name, idx
                    );
                }
            }
            wasm_function.instruction(instruction);
            instruction_count += 1;
        }
        debug_mir!(
            function_name = %function.name,
            instructions_copied = instruction_count,
            "After copy to WASM function"
        );

        // NOTE: Void functions don't need a final DROP instruction.
        // With structured control flow generation, all execution paths are properly handled:
        // - Paths with explicit returns will have Return instructions
        // - Paths that fall through will naturally END the function (valid for void functions)
        // Adding a DROP here causes validation errors when the stack is already empty.

        // CONSTRUCTOR FIX: Add implicit return of 'this' pointer for constructors
        // Constructors must return the instance pointer (i32) which is parameter 0
        let is_constructor =
            function.name == "constructor" || function.name.ends_with(".constructor");
        if is_constructor {
            debug_mir!(
                function_name = %function.name,
                "Adding implicit return of 'this' for constructor"
            );
            wasm_function.instruction(&Instruction::LocalGet(0));
        }

        // NOTE: For non-void functions, ensure all paths return
        // If the function returns a value and doesn't end with a Return instruction, add Unreachable
        // NOTE: Check for both Void and Ptr(Void)
        // Ptr(Void) represents a void function (no return value) in some code paths
        let is_void_return = matches!(function.return_type, MirType::Void)
            || matches!(&function.return_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void));
        let is_non_void = !is_void_return;
        let last_instruction_is_return = self
            .current_instructions
            .last()
            .is_some_and(|inst| matches!(inst, Instruction::Return | Instruction::Unreachable));

        if is_non_void && !last_instruction_is_return && !is_constructor {
            debug_mir!(
                function_name = %function.name,
                return_type = ?function.return_type,
                "Non-void function missing Return/Unreachable, adding Unreachable"
            );
            wasm_function.instruction(&Instruction::Unreachable);
        }

        // CRITICAL: Add END instruction to properly close the function
        wasm_function.instruction(&Instruction::End);

        // Add function to WASM module
        tracing::debug!(
            name = %function.name,
            instructions = self.current_instructions.len(),
            "Adding function to WASM module"
        );
        self.add_function_to_module(function.name.clone(), wasm_function, wasm_signature)?;
        tracing::debug!(
            name = %function.name,
            "Successfully added function to WASM module"
        );

        Ok(stats)
    }

    /// Generate WASM instructions for a basic block.
    #[allow(dead_code)] // Used internally by generate_function
    pub(super) fn generate_basic_block(
        &mut self,
        block: &MirBasicBlock,
    ) -> Result<(), CompilerError> {
        tracing::trace!(
            predecessors = block.predecessors.len(),
            "Starting basic block generation"
        );

        // Start block if it has predecessors (not entry block)
        if !block.predecessors.is_empty() {
            if let Some(&label) = self.block_labels.get(&block.id) {
                self.current_instructions
                    .push(Instruction::Block(BlockType::Empty));
                debug_mir!(label = label, "Added Block instruction for label");
            }
        }

        // Generate instructions
        tracing::trace!(
            instructions = block.instructions.len(),
            "Generating block instructions"
        );
        for (i, instruction) in block.instructions.iter().enumerate() {
            debug_mir!("DEBUG MIR: Processing instruction {}: {:?}", i, instruction);
            self.generate_instruction(instruction)?;
        }

        // Generate terminator
        debug_mir!("DEBUG MIR: Generating terminator: {:?}", block.terminator);
        self.generate_terminator(&block.terminator)?;

        // End block if it was started
        if !block.predecessors.is_empty() {
            self.current_instructions.push(Instruction::End);
        }

        debug_mir!("DEBUG MIR: generate_basic_block completed successfully");
        Ok(())
    }

    /// Generate a branch block body (for if/else branches) without following Jump terminators.
    ///
    /// Jump terminators in branch blocks represent exits to continuation blocks that should
    /// be generated after the if-else structure, not inside the branch.
    pub(super) fn generate_branch_block(
        &mut self,
        function: &MirFunction,
        block_id: BasicBlockId,
        generated: &mut std::collections::HashSet<BasicBlockId>,
    ) -> Result<(), CompilerError> {
        // Skip if already generated
        if generated.contains(&block_id) {
            debug_mir!(
                "DEBUG BRANCH_BLOCK: Skipping already-generated block {:?} in function '{}'",
                block_id,
                function.name
            );
            return Ok(());
        }
        debug_mir!(
            "DEBUG BRANCH_BLOCK: Inserting block {:?} into generated set for function '{}'",
            block_id,
            function.name
        );
        generated.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            return Ok(());
        };

        // Generate block instructions
        debug_mir!(
            "DEBUG BRANCH_BLOCK: Block {:?} has {} instructions in function '{}'",
            block_id,
            block.instructions.len(),
            function.name
        );
        for instruction in &block.instructions {
            self.generate_instruction(instruction)?;
        }

        // Handle terminator - but DON'T follow Jump terminators (those are exits to continuations)
        debug_mir!(
            "DEBUG BRANCH_BLOCK: Block {:?} terminator is {:?} in function '{}'",
            block_id,
            block.terminator,
            function.name
        );
        match &block.terminator {
            MirTerminator::Return { value } => {
                // A Return terminator in a branch block must leave an i32/i64/f64
                // on the stack matching the function's declared return type
                // before the bare Return instruction. There are three cases:
                //   1. value is Some(non-Undefined) → load it (with type coercion)
                //   2. value is Some(Undefined) → push a typed zero (implicit
                //      return from ensure_function_termination for a non-void
                //      function whose reachable tail had no explicit return)
                //   3. value is None → push a typed zero (defensive; should
                //      only happen for void functions)
                let func_return_type = self
                    .current_function
                    .as_ref()
                    .map(|f| f.return_type.clone());
                let mut pushed_value = false;
                if let Some(return_value) = value {
                    if !matches!(return_value, MirOperand::Constant(MirConstant::Undefined)) {
                        self.load_operand(return_value)?;
                        pushed_value = true;

                        // Coerce return value to the function's declared return type (E007 fix).
                        // value_to_type covers both parameters and locals; get_operand_mir_type
                        // only covers locals, so we use value_to_type for Value operands.
                        let value_type = match return_value {
                            MirOperand::Value(vid) => self.value_to_type.get(vid).cloned(),
                            _ => self.get_operand_mir_type(return_value),
                        };
                        match (&func_return_type, &value_type) {
                            (Some(MirType::F64), Some(MirType::I32))
                            | (Some(MirType::F64), Some(MirType::I8))
                            | (Some(MirType::F64), Some(MirType::I16))
                            | (Some(MirType::F64), Some(MirType::U8))
                            | (Some(MirType::F64), Some(MirType::U16))
                            | (Some(MirType::F64), Some(MirType::U32)) => {
                                self.current_instructions.push(Instruction::F64ConvertI32S);
                            }
                            (Some(MirType::I32), Some(MirType::F64)) => {
                                self.current_instructions.push(Instruction::I32TruncF64S);
                            }
                            _ => {}
                        }
                    }
                }
                if !pushed_value {
                    self.push_zero_for_return_type(&func_return_type);
                }
                self.current_instructions.push(Instruction::Return);
            }

            MirTerminator::Jump { target } => {
                // NOTE: Check if this jump is a break (targets loop exit block)
                // If so, we need to emit a br instruction with the correct depth
                let mut emitted_br = false;
                for loop_ctx in self.loop_context_stack.iter().rev() {
                    if *target == loop_ctx.exit_block_id {
                        // This is a break - emit br to exit the loop
                        // Calculate depth: current_depth - block_depth gives us how many levels to jump
                        let br_depth = self.current_block_depth - loop_ctx.block_depth;
                        debug_mir!(
                            "BREAK: block={:?} target={:?} current_depth={} loop_block_depth={} br_depth={}",
                            block_id, target, self.current_block_depth, loop_ctx.block_depth, br_depth
                        );
                        debug_mir!(
                            "DEBUG BREAK: Emitting br {} for break to loop exit block {:?} (current_depth={}, block_depth={})",
                            br_depth, target, self.current_block_depth, loop_ctx.block_depth
                        );
                        self.current_instructions.push(Instruction::Br(br_depth));
                        emitted_br = true;
                        break;
                    }
                }

                if !emitted_br {
                    // Check if the target is a loop header (nested loop).
                    // Loop headers must always be generated inline — they are never
                    // "continuation" blocks that an outer structure will emit later.
                    let target_is_loop_header = self.is_loop_header(function, *target);

                    if target_is_loop_header && !generated.contains(target) {
                        debug_mir!(
                            "DEBUG JUMP: Target {:?} is a loop header — generating inline via generate_structured_blocks",
                            target
                        );
                        self.generate_structured_blocks(function, *target, generated)?;
                    } else if !target_is_loop_header {
                        // Not a break and not a loop header — regular jump to a continuation
                        // block. Don't follow it here; the outer structure will generate it.
                        debug_mir!(
                            "DEBUG JUMP: Skipping jump to {:?} (not a loop exit, will be generated as continuation)",
                            target
                        );
                    }
                    // If target is already generated (e.g. already visited loop header), do nothing.
                }
            }

            MirTerminator::Branch {
                condition,
                true_block,
                false_block,
            } => {
                // Check if false_block is a continuation (no else clause in source)
                let has_else_clause =
                    !self.is_continuation_not_else(function, *true_block, *false_block);

                debug_mir!("DEBUG BRANCH_BLOCK: Processing nested Branch in function '{}', true_block={:?}, false_block={:?}, has_else_clause={}",
                    function.name, true_block, false_block, has_else_clause);

                // Pre-compute the THIS-if's merge block and push it as the outer-
                // continuation guard for any further-nested if/else inside the
                // branches we're about to emit. Without this, a doubly-nested
                // if/else can inline THIS if's merge inside its own body, marking
                // it `generated`; when we then try to inline it ourselves we skip
                // (already generated), and every statement after THIS if/else
                // gets emitted at the wrong nesting level — silently dropped from
                // the runtime path when the outer branch isn't taken (residual
                // CMP-HTML-BLOCK-INTERPOLATION-DROPS-VARIABLE-OPERANDS path that
                // shows up in plugin html_block_to_code's tag_start == -1 arm).
                let pending_continuation: Option<BasicBlockId> = if !has_else_clause {
                    Some(*false_block)
                } else {
                    let tc = self.find_eventual_continuation(function, *true_block);
                    let fc = self.find_eventual_continuation(function, *false_block);
                    match (tc, fc) {
                        (Some(t), Some(f)) if t == f => Some(t),
                        (Some(t), None) => Some(t),
                        (None, Some(f)) => Some(f),
                        _ => tc.or(fc),
                    }
                };
                let pushed_continuation = pending_continuation
                    .filter(|cont| !generated.contains(cont))
                    .inspect(|cont| self.outer_if_continuations.push(*cont));

                // Nested if-else: generate it fully (including its own continuation handling)
                self.load_operand(condition)?;
                self.current_instructions
                    .push(Instruction::If(BlockType::Empty));
                self.current_block_depth += 1;
                debug_mir!(
                    "IF block {:?}: depth now {} (function '{}')",
                    block_id,
                    self.current_block_depth,
                    function.name
                );

                self.generate_branch_block(function, *true_block, generated)?;
                debug_mir!(
                    "IF block {:?}: after true_block {:?}, depth={}",
                    block_id,
                    true_block,
                    self.current_block_depth
                );

                // Only generate else clause if false_block is NOT a loop exit continuation
                // NOTE: If false_block is the exit continuation of any loop in the function,
                // it will be generated by the loop structure itself. Don't generate it as an else clause.
                let is_loop_exit = self.is_loop_exit_continuation(function, *false_block);

                debug_mir!("DEBUG BRANCH_BLOCK: function='{}', block={:?}, false_block={:?}, is_loop_exit={}",
                    function.name, block_id, false_block, is_loop_exit);

                if has_else_clause && !is_loop_exit {
                    self.current_instructions.push(Instruction::Else);
                    // Else doesn't change depth
                    self.generate_branch_block(function, *false_block, generated)?;
                } else if is_loop_exit {
                    debug_mir!("DEBUG BRANCH_BLOCK: Skipping false_block {:?} - detected as loop exit continuation in function '{}'",
                        false_block, function.name);
                }

                self.current_instructions.push(Instruction::End);
                self.current_block_depth -= 1;

                if pushed_continuation.is_some() {
                    self.outer_if_continuations.pop();
                }
                debug_mir!(
                    "IF block {:?}: End emitted, depth now {} (function '{}')",
                    block_id,
                    self.current_block_depth,
                    function.name
                );

                // After generating nested if-else, check if there's a continuation to inline.
                // If both branches jump to the same continuation, generate it inline.
                // If one branch returns and the other jumps, generate the jumped-to continuation.
                let true_has_return = self.block_directly_returns(function, *true_block);
                let false_has_return = if has_else_clause {
                    self.block_directly_returns(function, *false_block)
                } else {
                    false // No else clause means false branch doesn't return
                };

                if true_has_return && false_has_return {
                    // Both nested branches return - add unreachable
                    debug_mir!(
                        "Both branches return in block {:?}, adding Unreachable (function '{}')",
                        block_id,
                        function.name
                    );
                    self.current_instructions.push(Instruction::Unreachable);
                } else {
                    // Find the merge block that the non-returning branch(es) flow into.
                    //
                    // Two cases, handled separately because they have very different
                    // continuation semantics:
                    //
                    // 1. has_else_clause = false (single-arm if). The MIR builder
                    //    allocates `false_block` AS this if's merge point — it is the
                    //    correct continuation by construction. We MUST NOT chase via
                    //    `find_eventual_continuation` here: that walks every empty
                    //    Jump in the chain (including past the if's own merge into
                    //    the surrounding scope's merge, and possibly past several
                    //    enclosing scopes' merges) and lands on a block that
                    //    logically belongs to an OUTER scope. Inlining that block
                    //    inside this if's body marks it `generated`, so when the
                    //    top-level `generate_structured_blocks` walks the function's
                    //    natural Jump chain to emit the trailing code, the block is
                    //    skipped — every statement after this if disappears from
                    //    the emitted WASM, the function falls off its end without a
                    //    return value, and execution traps on the missing return.
                    //    Regression introduced by `efc89cd1` (chase_jump_chain in
                    //    `find_eventual_continuation`); reported as
                    //    FRAME-DATA-2-1-4-DATA-BLOCK-TRAP (fp 908a9fc55653), where
                    //    `parse_field_line` has a 3-level nested `if/if/if` (no
                    //    else) followed by trailing code that the broken codegen
                    //    silently dropped.
                    //
                    // 2. has_else_clause = true. Both branches end at a real merge
                    //    point. A branch may end in `Jump { target }` (simple body)
                    //    OR in `Branch { .. }` (nested if/else); the latter needs
                    //    `find_eventual_continuation` to chase through the inner
                    //    if's synthesized merge blocks until the common outer merge
                    //    is found. The pre-`efc89cd1` implementation only handled
                    //    `Jump` and silently dropped every statement after a nested
                    //    if/else inside a branch (root cause of
                    //    COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS). Both
                    //    branches' continuations must agree, otherwise the outer
                    //    structure will emit it.
                    let mut continuation: Option<BasicBlockId> = None;

                    if !has_else_clause {
                        if !is_loop_exit {
                            continuation = Some(*false_block);
                        }
                    } else {
                        let true_cont = if !true_has_return {
                            function
                                .blocks
                                .get(true_block)
                                .and_then(|b| match &b.terminator {
                                    MirTerminator::Jump { target } => Some(*target),
                                    MirTerminator::Branch { .. } => {
                                        self.find_eventual_continuation(function, *true_block)
                                    }
                                    _ => None,
                                })
                        } else {
                            None
                        };

                        if let Some(tc) = true_cont {
                            let is_loop_exit_target = self
                                .loop_context_stack
                                .iter()
                                .any(|ctx| ctx.exit_block_id == tc);
                            if !is_loop_exit_target {
                                continuation = Some(tc);
                            }
                        }

                        if !false_has_return {
                            let false_cont =
                                function.blocks.get(false_block).and_then(|b| {
                                    match &b.terminator {
                                        MirTerminator::Jump { target } => Some(*target),
                                        MirTerminator::Branch { .. } => {
                                            self.find_eventual_continuation(function, *false_block)
                                        }
                                        _ => None,
                                    }
                                });

                            if let Some(fc) = false_cont {
                                if let Some(tc) = continuation {
                                    if tc != fc {
                                        // Branches merge at different points — can't inline a
                                        // single continuation here; outer structure will pick it up.
                                        continuation = None;
                                    }
                                } else {
                                    continuation = Some(fc);
                                }
                            }
                        }
                    }

                    // Inline continuation if found.
                    // Skip the inline if:
                    //   * `cont` is already in `generated` (outer scope will emit it), OR
                    //   * `cont` matches an outer if/else's pending merge block — emitting
                    //     it inside our branch would marks it `generated`, then the outer
                    //     scope sees the contains check, skips, and every statement after
                    //     the outer if/else is silently dropped from the WASM body.
                    if let Some(cont) = continuation {
                        let outer_owns = self.outer_if_continuations.contains(&cont);
                        if !generated.contains(&cont) && !outer_owns {
                            debug_mir!(
                                "Inlining continuation {:?} at depth {} (function '{}')",
                                cont,
                                self.current_block_depth,
                                function.name
                            );
                            self.generate_branch_block(function, cont, generated)?;
                        } else {
                            debug_mir!("DEBUG BRANCH_BLOCK: Skipping continuation block {:?} - {} (function '{}')",
                                cont,
                                if outer_owns { "owned by outer if" } else { "already marked for generation" },
                                function.name);
                        }
                    }
                }
            }

            MirTerminator::Unreachable => {
                // NOTE: Skip adding Unreachable - see comment in generate_structured_blocks
                // MirTerminator::Unreachable is a placeholder that should not generate WASM Unreachable
                // for void functions ending naturally.
            }

            MirTerminator::Trap => {
                // Contract violation trap - ALWAYS generates WASM unreachable instruction
                self.current_instructions.push(Instruction::Unreachable);
            }
        }

        Ok(())
    }

    /// Generate structured control flow for blocks.
    ///
    /// This is the primary recursive function that walks the MIR CFG and emits
    /// structured WASM (block/loop/if/else/end) instructions.
    pub(super) fn generate_structured_blocks(
        &mut self,
        function: &MirFunction,
        block_id: BasicBlockId,
        generated: &mut std::collections::HashSet<BasicBlockId>,
    ) -> Result<(), CompilerError> {
        // Skip if already generated
        if generated.contains(&block_id) {
            debug_mir!("DEBUG GENERATE_BLOCKS: Skipping already-generated block {:?} in function '{}', generated set contains {} blocks",
                block_id, function.name, generated.len());
            return Ok(());
        }
        debug_mir!(
            "DEBUG GENERATE_BLOCKS: Inserting block {:?} into generated set for function '{}'",
            block_id,
            function.name
        );
        generated.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            debug_mir!(
                "DEBUG GENERATE_BLOCKS: Block {:?} not found in function '{}'",
                block_id,
                function.name
            );
            return Ok(());
        };

        debug_mir!(
            "DEBUG GENERATE_BLOCKS: Generating block {:?} in function '{}', terminator={:?}",
            block_id,
            function.name,
            block.terminator
        );

        // CRITICAL: Check if this block is a loop header BEFORE generating instructions
        // For loop headers, instructions must be generated INSIDE the loop
        let is_loop_header = self.is_loop_header(function, block_id);

        // Generate block instructions (UNLESS this is a loop header - those go inside the loop)
        if !is_loop_header {
            for instruction in &block.instructions {
                self.generate_instruction(instruction)?;
            }
        }

        // Handle terminator with structured control flow.
        // See push_zero_for_return_type for the three-case invariant.
        match &block.terminator {
            MirTerminator::Return { value } => {
                let func_return_type = self
                    .current_function
                    .as_ref()
                    .map(|f| f.return_type.clone());
                let mut pushed_value = false;
                if let Some(return_value) = value {
                    if !matches!(return_value, MirOperand::Constant(MirConstant::Undefined)) {
                        self.load_operand(return_value)?;
                        pushed_value = true;

                        // Coerce the return value to match the function's declared return type
                        // (E007 fix). value_to_type covers both parameters and locals;
                        // get_operand_mir_type only covers locals, so we use value_to_type
                        // as the authoritative type lookup for Value operands.
                        let value_type = match return_value {
                            MirOperand::Value(vid) => self.value_to_type.get(vid).cloned(),
                            _ => self.get_operand_mir_type(return_value),
                        };
                        match (&func_return_type, &value_type) {
                            (Some(MirType::F64), Some(MirType::I32))
                            | (Some(MirType::F64), Some(MirType::I8))
                            | (Some(MirType::F64), Some(MirType::I16))
                            | (Some(MirType::F64), Some(MirType::U8))
                            | (Some(MirType::F64), Some(MirType::U16))
                            | (Some(MirType::F64), Some(MirType::U32)) => {
                                self.current_instructions.push(Instruction::F64ConvertI32S);
                            }
                            (Some(MirType::I32), Some(MirType::F64)) => {
                                self.current_instructions.push(Instruction::I32TruncF64S);
                            }
                            _ => {}
                        }
                    }
                }
                if !pushed_value {
                    self.push_zero_for_return_type(&func_return_type);
                }
                self.current_instructions.push(Instruction::Return);
            }

            MirTerminator::Jump { target } => {
                // Just continue to next block inline
                self.generate_structured_blocks(function, *target, generated)?;
            }

            MirTerminator::Branch {
                condition,
                true_block,
                false_block,
            } => {
                // NOTE: Check if this block is a loop header (has backedge)
                let is_loop = self.is_loop_header(function, block_id);

                debug_mir!("DEBUG BRANCH: Block {:?} in function '{}', is_loop={}, true_block={:?}, false_block={:?}",
                    block_id, function.name, is_loop, true_block, false_block);

                if is_loop {
                    debug_mir!("DEBUG LOOP: Block {:?} is a loop header in function '{}', generating loop structure with false_block={:?} as continuation",
                        block_id, function.name, false_block);

                    // Generate loop structure:
                    // block (outer - for exit via br_if 1)
                    //   loop (inner - for continue via br 0)
                    //     header block instructions (condition evaluation)
                    //     condition check
                    //     br_if 1 (!condition) - exit if condition is false
                    //     body (true_block)
                    //     br 0 - jump back to loop header (backedge handled by MirTerminator::Jump)
                    //   end
                    // end
                    // continuation (false_block)

                    self.current_instructions
                        .push(Instruction::Block(BlockType::Empty)); // label @1 (exit target)
                    self.current_block_depth += 1;

                    // NOTE: Push loop context so break inside nested if can emit correct br
                    self.loop_context_stack.push(LoopCodegenContext {
                        exit_block_id: *false_block,
                        block_depth: self.current_block_depth,
                    });

                    self.current_instructions
                        .push(Instruction::Loop(BlockType::Empty)); // label @0 (loop target)
                    self.current_block_depth += 1;

                    // CRITICAL: Generate header block instructions INSIDE the loop
                    // This ensures condition is re-evaluated on each iteration
                    for instruction in &block.instructions {
                        self.generate_instruction(instruction)?;
                    }

                    // Load condition and negate it (br_if when condition is FALSE)
                    self.load_operand(condition)?;
                    self.current_instructions.push(Instruction::I32Eqz); // Negate: br_if when 0 (false)
                    self.current_instructions.push(Instruction::BrIf(1)); // Exit to block @1 if condition is false

                    // Generate loop body (true_block) - this will have a Jump to increment or back to header
                    // Mark the loop body as generated to prevent infinite recursion
                    self.generate_branch_block(function, *true_block, generated)?;

                    // NOTE: Find and generate the increment block for for-loops
                    // The body may contain nested control flow (if statements), so we can't just
                    // check the body's direct terminator. Instead, find the increment block
                    // by looking for any block that jumps back to the header.
                    let increment_block_id =
                        self.find_loop_increment_block(function, *true_block, block_id);

                    if let Some(increment_id) = increment_block_id {
                        // NOTE: Check if increment block was already generated
                        // This happens when a nested if statement's continuation is the same
                        // as the increment block (e.g., while loop with if inside).
                        // The if's continuation handling may have already generated this block.
                        if !generated.contains(&increment_id) {
                            if let Some(increment_block) = function.blocks.get(&increment_id) {
                                debug_mir!(
                                    "DEBUG LOOP: Found increment block {:?} that jumps back to header {:?}",
                                    increment_id, block_id
                                );

                                // Generate increment block instructions INSIDE the loop
                                for instruction in &increment_block.instructions {
                                    self.generate_instruction(instruction)?;
                                }
                                generated.insert(increment_id);
                            }
                        } else {
                            debug_mir!(
                                "DEBUG LOOP: Increment block {:?} already generated (by nested if continuation), skipping",
                                increment_id
                            );
                        }
                        // Always add br 0 to jump back to loop header (regardless of whether we generated the block)
                        self.current_instructions.push(Instruction::Br(0));
                    } else {
                        // No separate increment block - check if body jumps directly back to header
                        if let Some(body_block) = function.blocks.get(true_block) {
                            if let MirTerminator::Jump { target } = &body_block.terminator {
                                if *target == block_id {
                                    // Simple while loop - body jumps directly back to header
                                    debug_mir!("DEBUG LOOP: Body block {:?} jumps directly back to header {:?}, adding br 0", true_block, block_id);
                                    self.current_instructions.push(Instruction::Br(0));
                                }
                            }
                        }
                    }

                    self.current_instructions.push(Instruction::End); // end loop
                    self.current_block_depth -= 1;
                    self.current_instructions.push(Instruction::End); // end block
                    self.current_block_depth -= 1;

                    // Pop loop context
                    self.loop_context_stack.pop();

                    // Generate loop continuation (false_block) - this is the code after the loop.
                    // CRITICAL: Use generate_branch_block (NOT generate_structured_blocks) to
                    // avoid consuming blocks that belong to outer control structures. If this
                    // loop is inside an if's true_block, generate_structured_blocks would follow
                    // Jump chains into the if's continuation, preventing it from being generated
                    // at the correct nesting level after the if's End instruction.
                    debug_mir!(
                        "DEBUG LOOP: Generating continuation block {:?} for loop in function '{}'",
                        false_block,
                        function.name
                    );
                    self.generate_branch_block(function, *false_block, generated)?;
                } else {
                    // Regular if/else (not a loop)
                    // Check if false_block is a continuation (no else clause in source)
                    let has_else_clause =
                        !self.is_continuation_not_else(function, *true_block, *false_block);

                    // Pre-compute the continuation so nested `generate_branch_block`
                    // calls in the branches know this is OUR merge point and must
                    // not inline it into themselves. Without this, a nested if/else
                    // inside our else clause would inline the outer merge inside
                    // its own body; we'd then see `generated.contains(&cont)` and
                    // skip emission, dropping every statement after this if/else
                    // (CMP-HTML-BLOCK-INTERPOLATION-DROPS-VARIABLE-OPERANDS final
                    // codegen path).
                    let pending_continuation: Option<BasicBlockId> = if !has_else_clause {
                        Some(*false_block)
                    } else {
                        let tc = self.find_eventual_continuation(function, *true_block);
                        let fc = self.find_eventual_continuation(function, *false_block);
                        match (tc, fc) {
                            (Some(t), Some(f)) if t == f => Some(t),
                            (Some(t), None) => Some(t),
                            (None, Some(f)) => Some(f),
                            _ => tc.or(fc),
                        }
                    };
                    let pushed_continuation = pending_continuation
                        .filter(|cont| !generated.contains(cont))
                        .inspect(|cont| self.outer_if_continuations.push(*cont));

                    // Generate if/else structure
                    self.load_operand(condition)?;
                    self.current_instructions
                        .push(Instruction::If(BlockType::Empty));
                    self.current_block_depth += 1;

                    // Use generate_branch_block to avoid following Jump terminators inside branches
                    self.generate_branch_block(function, *true_block, generated)?;

                    // Only generate else clause if false_block is NOT an empty continuation
                    if has_else_clause {
                        self.current_instructions.push(Instruction::Else);
                        // Else doesn't change depth - same block
                        self.generate_branch_block(function, *false_block, generated)?;
                    }

                    self.current_instructions.push(Instruction::End);
                    self.current_block_depth -= 1;

                    // Pop the outer continuation guard now that both branches are done.
                    if pushed_continuation.is_some() {
                        self.outer_if_continuations.pop();
                    }

                    // Check if both branches directly return (without following Jumps to continuations)
                    let true_has_return = self.block_directly_returns(function, *true_block);
                    let false_has_return = if has_else_clause {
                        self.block_directly_returns(function, *false_block)
                    } else {
                        false // No else clause means false branch doesn't return
                    };

                    debug_mir!("DEBUG RETURN CHECK: Function '{}', Block {:?}, true_has_return={}, false_has_return={}, has_else_clause={}",
                        function.name, block_id, true_has_return, false_has_return, has_else_clause);

                    if true_has_return && false_has_return {
                        // Both branches return - add unreachable to indicate code after if-else is never reached
                        self.current_instructions.push(Instruction::Unreachable);
                    } else {
                        // Find and generate continuation block.
                        // We need to find the continuation that at least one non-returning branch jumps to.
                        let mut continuation: Option<BasicBlockId> = None;

                        if !has_else_clause {
                            // No else clause: `false_block` IS the outer if's continuation by
                            // construction (the MIR builder allocates it as the merge point for
                            // the if's non-taken path). Use it directly.
                            //
                            // `find_eventual_continuation(true_block)` is NOT safe here when the
                            // true branch contains nested control flow: it returns the inner
                            // merge block, which `generate_branch_block` has already inlined
                            // while emitting the true branch. Re-entering it would short-circuit
                            // on `generated.contains(&...)` and the outer continuation would
                            // never be visited, dropping every statement after the outer if.
                            continuation = Some(*false_block);
                        } else {
                            // Real else clause: locate the merge block both branches jump to.
                            //
                            // We use `find_eventual_continuation` uniformly (not the
                            // direct Jump target) so a branch whose immediate Jump
                            // lands on a non-empty intermediate merge gets chased to
                            // the OUTER merge. Picking the immediate Jump target
                            // would lock continuation to the intermediate merge,
                            // and when the two branches' intermediate merges differ
                            // (each containing post-inner-if statements that then
                            // Jump to the same outer merge), the outer merge would
                            // never be emitted — silently dropping every statement
                            // that follows the outer if/else
                            // (CMP-HTML-BLOCK-INTERPOLATION-DROPS-VARIABLE-OPERANDS
                            // residual path, fp e96188e7e08a).

                            if !true_has_return {
                                continuation =
                                    self.find_eventual_continuation(function, *true_block);
                            }

                            if !false_has_return {
                                let false_cont =
                                    self.find_eventual_continuation(function, *false_block);
                                if let Some(fc) = false_cont {
                                    if continuation.is_none() {
                                        continuation = Some(fc);
                                    }
                                    // Both branches should lead to same continuation
                                }
                            }
                        }

                        // Generate the continuation block if we found one
                        if let Some(cont) = continuation {
                            self.generate_structured_blocks(function, cont, generated)?;
                        }
                    }
                }
            }

            MirTerminator::Unreachable => {
                // NOTE: Only add Unreachable for truly unreachable code (inside branches with both returning)
                // For void functions that reach the end naturally, we should NOT add Unreachable.
                // The function will end with the End instruction added later.
                //
                // We only add Unreachable here if this block is NOT reachable from normal control flow.
                // Since MirTerminator::Unreachable is used as a placeholder during MIR construction,
                // reaching it at the end of function generation means the function ends naturally.
                // For void functions, this is valid - no Unreachable needed.
                //
                // Skip adding Unreachable - let function end naturally with End instruction
            }

            MirTerminator::Trap => {
                // Contract violation trap - ALWAYS generates WASM unreachable instruction
                self.current_instructions.push(Instruction::Unreachable);
            }
        }

        Ok(())
    }
}
