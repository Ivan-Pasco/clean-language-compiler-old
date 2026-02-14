use crate::ast::Type;
use std::collections::HashMap;

/// Comparison and conditional operation builtin functions
/// This module is retained for structural consistency but currently provides no functions
/// as comparison and conditional operations are handled through native operators
pub struct ComparisonFunctions;

impl ComparisonFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        HashMap::new()
    }
}