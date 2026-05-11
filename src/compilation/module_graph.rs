//! Module Dependency Graph
//!
//! Provides data structures and algorithms for tracking module dependencies,
//! detecting cycles, and determining compilation order.

use super::CompilationModuleId;
use crate::ast::SourceLocation;
use crate::error::CompilerError;
use std::collections::{HashMap, HashSet, VecDeque};

/// Represents an edge in the module dependency graph
#[derive(Debug, Clone)]
pub struct DependencyEdge {
    /// The module that has the import
    pub from: CompilationModuleId,
    /// The module being imported
    pub to: CompilationModuleId,
    /// Location of the import statement for error reporting
    pub location: SourceLocation,
}

/// Module dependency graph for managing compilation order
#[derive(Debug, Default)]
pub struct ModuleGraph {
    /// Adjacency list: module -> modules it imports
    dependencies: HashMap<CompilationModuleId, Vec<CompilationModuleId>>,

    /// Reverse adjacency list: module -> modules that import it
    dependents: HashMap<CompilationModuleId, Vec<CompilationModuleId>>,

    /// All edges with their source locations
    edges: Vec<DependencyEdge>,

    /// All nodes (modules) in the graph
    nodes: HashSet<CompilationModuleId>,
}

impl ModuleGraph {
    /// Create a new empty module graph
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a module to the graph
    pub fn add_module(&mut self, id: CompilationModuleId) {
        self.nodes.insert(id);
        self.dependencies.entry(id).or_default();
        self.dependents.entry(id).or_default();
    }

    /// Add a dependency edge: `from` imports `to`
    pub fn add_dependency(
        &mut self,
        from: CompilationModuleId,
        to: CompilationModuleId,
        location: SourceLocation,
    ) {
        // Ensure both nodes exist
        self.add_module(from);
        self.add_module(to);

        // Add to adjacency lists
        self.dependencies.entry(from).or_default().push(to);

        self.dependents.entry(to).or_default().push(from);

        // Store the edge with location
        self.edges.push(DependencyEdge { from, to, location });
    }

    /// Get the dependencies of a module (modules it imports)
    pub fn get_dependencies(&self, id: CompilationModuleId) -> &[CompilationModuleId] {
        self.dependencies
            .get(&id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get the dependents of a module (modules that import it)
    pub fn get_dependents(&self, id: CompilationModuleId) -> &[CompilationModuleId] {
        self.dependents
            .get(&id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Check if there are any circular dependencies
    ///
    /// Returns `Ok(())` if no cycles exist, or `Err` with the cycle path if found.
    pub fn check_cycles(&self) -> Result<(), CompilerError> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for &node in &self.nodes {
            if !visited.contains(&node) {
                if let Some(cycle) =
                    self.detect_cycle_dfs(node, &mut visited, &mut rec_stack, &mut path)
                {
                    return Err(self.create_cycle_error(&cycle));
                }
            }
        }

        Ok(())
    }

    /// DFS helper for cycle detection
    fn detect_cycle_dfs(
        &self,
        node: CompilationModuleId,
        visited: &mut HashSet<CompilationModuleId>,
        rec_stack: &mut HashSet<CompilationModuleId>,
        path: &mut Vec<CompilationModuleId>,
    ) -> Option<Vec<CompilationModuleId>> {
        visited.insert(node);
        rec_stack.insert(node);
        path.push(node);

        for &dep in self.get_dependencies(node) {
            if !visited.contains(&dep) {
                if let Some(cycle) = self.detect_cycle_dfs(dep, visited, rec_stack, path) {
                    return Some(cycle);
                }
            } else if rec_stack.contains(&dep) {
                // Found a cycle - extract it from the path
                let cycle_start = path.iter().position(|&x| x == dep).unwrap_or(0);
                let mut cycle = path[cycle_start..].to_vec();
                cycle.push(dep); // Complete the cycle
                return Some(cycle);
            }
        }

        path.pop();
        rec_stack.remove(&node);
        None
    }

    /// Create an error for a detected cycle
    fn create_cycle_error(&self, cycle: &[CompilationModuleId]) -> CompilerError {
        let cycle_str = cycle
            .iter()
            .map(|id| format!("{:?}", id))
            .collect::<Vec<_>>()
            .join(" -> ");

        // Try to find the location of the import that creates the cycle
        let location = if cycle.len() >= 2 {
            self.edges
                .iter()
                .find(|e| e.from == cycle[cycle.len() - 2] && e.to == cycle[cycle.len() - 1])
                .map(|e| e.location.clone())
        } else {
            None
        };

        CompilerError::import_error(
            format!("Circular dependency detected: {}", cycle_str),
            "import",
            location,
        )
    }

    /// Perform topological sort to determine compilation order
    ///
    /// Returns modules in order such that dependencies come before dependents.
    /// The entry module will be last in the returned order.
    pub fn topological_sort(
        &self,
        entry: CompilationModuleId,
    ) -> Result<Vec<CompilationModuleId>, CompilerError> {
        // First check for cycles
        self.check_cycles()?;

        // Kahn's algorithm for topological sort
        let mut in_degree: HashMap<CompilationModuleId, usize> = HashMap::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        // Calculate in-degrees
        for &node in &self.nodes {
            in_degree.insert(node, 0);
        }
        for deps in self.dependencies.values() {
            for &dep in deps {
                *in_degree.entry(dep).or_insert(0) += 1;
            }
        }

        // Find nodes with no dependencies (in-degree 0)
        for (&node, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(node);
            }
        }

        // Process nodes in topological order
        while let Some(node) = queue.pop_front() {
            result.push(node);

            for &dep in self.get_dependencies(node) {
                if let Some(degree) = in_degree.get_mut(&dep) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dep);
                    }
                }
            }
        }

