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

        // The MIR builder labels the block of a source `else` clause "else"
        // and the merge block of an `if` "continue" (mir_builder/statements.rs,
        // If), so the label decides when present. An "else" block is an else
        // clause whatever its shape: with a then-arm ending in `break`, the
        // checks below took `if c: break else: x` for `if c: break` followed
        // by `x`, emitted `x` as the if's continuation and dropped the
        // statements after the if/else.
        match false_blk.label.as_deref() {
            Some("else") => return false,
            Some("continue") => return true,
            _ => {}
        }

        // Check #1: Empty block with simple terminator
        if false_blk.instructions.is_empty() {
            match &false_blk.terminator {
                MirTerminator::Unreachable => return true,
                MirTerminator::Jump { target } => {
                    // EXCEPT: an empty block that Jumps to the innermost loop's
                    // exit is an `else: break` body, NOT the if's continuation.
                    // Treating it as a no-else case caused the else branch to be
                    // silently dropped from the emitted WASM, which kept the
                    // inner loop running forever (root cause of the inner
                    // `find_unescaped_quote` hang that surfaced on top of
                    // COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS after the
                    // sibling fix to `collect_jump_targets`).
                    let is_break_to_loop_exit = self
                        .loop_context_stack
                        .iter()
                        .any(|ctx| ctx.exit_block_id == *target);
                    if !is_break_to_loop_exit {
                        return true;
                    }
                }
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
            for block in function.blocks.values() {
                match &block.terminator {
                    MirTerminator::Jump { target } if *target == false_block => {
                        predecessor_count += 1;
                    }
                    MirTerminator::Branch {
                        true_block: tb,
                        false_block: fb,
                        ..
                    } if *tb == false_block || *fb == false_block => {
                        predecessor_count += 1;
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

    /// Whether a Jump to `block_id` leaves the current iteration of a loop
    /// being emitted: `continue` (to the loop header, or to the increment
    /// block of an `iterate` loop) or `break` (to the loop exit). Such a
    /// Jump is emitted as a `br` and never names a merge block.
    pub(super) fn ends_loop_iteration(&self, block_id: BasicBlockId) -> bool {
        self.loop_context_stack.iter().any(|l| {
            l.header_block_id == block_id
                || l.exit_block_id == block_id
                || l.continue_target.is_some_and(|(c, _)| c == block_id)
        })
    }

    /// Helper to check if a block eventually jumps to a target block (following Jump terminators).
    ///
    /// The walk stays inside the current iteration of the enclosing loops: a
    /// Jump to a loop header, to a loop's increment block (`continue`) or to
    /// a loop exit (`break`) leaves the region. Following the back edge
    /// reached every block of the loop body again, so an if/else whose
    /// then-arm ends in `continue` looked like it flowed into its own else
    /// arm; the else arm was then emitted as the if's continuation and the
    /// statements after the if/else were dropped.
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
                    // Follow the jump, unless it ends this loop iteration
                    if !self.ends_loop_iteration(*target) {
                        to_visit.push(*target);
                    }
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

        // A candidate the walk also stepped through is interior to this
        // region, not an exit from it: with two consecutive single-arm ifs,
        // the first if's merge (which holds the second if's test) is both a
        // Jump target of the first if's body and the block the walk follows
        // into via the first if's false edge. Keeping it left two candidates
        // (it and the real exit), so no continuation was found.
        continuations.retain(|c| !visited.contains(c));

        // Reduce intermediate merges out of the candidate set.
        //
        // When a branch's then- or else-arm contains its OWN inner if/else followed
        // by more statements, the inner if's merge block is non-empty (it carries the
        // post-inner statements) AND ends in a Jump to the outer merge. Both blocks
        // end up in `continuations`, making the set size > 1 and causing the function
        // to return None — the caller then never emits the outer merge, and every
        // statement after the outer if disappears from the WASM body
        // (CMP-HTML-BLOCK-INTERPOLATION-DROPS-VARIABLE-OPERANDS, fp e96188e7e08a).
        //
        // Fix: chase each candidate's Jump chain. If candidate `t` can reach
        // another candidate `u` (u != t), then `t` is an intermediate merge that
        // must be dropped. Iterate to fixed point so chains of arbitrary depth
        // collapse cleanly. Bounded by the function's block count, so terminates.
        //
        // Snapshot ordering: HashSet iteration order is randomised per process,
        // and the removals below depend on which candidate is inspected first.
        // Without a deterministic order, two runs against identical source can
        // produce different final continuation sets — an intermediate merge
        // survives on one run but not the other, and the caller emits either
        // a full merge body or a `br` short-circuit accordingly. This is the
        // root cause of CODEGEN-NONDET-001 (fp 44ab9e9a1486): frame.data's
        // parse_join_clause emits a full ~50-instruction concat chain on some
        // runs and just `br 0` on others, ~50/50 depending on the process's
        // HashSet seed. Sort by BasicBlockId (a wrapper around u32) so the
        // reduction is reproducible across processes.
        loop {
            let mut changed = false;
            let mut snapshot: Vec<_> = continuations.iter().copied().collect();
            snapshot.sort();
            for t in snapshot {
                // Walk the Jump chain from t until we hit a non-Jump terminator,
                // a block already visited, or a block that is itself a candidate.
                let mut cur = match function.blocks.get(&t).map(|b| &b.terminator) {
                    Some(MirTerminator::Jump { target }) => *target,
                    _ => continue,
                };
                let mut seen = std::collections::HashSet::new();
                seen.insert(t);
                loop {
                    if !seen.insert(cur) {
                        break;
                    }
                    if continuations.contains(&cur) && cur != t {
                        // t reaches a downstream candidate — drop t.
                        continuations.remove(&t);
                        changed = true;
                        break;
                    }
                    match function.blocks.get(&cur).map(|b| &b.terminator) {
                        Some(MirTerminator::Jump { target }) => cur = *target,
                        _ => break,
                    }
                }
            }
            if !changed {
                break;
            }
        }

        // If all non-returning paths lead to the same block, that's our continuation
        if continuations.len() == 1 {
            continuations.into_iter().next()
        } else {
            None
        }
    }

    /// Pick the merge block of an if/else from the eventual continuations of
    /// its two arms.
    ///
    /// When an arm ends with its own inner if/else followed by more
    /// statements, that arm's eventual continuation is the inner merge (it
    /// carries the trailing statements) — a block that belongs to the arm, not
    /// the merge of this if/else. The inner merge Jump-chains into the other
    /// arm's continuation, so whichever candidate reaches the other is the
    /// intermediate one and the reached block is the real merge. Treating the
    /// intermediate block as this if's merge made the inner if refuse to
    /// inline it (it looked owned by the outer if), so its statements were
    /// emitted after the outer if/else and also ran on the other arm.
    pub(super) fn resolve_if_merge(
        &self,
        function: &MirFunction,
        true_cont: Option<BasicBlockId>,
        false_cont: Option<BasicBlockId>,
    ) -> Option<BasicBlockId> {
        match (true_cont, false_cont) {
            (Some(t), Some(f)) if t == f => Some(t),
            (Some(t), Some(f)) if self.jump_chain_reaches(function, t, f) => Some(f),
            (Some(t), Some(f)) if self.jump_chain_reaches(function, f, t) => Some(t),
            (t, f) => t.or(f),
        }
    }

    /// Whether following `from`'s Jump terminators (through blocks with or
    /// without instructions) arrives at `to`.
    pub(super) fn jump_chain_reaches(
        &self,
        function: &MirFunction,
        from: BasicBlockId,
        to: BasicBlockId,
    ) -> bool {
        let mut cur = from;
        let mut seen = std::collections::HashSet::new();
        while seen.insert(cur) {
            match function.blocks.get(&cur).map(|b| &b.terminator) {
                Some(MirTerminator::Jump { target }) => {
                    if *target == to {
                        return true;
                    }
                    cur = *target;
                }
                _ => return false,
            }
        }
        false
    }

    /// Walk a Jump chain through empty merge blocks until landing on a
    /// block that either carries user instructions or has a non-Jump
    /// terminator. Returns the first such block.
    ///
    /// This is what lets `find_eventual_continuation` see through the
    /// stack of synthesized merge blocks the MIR builder produces around
    /// nested if/else inside another if/else — without it, the search
    /// stops at the innermost merge and the outer caller's inlining
    /// short-circuits on `generated.contains`, dropping every statement
    /// that follows the nested control flow (root cause of
    /// COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS — see
    /// `process_text_node`, where the `remaining = ...` reassignment after
    /// the inner `if/else` was silently dropped from the emitted WASM).
    fn chase_jump_chain(&self, function: &MirFunction, start: BasicBlockId) -> BasicBlockId {
        let mut cur = start;
        let mut seen = std::collections::HashSet::new();
        loop {
            if !seen.insert(cur) {
                return cur;
            }
            let Some(block) = function.blocks.get(&cur) else {
                return cur;
            };
            if !block.instructions.is_empty() {
                return cur;
            }
            match &block.terminator {
                MirTerminator::Jump { target } => {
                    cur = *target;
                }
                _ => return cur,
            }
        }
    }

    /// Recursively collect all Jump targets from a block's control flow.
    ///
    /// Follows through nested Branches to find where non-returning paths
    /// jump, then through chains of empty merge blocks via
    /// [`chase_jump_chain`] so a stack of nested if/else collapses to a
    /// single eventual continuation rather than the innermost merge.
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
                // `break` and `continue` of a loop being emitted leave the
                // region like `return` does and name no merge block. Counting
                // a `break` made an if/else whose other arm reaches the real
                // merge report two candidates (no continuation); following a
                // `continue` to the header like a jump into a nested loop
                // below picked the loop's exit as the merge.
                if self.ends_loop_iteration(*target) {
                    return;
                }
                // A jump into a loop is not a continuation: the loop is part
                // of this path, which continues where the loop exits (the
                // header's false branch, as generate_structured_blocks lays
                // loops out). Stopping at the header made an if/else whose
                // then-branch holds a loop report the header as its merge,
                // and the real merge — every statement after the if/else —
                // was never emitted.
                if self.is_loop_header(function, *target) {
                    if let Some(MirTerminator::Branch { false_block, .. }) =
                        function.blocks.get(target).map(|b| &b.terminator)
                    {
                        visited.insert(*target);
                        self.collect_jump_targets(function, *false_block, visited, targets);
                        return;
                    }
                }
                let dest = self.chase_jump_chain(function, *target);
                // A jump to the increment block of a loop being emitted is a
                // `continue`: it leaves the region like `return` does (the
                // loop codegen branches out of the body), so it is no merge
                // candidate. Counting it let an if/else whose arm ends in a
                // nested `continue` pick the increment block as its merge, and
                // the statements after the if/else were never emitted.
                let is_continue = self
                    .loop_context_stack
                    .iter()
                    .any(|l| l.continue_target.is_some_and(|(id, _)| id == dest));
                if !is_continue {
                    targets.insert(dest);
                }
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
        //
        // Determinism: `function.blocks` is a HashMap with non-deterministic iteration
        // order. When multiple blocks satisfy the predicate (which happens when the
        // body contains `continue` inside nested control flow, since `continue` creates
        // an additional block that also jumps to the header), returning whichever the
        // iterator sees first produces different WASM across runs. Sort candidates and
        // return the smallest qualifying id — semantically the closest natural increment
        // block to the header/body.
        let mut candidates: Vec<BasicBlockId> = function
            .blocks
            .iter()
            .filter_map(|(block_id, block)| {
                if let MirTerminator::Jump { target } = &block.terminator {
                    if *target == header_block_id
                        && block_id.0 > body_block_id.0
                        && block_id.0 > header_block_id.0
                    {
                        return Some(*block_id);
                    }
                }
                None
            })
            .collect();
        candidates.sort_by_key(|id| id.0);
        if let Some(picked) = candidates.first().copied() {
            debug_mir!(
                "DEBUG find_loop_increment_block: Picked increment block {:?} from {} candidate(s) for header {:?}",
                picked,
                candidates.len(),
                header_block_id
            );
            return Some(picked);
        }
        None
    }
}
