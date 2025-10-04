use crate::ast::Type;
use std::collections::HashMap;

/// Utility builtin functions
/// Handles miscellaneous functions that don't fit into specific categories
pub struct UtilityFunctions;

impl UtilityFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        let _functions = HashMap::new();
        
        // Currently no utility functions - this module exists for future extensibility
        // Functions like debug output, profiling, or other utilities can be added here
        
        _functions
    }
}