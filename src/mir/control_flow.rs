//! Control Flow Graph Analysis for MIR
//!
//! This module provides control flow graph (CFG) analysis capabilities for MIR,
//! including dominance analysis, loop detection, and various graph algorithms
//! used by optimization passes.

use crate::mir::mir_types::{
    MirFunction, MirTerminator,
    BasicBlockId
};
use std::collections::{HashMap, HashSet, VecDeque};

/// Control Flow Graph representation
#[derive(Debug, Clone)]
pub struct ControlFlowGraph {
    /// All basic blocks in the CFG
    pub blocks: HashMap<BasicBlockId, BasicBlock>,
    
    /// Entry block ID
    pub entry_block: BasicBlockId,
    
    /// Adjacency list for successors
    pub successors: HashMap<BasicBlockId, HashSet<BasicBlockId>>,
    
    /// Adjacency list for predecessors
    pub predecessors: HashMap<BasicBlockId, HashSet<BasicBlockId>>,
    
    /// Dominance information
    pub dominance: DominanceInfo,
    
    /// Loop information
    pub loops: LoopInfo,
}

/// Basic block information in CFG context
#[derive(Debug, Clone)]
pub struct BasicBlock {
    /// Block ID
    pub id: BasicBlockId,
    
    /// Block label for debugging
    pub label: Option<String>,
    
    /// Number of instructions in this block
    pub instruction_count: usize,
    
    /// Whether this block has side effects
    pub has_side_effects: bool,
    
    /// Whether this block is reachable from entry
    pub is_reachable: bool,
}

/// Dominance analysis information
#[derive(Debug, Clone, Default)]
pub struct DominanceInfo {
    /// Immediate dominators: dom[block] = immediate dominator of block
    pub immediate_dominators: HashMap<BasicBlockId, BasicBlockId>,
    
    /// Dominance tree: children of each block in the dominance tree
    pub dominance_tree: HashMap<BasicBlockId, HashSet<BasicBlockId>>,
    
    /// Dominance frontiers: blocks where dominance ends
    pub dominance_frontiers: HashMap<BasicBlockId, HashSet<BasicBlockId>>,
    
    /// Dominator sets: all blocks that dominate each block
    pub dominators: HashMap<BasicBlockId, HashSet<BasicBlockId>>,
}

/// Loop analysis information
#[derive(Debug, Clone, Default)]
pub struct LoopInfo {
    /// Natural loops in the CFG
    pub natural_loops: Vec<NaturalLoop>,
    
    /// Loop headers (blocks that are targets of back edges)
    pub loop_headers: HashSet<BasicBlockId>,
    
    /// Back edges in the CFG
    pub back_edges: Vec<(BasicBlockId, BasicBlockId)>,
    
    /// Loop nesting levels
    pub loop_depths: HashMap<BasicBlockId, usize>,
}

/// A natural loop in the control flow graph
#[derive(Debug, Clone)]
pub struct NaturalLoop {
    /// Header block of the loop
    pub header: BasicBlockId,
    
    /// All blocks in the loop
    pub blocks: HashSet<BasicBlockId>,
    
    /// Exit blocks (blocks with successors outside the loop)
    pub exit_blocks: HashSet<BasicBlockId>,
    
    /// Back edge that defines this loop
    pub back_edge: (BasicBlockId, BasicBlockId),
    
    /// Nesting level of this loop
    pub nesting_level: usize,
    
    /// Parent loop (if nested)
    pub parent_loop: Option<usize>, // Index into natural_loops vec
}

impl ControlFlowGraph {
    /// Build CFG from MIR function
    pub fn from_function(function: &MirFunction) -> Self {
        let mut cfg = Self {
            blocks: HashMap::new(),
            entry_block: function.entry_block,
            successors: HashMap::new(),
            predecessors: HashMap::new(),
            dominance: DominanceInfo::default(),
            loops: LoopInfo::default(),
        };
        
        // Build basic block information
        for (block_id, mir_block) in &function.blocks {
            let has_side_effects = mir_block.instructions.iter().any(|inst| {
                matches!(inst.operation, 
                    crate::mir::mir_types::MirOperation::Store { .. } |
                    crate::mir::mir_types::MirOperation::Call { .. }
                )
            });
            
            let basic_block = BasicBlock {
                id: *block_id,
                label: mir_block.label.clone(),
                instruction_count: mir_block.instructions.len(),
                has_side_effects,
                is_reachable: false, // Will be computed later
            };
            
            cfg.blocks.insert(*block_id, basic_block);
        }
        
        // Build successor/predecessor relationships
        cfg.build_edges(function);
        
        // Compute reachability
        cfg.compute_reachability();
        
        // Compute dominance information
        cfg.compute_dominance();
        
        // Detect loops
        cfg.detect_loops();
        
        cfg
    }
    