        // Verify all nodes are included
        if result.len() != self.nodes.len() {
            return Err(CompilerError::codegen_error(
                "Topological sort failed - not all nodes processed (cycle exists?)",
                None,
                None,
            ));
        }

        // Reverse so dependencies come first
        result.reverse();

        // Ensure entry module is last
        if let Some(pos) = result.iter().position(|&x| x == entry) {
            result.remove(pos);
            result.push(entry);
        }

        Ok(result)
    }

    /// Get the number of modules in the graph
    pub fn module_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get all modules in the graph
    pub fn modules(&self) -> impl Iterator<Item = &CompilationModuleId> {
        self.nodes.iter()
    }

    /// Check if a module has any dependencies
    pub fn has_dependencies(&self, id: CompilationModuleId) -> bool {
        !self.get_dependencies(id).is_empty()
    }

    /// Get the total number of edges (imports)
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_location() -> SourceLocation {
        SourceLocation {
            file: "test.cln".to_string(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        }
    }

    #[test]
    fn test_empty_graph() {
        let graph = ModuleGraph::new();
        assert_eq!(graph.module_count(), 0);
    }

    #[test]
    fn test_single_module() {
        let mut graph = ModuleGraph::new();
        let main = CompilationModuleId(0);
        graph.add_module(main);

        assert_eq!(graph.module_count(), 1);
        assert!(graph.get_dependencies(main).is_empty());
    }

    #[test]
    fn test_simple_dependency() {
        let mut graph = ModuleGraph::new();
        let main = CompilationModuleId(0);
        let utils = CompilationModuleId(1);

        graph.add_dependency(main, utils, test_location());

        assert_eq!(graph.module_count(), 2);
        assert_eq!(graph.get_dependencies(main), &[utils]);
        assert_eq!(graph.get_dependents(utils), &[main]);
    }

    #[test]
    fn test_no_cycle() {
        let mut graph = ModuleGraph::new();
        let a = CompilationModuleId(0);
        let b = CompilationModuleId(1);
        let c = CompilationModuleId(2);

        // A -> B -> C (no cycle)
        graph.add_dependency(a, b, test_location());
        graph.add_dependency(b, c, test_location());

        assert!(graph.check_cycles().is_ok());
    }

    #[test]
    fn test_simple_cycle_detected() {
        let mut graph = ModuleGraph::new();
        let a = CompilationModuleId(0);
        let b = CompilationModuleId(1);

        // A -> B -> A (cycle)
        graph.add_dependency(a, b, test_location());
        graph.add_dependency(b, a, test_location());

        assert!(graph.check_cycles().is_err());
    }

    #[test]
    fn test_three_node_cycle_detected() {
        let mut graph = ModuleGraph::new();
        let a = CompilationModuleId(0);
        let b = CompilationModuleId(1);
        let c = CompilationModuleId(2);

        // A -> B -> C -> A (cycle)
        graph.add_dependency(a, b, test_location());
        graph.add_dependency(b, c, test_location());
        graph.add_dependency(c, a, test_location());

        assert!(graph.check_cycles().is_err());
    }

    #[test]
    fn test_topological_sort_linear() {
        let mut graph = ModuleGraph::new();
        let a = CompilationModuleId(0);
        let b = CompilationModuleId(1);
        let c = CompilationModuleId(2);

        // A imports B, B imports C
        // Compilation order should be: C, B, A
        graph.add_dependency(a, b, test_location());
        graph.add_dependency(b, c, test_location());

        let order = graph.topological_sort(a).unwrap();

        // C should come before B, B before A
        let c_pos = order.iter().position(|&x| x == c).unwrap();
        let b_pos = order.iter().position(|&x| x == b).unwrap();
        let a_pos = order.iter().position(|&x| x == a).unwrap();

        assert!(c_pos < b_pos, "C should come before B");
        assert!(b_pos < a_pos, "B should come before A");
        assert_eq!(a_pos, order.len() - 1, "Entry (A) should be last");
    }

    #[test]
    fn test_topological_sort_diamond() {
        let mut graph = ModuleGraph::new();
        let main = CompilationModuleId(0);
        let a = CompilationModuleId(1);
        let b = CompilationModuleId(2);
        let common = CompilationModuleId(3);

        // Diamond pattern: main -> a -> common, main -> b -> common
        graph.add_dependency(main, a, test_location());
        graph.add_dependency(main, b, test_location());
        graph.add_dependency(a, common, test_location());
        graph.add_dependency(b, common, test_location());

        let order = graph.topological_sort(main).unwrap();

        // common should come before both a and b
        // a and b should come before main
        let common_pos = order.iter().position(|&x| x == common).unwrap();
        let a_pos = order.iter().position(|&x| x == a).unwrap();
        let b_pos = order.iter().position(|&x| x == b).unwrap();
        let main_pos = order.iter().position(|&x| x == main).unwrap();

        assert!(common_pos < a_pos, "common should come before a");
        assert!(common_pos < b_pos, "common should come before b");
        assert!(a_pos < main_pos, "a should come before main");
        assert!(b_pos < main_pos, "b should come before main");
        assert_eq!(main_pos, order.len() - 1, "Entry (main) should be last");
    }
}
