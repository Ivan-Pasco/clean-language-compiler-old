//! Control flow analysis helpers for MIR code generation.
//!
//! This module contains functions that analyse the control-flow graph of a MIR
//! function to answer questions needed during structured WASM emission:
//!
//! * Does a block always terminate with an explicit return?
//! * Is a "false_block" really a continuation (no else) or a genuine else clause?
//! * Which block is the loop header / loop exit?
//! * What is the increment block in a for-loop?

use super::*;

impl MirCodeGenerator<'_> {
    /// Check if a block directly returns (without following Jump terminators).
    ///
    /// Returns `true` if every execution path from `block_id` ends in a
    /// `MirTerminator::Return` or `MirTerminator::Trap` — without ever following
    /// a `Jump` to a continuation block.  Used to detect whether an if-else
    /// branch exits via `return` vs. via a jump to a shared continuation.
    pub(super) fn block_directly_returns(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
    ) -> bool {
        let mut visited = std::collections::HashSet::new();
        self.block_directly_returns_recursive(function, block_id, &mut visited)
    }

    fn block_directly_returns_recursive(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
    ) -> bool {
        // Prevent infinite loops
        if visited.contains(&block_id) {
            return false;
        }
        visited.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            return false;
        };

        match &block.terminator {
            // NOTE: Only explicit Return counts as "directly returns"
            // MirTerminator::Unreachable is a placeholder for void functions ending naturally
            // and should NOT be considered a direct return - it does NOT guarantee the function
            // has returned a value or terminated execution
            MirTerminator::Return { .. } => true,
            MirTerminator::Unreachable => false,
            MirTerminator::Branch {
                true_block,
                false_block,
                ..
            } => {
                // Both branches must directly return
                self.block_directly_returns_recursive(function, *true_block, visited)
                    && self.block_directly_returns_recursive(function, *false_block, visited)
            }
            MirTerminator::Jump { .. } => {
                // Jump means this branch exits to a continuation - NOT a direct return
                false
            }
            MirTerminator::Trap => {
                // Trap terminates execution - counts as "directly returns"
                true
            }
        }
    }

    /// Helper function to check if false_block is a continuation (not a real else clause).
    ///
    /// A false_block is a continuation if:
    /// 1. It's empty (no instructions) with Unreachable/Jump/Return terminator, OR
    /// 2. The true_block jumps directly to it (indicating no else clause in source), OR
    /// 3. The true_block has nested control flow and one of its exit paths jumps to false_block
    ///
    /// Used to detect when an if statement has NO else clause.
    pub(super) fn is_continuation_not_else(
        &self,
        function: &MirFunction,
        true_block: BasicBlockId,
        false_block: BasicBlockId,
    ) -> bool {
        let Some(false_blk) = function.blocks.get(&false_block) else {
            return false;
        };

        // Check #1: Empty block with simple terminator
        if false_blk.instructions.is_empty() {
            match &false_blk.terminator {
                MirTerminator::Unreachable | MirTerminator::Jump { .. } => return true,
                MirTerminator::Return { value } => {
                    // Empty continuation if returning nothing or undefined
                    if matches!(
                        value,
                        None | Some(MirOperand::Constant(MirConstant::Undefined))
                    ) {
                        return true;
                    }
                }
                _ => {}
            }
        }

        // Check #2: True branch jumps to false_block
        if let Some(true_blk) = function.blocks.get(&true_block) {
            if let MirTerminator::Jump { target } = &true_blk.terminator {
                if *target == false_block {
                    return true;
                }

                // Check #2b: True branch is a break (jumps to a loop exit block).
                // In `if condition; break` patterns, the false_block is the continuation
                // (code after the if-break), not an else clause.
                for loop_ctx in &self.loop_context_stack {
                    if *target == loop_ctx.exit_block_id {
                        return true;
                    }
                }
            }

            // Check #3: True branch eventually reaches false_block through any path.
            if self.block_eventually_jumps_to(function, true_block, false_block) {
                return true;
            }
        }

        // Check #4: false_block has multiple predecessors — it's a merge point.
        // Count how many blocks in the function jump or branch to false_block.
        // If more than one block targets it, it's a continuation (merge), not an else.
        {
            let mut predecessor_count = 0;
            for (_, block) in &function.blocks {
                match &block.terminator {
                    MirTerminator::Jump { target } => {
                        if *target == false_block {
                            predecessor_count += 1;
                        }
                    }
                    MirTerminator::Branch {
                        true_block: tb,
                        false_block: fb,
                        ..
                    } => {
                        if *tb == false_block || *fb == false_block {
                            predecessor_count += 1;
                        }
                    }
                    _ => {}
                }
            }
            // The Branch that we're currently checking contributes 1 predecessor.
            // If there's more than 1 total, false_block is a merge point.
            if predecessor_count > 1 {
                return true;
            }
        }

        false
    }

    /// Helper to check if a block eventually jumps to a target block (following Jump terminators).
    pub(super) fn block_eventually_jumps_to(
        &self,
        function: &MirFunction,
        start_block: BasicBlockId,
        target_block: BasicBlockId,
    ) -> bool {
        let mut visited = std::collections::HashSet::new();
        let mut to_visit = vec![start_block];

        while let Some(block_id) = to_visit.pop() {
            if visited.contains(&block_id) {
                continue;
            }
            visited.insert(block_id);

            let Some(block) = function.blocks.get(&block_id) else {
                continue;
            };

            match &block.terminator {
                MirTerminator::Jump { target } => {
                    if *target == target_block {
                        return true;
                    }
                    // Follow the jump
                    to_visit.push(*target);
                }
                MirTerminator::Branch {
                    true_block,
                    false_block,
                    ..
                } => {
                    // Check both branches
                    to_visit.push(*true_block);
                    to_visit.push(*false_block);
                }
                MirTerminator::Return { .. } | MirTerminator::Unreachable | MirTerminator::Trap => {
                    // Dead end (Return, Unreachable, or Trap)
                }
            }
        }

        false
    }

    /// Find the eventual continuation block from a block that may have nested control flow.
    ///
    /// Follows through nested Branches to find where non-returning paths eventually jump.
    /// Returns `None` if all paths return or if there's no common continuation.
    pub(super) fn find_eventual_continuation(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
    ) -> Option<BasicBlockId> {
        let mut visited = std::collections::HashSet::new();
        let mut continuations = std::collections::HashSet::new();
        self.collect_jump_targets(function, block_id, &mut visited, &mut continuations);

        // If all non-returning paths lead to the same block, that's our continuation
        if continuations.len() == 1 {
            continuations.into_iter().next()
        } else {
            None
        }
    }

    /// Recursively collect all Jump targets from a block's control flow.
    ///
    /// Follows through nested Branches to find where non-returning paths jump.
    pub(super) fn collect_jump_targets(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
        targets: &mut std::collections::HashSet<BasicBlockId>,
    ) {
        if visited.contains(&block_id) {
            return;
        }
        visited.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            return;
        };

        match &block.terminator {
            MirTerminator::Jump { target } => {
                targets.insert(*target);
            }
            MirTerminator::Branch {
                true_block,
                false_block,
                ..
            } => {
                // Follow both branches
                self.collect_jump_targets(function, *true_block, visited, targets);
                self.collect_jump_targets(function, *false_block, visited, targets);
            }
            MirTerminator::Return { .. } | MirTerminator::Unreachable | MirTerminator::Trap => {
                // Dead end - no continuation from here
            }
        }
    }

    /// Detects if a block is a loop header by checking for backedges using DFS.
    ///
    /// A block is a loop header if there is a cycle in the CFG that includes this
    /// block as the target of a backedge.  More robust than block-ID ordering.
    pub(super) fn is_loop_header(&self, function: &MirFunction, block_id: BasicBlockId) -> bool {
        // Build successors map for DFS traversal
        let mut successors: std::collections::HashMap<BasicBlockId, Vec<BasicBlockId>> =
            std::collections::HashMap::new();

        for (id, block) in &function.blocks {
            let succs = match &block.terminator {
                MirTerminator::Jump { target } => vec![*target],
                MirTerminator::Branch {
                    true_block,
                    false_block,
                    ..
                } => vec![*true_block, *false_block],
                MirTerminator::Return { .. } | MirTerminator::Unreachable | MirTerminator::Trap => {
                    vec![]
                }
            };
            successors.insert(*id, succs);
        }

        // Find all back edges using DFS from entry block
        let mut visited = std::collections::HashSet::new();
        let mut on_stack = std::collections::HashSet::new();
        let mut back_edges = Vec::new();

        // Start DFS from entry block (block 0)
        let entry_block = BasicBlockId(0);
        if function.blocks.contains_key(&entry_block) {
            self.find_back_edges_dfs_internal(
                entry_block,
                &successors,
                &mut visited,
                &mut on_stack,
                &mut back_edges,
            );
        }

        // Check if this block is the target (header) of any back edge
        back_edges.iter().any(|(_tail, head)| *head == block_id)
    }

    /// Internal DFS helper for finding back edges.
    pub(super) fn find_back_edges_dfs_internal(
        &self,
        block: BasicBlockId,
        successors: &std::collections::HashMap<BasicBlockId, Vec<BasicBlockId>>,
        visited: &mut std::collections::HashSet<BasicBlockId>,
        on_stack: &mut std::collections::HashSet<BasicBlockId>,
        back_edges: &mut Vec<(BasicBlockId, BasicBlockId)>,
    ) {
        visited.insert(block);
        on_stack.insert(block);

        if let Some(succs) = successors.get(&block) {
            for &successor in succs {
                if on_stack.contains(&successor) {
                    // Back edge found: current block -> successor (which is already on stack)
                    back_edges.push((block, successor));
                } else if !visited.contains(&successor) {
                    self.find_back_edges_dfs_internal(
                        successor, successors, visited, on_stack, back_edges,
                    );
                }
            }
        }

        on_stack.remove(&block);
    }

    /// Check if a block is the exit continuation of any loop.
    ///
    /// Returns `true` when `block_id` is the `false_block` of a loop header's `Branch`
    /// terminator — meaning it is the block that is jumped to when the loop exits.
    pub(super) fn is_loop_exit_continuation(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
    ) -> bool {
        function.blocks.iter().any(|(loop_id, loop_block)| {
            // Check if this block is a loop header
            let is_header = self.is_loop_header(function, *loop_id);
            if !is_header {
                return false;
            }

            // Check if the loop header's Branch has our block as the false_block (exit)
            matches!(&loop_block.terminator,
                MirTerminator::Branch { false_block, .. } if *false_block == block_id)
        })
    }

    #[allow(dead_code)] // CFG analysis helper — available for future optimizer passes
    pub(super) fn block_always_returns(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
    ) -> bool {
        let mut visited = std::collections::HashSet::new();
        let mut on_stack = std::collections::HashSet::new();
        self.block_always_returns_recursive(function, block_id, &mut visited, &mut on_stack)
    }

    #[allow(dead_code)] // Recursive impl for block_always_returns — not yet called externally
    fn block_always_returns_recursive(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
        on_stack: &mut std::collections::HashSet<BasicBlockId>,
    ) -> bool {
        // If we encounter a block on the recursion stack, it's a back-edge (loop)
        // Treat loop back-edges as "okay" - the exit path must return
        if on_stack.contains(&block_id) {
            return true;
        }

        // If already fully analyzed, return cached result
        if visited.contains(&block_id) {
            return false;
        }

        // Mark this block as being on the recursion stack
        on_stack.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            on_stack.remove(&block_id);
            return false;
        };

        let result = match &block.terminator {
            MirTerminator::Return { .. } | MirTerminator::Unreachable | MirTerminator::Trap => true,
            MirTerminator::Branch {
                true_block,
                false_block,
                ..
            } => {
                // Both branches must return (back-edges return true automatically)
                self.block_always_returns_recursive(function, *true_block, visited, on_stack)
                    && self.block_always_returns_recursive(
                        function,
                        *false_block,
                        visited,
                        on_stack,
                    )
            }
            MirTerminator::Jump { target } => {
                // Follow the jump
                self.block_always_returns_recursive(function, *target, visited, on_stack)
            }
        };

        // Remove from recursion stack
        on_stack.remove(&block_id);

        // If this block doesn't return, cache it
        if !result {
            visited.insert(block_id);
        }

        result
    }

    /// Find the block that eventually leads to the loop's increment/backedge.
    ///
    /// Handles cases where the body contains nested control flow (if statements)
    /// and needs to follow through to find what eventually jumps to increment.
    pub(super) fn find_loop_increment_block(
        &self,
        function: &MirFunction,
        body_block_id: BasicBlockId,
        header_block_id: BasicBlockId,
    ) -> Option<BasicBlockId> {
        // Look for a block that:
        // 1. Jumps back to the header (backedge)
        // 2. Has a block ID GREATER than the body_block_id (comes after in CFG order)
        // 3. Is labeled as an increment block (for explicit for-loop increments)
        //
        // The second condition is crucial: the init block (block 0) also jumps to header,
        // but it's not the increment block - it's the entry point.
        for (block_id, block) in &function.blocks {
            if let MirTerminator::Jump { target } = &block.terminator {
                if *target == header_block_id
                    && block_id.0 > body_block_id.0
                    && block_id.0 > header_block_id.0
                {
                    debug_mir!(
                        "DEBUG find_loop_increment_block: Found increment block {:?} (label={:?}) that jumps to header {:?}",
                        block_id, block.label, header_block_id
                    );
                    return Some(*block_id);
                }
            }
        }
        None
    }

    /// Check if a path from start_block eventually reaches target_block.
    #[allow(dead_code)] // CFG reachability helper — available for future optimizer passes
    pub(super) fn path_reaches_block(
        &self,
        function: &MirFunction,
        start_block: BasicBlockId,
        target_block: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
    ) -> bool {
        if start_block == target_block {
            return true;
        }
        if visited.contains(&start_block) {
            return false;
        }
        visited.insert(start_block);

        if let Some(block) = function.blocks.get(&start_block) {
            match &block.terminator {
                MirTerminator::Jump { target } => {
                    self.path_reaches_block(function, *target, target_block, visited)
                }
                MirTerminator::Branch {
                    true_block,
                    false_block,
                    ..
                } => {
                    self.path_reaches_block(function, *true_block, target_block, visited)
                        || self.path_reaches_block(function, *false_block, target_block, visited)
                }
                MirTerminator::Return { .. } | MirTerminator::Unreachable | MirTerminator::Trap => {
                    false
                }
            }
        } else {
            false
        }
    }
}