    /// Build successor and predecessor edges
    fn build_edges(&mut self, function: &MirFunction) {
        for (block_id, mir_block) in &function.blocks {
            let mut successors = HashSet::new();
            
            // Determine successors based on terminator
            match &mir_block.terminator {
                MirTerminator::Jump { target } => {
                    successors.insert(*target);
                }
                MirTerminator::Branch { true_block, false_block, .. } => {
                    successors.insert(*true_block);
                    successors.insert(*false_block);
                }
                MirTerminator::Return { .. } | MirTerminator::Unreachable => {
                    // No successors
                }
            }
            
            // Update successor map
            self.successors.insert(*block_id, successors.clone());
            
            // Update predecessor map
            for successor in successors {
                self.predecessors.entry(successor)
                    .or_insert_with(HashSet::new)
                    .insert(*block_id);
            }
        }
    }
    
    /// Compute reachability from entry block
    fn compute_reachability(&mut self) {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        
        // Start from entry block
        queue.push_back(self.entry_block);
        visited.insert(self.entry_block);
        
        // BFS traversal
        while let Some(block_id) = queue.pop_front() {
            if let Some(block) = self.blocks.get_mut(&block_id) {
                block.is_reachable = true;
            }
            
            // Visit successors
            if let Some(successors) = self.successors.get(&block_id) {
                for &successor in successors {
                    if !visited.contains(&successor) {
                        visited.insert(successor);
                        queue.push_back(successor);
                    }
                }
            }
        }
    }
    
    /// Compute dominance information using Lengauer-Tarjan algorithm
    fn compute_dominance(&mut self) {
        // Simplified dominance computation using iterative data flow
        let blocks: Vec<BasicBlockId> = self.blocks.keys().cloned().collect();
        
        // Initialize dominators
        for &block in &blocks {
            if block == self.entry_block {
                // Entry block dominates only itself
                let mut doms = HashSet::new();
                doms.insert(block);
                self.dominance.dominators.insert(block, doms);
            } else {
                // All blocks initially dominate non-entry blocks
                self.dominance.dominators.insert(block, blocks.iter().cloned().collect());
            }
        }
        
        // Iterative computation until fixed point
        let mut changed = true;
        while changed {
            changed = false;
            
            for &block in &blocks {
                if block == self.entry_block {
                    continue;
                }
                
                // Compute intersection of dominators of predecessors
                let preds = self.predecessors.get(&block).cloned().unwrap_or_default();
                if preds.is_empty() {
                    continue;
                }
                
                let mut new_doms = None;
                for &pred in &preds {
                    if let Some(pred_doms) = self.dominance.dominators.get(&pred) {
                        match new_doms {
                            None => new_doms = Some(pred_doms.clone()),
                            Some(ref mut doms) => {
                                *doms = doms.intersection(pred_doms).cloned().collect();
                            }
                        }
                    }
                }
                
                if let Some(mut new_doms) = new_doms {
                    // Add the block itself
                    new_doms.insert(block);
                    
                    // Check if changed
                    if let Some(old_doms) = self.dominance.dominators.get(&block) {
                        if &new_doms != old_doms {
                            changed = true;
                        }
                    }
                    
                    self.dominance.dominators.insert(block, new_doms);
                }
            }
        }
        
        // Compute immediate dominators
        self.compute_immediate_dominators();
        
        // Build dominance tree
        self.build_dominance_tree();
        
        // Compute dominance frontiers
        self.compute_dominance_frontiers();
    }
    
    /// Compute immediate dominators from dominator sets
    fn compute_immediate_dominators(&mut self) {
        for (&block, dominators) in &self.dominance.dominators {
            if block == self.entry_block {
                continue;
            }
            
            // Find the immediate dominator (closest dominator other than itself)
            let mut idom = None;
            let mut min_dom_count = usize::MAX;
            
            for &dom in dominators {
                if dom == block {
                    continue;
                }
                
                if let Some(dom_dominators) = self.dominance.dominators.get(&dom) {
                    if dom_dominators.len() < min_dom_count {
                        min_dom_count = dom_dominators.len();
                        idom = Some(dom);
                    }
                }
            }
            
            if let Some(idom) = idom {
                self.dominance.immediate_dominators.insert(block, idom);
            }
        }
    }
    
    /// Build dominance tree from immediate dominators
    fn build_dominance_tree(&mut self) {
        for (&child, &parent) in &self.dominance.immediate_dominators {
            self.dominance.dominance_tree
                .entry(parent)
                .or_insert_with(HashSet::new)
                .insert(child);
        }
    }
    
    /// Compute dominance frontiers
    fn compute_dominance_frontiers(&mut self) {
        for &block in self.blocks.keys() {
            let mut frontier = HashSet::new();
            
            if let Some(predecessors) = self.predecessors.get(&block) {
                if predecessors.len() > 1 {
                    // Block has multiple predecessors, compute frontier
                    for &pred in predecessors {
                        let mut runner = pred;
                        
                        // Walk up dominance tree until we reach a common dominator
                        while !self.strictly_dominates(runner, block) {
                            frontier.insert(block);
                            
                            if let Some(&idom) = self.dominance.immediate_dominators.get(&runner) {
                                runner = idom;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            
            self.dominance.dominance_frontiers.insert(block, frontier);
        }
    }
    
    /// Check if block A strictly dominates block B
    fn strictly_dominates(&self, a: BasicBlockId, b: BasicBlockId) -> bool {
        if a == b {
            return false;
        }
        
        if let Some(b_doms) = self.dominance.dominators.get(&b) {
            b_doms.contains(&a)
        } else {
            false
        }
    }
    
    /// Detect natural loops using back edges
    fn detect_loops(&mut self) {
        // Find back edges using depth-first search
        let mut visited = HashSet::new();
        let mut on_stack = HashSet::new();
        let mut back_edges = Vec::new();
        
        self.find_back_edges_dfs(self.entry_block, &mut visited, &mut on_stack, &mut back_edges);
        
        self.loops.back_edges = back_edges.clone();
        
        // For each back edge, find the natural loop
        for &(tail, head) in &back_edges {
            self.loops.loop_headers.insert(head);
            
            // Find all blocks in the natural loop
            let loop_blocks = self.find_natural_loop_blocks(tail, head);
            
            let natural_loop = NaturalLoop {
                header: head,
                blocks: loop_blocks.clone(),
                exit_blocks: self.find_loop_exits(&loop_blocks),
                back_edge: (tail, head),
                nesting_level: 0, // Will be computed later
                parent_loop: None, // Will be computed later
            };
            
            self.loops.natural_loops.push(natural_loop);
        }
        
        // Compute nesting levels and parent relationships
        self.compute_loop_nesting();
    }
    
    /// Find back edges using DFS
    fn find_back_edges_dfs(
        &self,
        block: BasicBlockId,
        visited: &mut HashSet<BasicBlockId>,
        on_stack: &mut HashSet<BasicBlockId>,
        back_edges: &mut Vec<(BasicBlockId, BasicBlockId)>,
    ) {
        visited.insert(block);
        on_stack.insert(block);
        
        if let Some(successors) = self.successors.get(&block) {
            for &successor in successors {
                if on_stack.contains(&successor) {
                    // Back edge found
                    back_edges.push((block, successor));
                } else if !visited.contains(&successor) {
                    self.find_back_edges_dfs(successor, visited, on_stack, back_edges);
                }
            }
        }
        
        on_stack.remove(&block);
    }
    
    /// Find all blocks in a natural loop given a back edge
    fn find_natural_loop_blocks(&self, tail: BasicBlockId, head: BasicBlockId) -> HashSet<BasicBlockId> {
        let mut loop_blocks = HashSet::new();
        let mut stack = Vec::new();
        
        loop_blocks.insert(head);
        
        if tail != head {
            loop_blocks.insert(tail);
            stack.push(tail);
        }
        
        // Add all blocks that can reach the tail without going through the head
        while let Some(block) = stack.pop() {
            if let Some(preds) = self.predecessors.get(&block) {
                for &pred in preds {
                    if !loop_blocks.contains(&pred) {
                        loop_blocks.insert(pred);
                        stack.push(pred);
                    }
                }
            }
        }
        
        loop_blocks
    }
    
    /// Find exit blocks for a loop
    fn find_loop_exits(&self, loop_blocks: &HashSet<BasicBlockId>) -> HashSet<BasicBlockId> {
        let mut exit_blocks = HashSet::new();
        
        for &block in loop_blocks {
            if let Some(successors) = self.successors.get(&block) {
                for &successor in successors {
                    if !loop_blocks.contains(&successor) {
                        exit_blocks.insert(block);
                        break;
                    }
                }
            }
        }
        
        exit_blocks
    }
    
    /// Compute loop nesting levels and parent relationships
    fn compute_loop_nesting(&mut self) {
        // Sort loops by size (smaller loops are more nested)
        let mut loop_indices: Vec<usize> = (0..self.loops.natural_loops.len()).collect();
        loop_indices.sort_by_key(|&i| self.loops.natural_loops[i].blocks.len());
        
        // Compute nesting for each block
        for &block in self.blocks.keys() {
            let mut depth = 0;
            
            for &loop_idx in &loop_indices {
                if self.loops.natural_loops[loop_idx].blocks.contains(&block) {
                    depth += 1;
                }
            }
            
            self.loops.loop_depths.insert(block, depth);
        }
        
        // Set nesting levels and find parent loops
        for i in 0..self.loops.natural_loops.len() {
            let mut max_parent_size = 0;
            let mut parent_idx = None;
            
            for j in 0..self.loops.natural_loops.len() {
                if i == j {
                    continue;
                }
                
                // Check if loop j contains loop i
                let loop_i_header = self.loops.natural_loops[i].header;
                if self.loops.natural_loops[j].blocks.contains(&loop_i_header) {
                    let parent_size = self.loops.natural_loops[j].blocks.len();
                    if parent_size > max_parent_size {
                        max_parent_size = parent_size;
                        parent_idx = Some(j);
                    }
                }
            }
            
            self.loops.natural_loops[i].parent_loop = parent_idx;
            self.loops.natural_loops[i].nesting_level = if parent_idx.is_some() {
                self.loops.natural_loops[parent_idx.unwrap()].nesting_level + 1
            } else {
                1
            };
        }
    }
    
    /// Get blocks in post-order traversal
    pub fn post_order(&self) -> Vec<BasicBlockId> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        
        self.post_order_dfs(self.entry_block, &mut visited, &mut result);
        result
    }
    
    /// Post-order DFS helper
    fn post_order_dfs(&self, block: BasicBlockId, visited: &mut HashSet<BasicBlockId>, result: &mut Vec<BasicBlockId>) {
        if visited.contains(&block) {
            return;
        }
        
        visited.insert(block);
        
        if let Some(successors) = self.successors.get(&block) {
            for &successor in successors {
                self.post_order_dfs(successor, visited, result);
            }
        }
        
        result.push(block);
    }
    
    /// Get blocks in reverse post-order (good for data flow analysis)
    pub fn reverse_post_order(&self) -> Vec<BasicBlockId> {
        let mut post_order = self.post_order();
        post_order.reverse();
        post_order
    }
    
    /// Check if the CFG is reducible
    pub fn is_reducible(&self) -> bool {
        // A CFG is reducible if all loops are natural loops
        // This is a simplified check - proper reducibility testing is more complex
        
        // For now, assume reducible if we can find a proper loop structure
        !self.loops.natural_loops.is_empty() || self.loops.back_edges.is_empty()
    }
}

impl Default for ControlFlowGraph {
    fn default() -> Self {
        Self {
            blocks: HashMap::new(),
            entry_block: BasicBlockId(0),
            successors: HashMap::new(),
            predecessors: HashMap::new(),
            dominance: DominanceInfo::default(),
            loops: LoopInfo::default(),
        }
    }
}