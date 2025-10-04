use crate::ast::Type;
use std::collections::HashMap;
use lazy_static::lazy_static;

use super::builtin_categories::{
    io_functions::IoFunctions,
    math_functions::MathFunctions,
    conversion_functions::ConversionFunctions,
    input_functions::InputFunctions,
    comparison_functions::ComparisonFunctions,
    string_functions::StringFunctions,
    list_functions::ListFunctions,
    utility_functions::UtilityFunctions,
    file_functions::FileFunctions,
};

/// Type signature for builtin functions: (parameter_types, return_type, required_param_count)
pub type FunctionSignature = (Vec<Type>, Type, usize);

/// Centralized builtin function registry with modular design
/// Each category is managed by a separate module for maintainability
pub struct BuiltinRegistry {
    functions: HashMap<String, Vec<FunctionSignature>>,
}

impl BuiltinRegistry {
    /// Create a new builtin registry with all function categories loaded
    pub fn new() -> Self {
        let mut registry = Self {
            functions: HashMap::new(),
        };
        
        // Load all function categories
        registry.load_io_functions();
        registry.load_math_functions();
        registry.load_conversion_functions();
        registry.load_input_functions();
        registry.load_comparison_functions();
        registry.load_string_functions();
        registry.load_list_functions();
        registry.load_utility_functions();
        registry.load_file_functions();
        
        registry
    }
    
    /// Get function signatures for a given function name
    #[allow(dead_code)]
    pub fn get_function(&self, name: &str) -> Option<&Vec<FunctionSignature>> {
        self.functions.get(name)
    }
    
    /// Check if a function is a builtin
    pub fn is_builtin(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }
    
    /// Get all function names (for debugging/introspection)
    #[allow(dead_code)]
    pub fn get_all_function_names(&self) -> Vec<&String> {
        self.functions.keys().collect()
    }
    
    /// Get a copy of the entire function table (for compatibility with existing code)
    pub fn get_function_table(&self) -> HashMap<String, Vec<FunctionSignature>> {
        self.functions.clone()
    }
    
    // Category loading methods
    
    fn load_io_functions(&mut self) {
        let io_functions = IoFunctions::get_functions();
        for (name, signatures) in io_functions {
            self.functions.insert(name, signatures);
        }
    }
    
    fn load_math_functions(&mut self) {
        let math_functions = MathFunctions::get_functions();
        for (name, signatures) in math_functions {
            self.functions.insert(name, signatures);
        }
    }
    
    fn load_conversion_functions(&mut self) {
        let conversion_functions = ConversionFunctions::get_functions();
        for (name, signatures) in conversion_functions {
            self.functions.insert(name, signatures);
        }
    }
    
    fn load_input_functions(&mut self) {
        let input_functions = InputFunctions::get_functions();
        for (name, signatures) in input_functions {
            self.functions.insert(name, signatures);
        }
    }
    
    fn load_comparison_functions(&mut self) {
        let comparison_functions = ComparisonFunctions::get_functions();
        for (name, signatures) in comparison_functions {
            self.functions.insert(name, signatures);
        }
    }
    
    fn load_string_functions(&mut self) {
        let string_functions = StringFunctions::get_functions();
        for (name, signatures) in string_functions {
            self.functions.insert(name, signatures);
        }
    }
    
    fn load_list_functions(&mut self) {
        let list_functions = ListFunctions::get_functions();
        for (name, signatures) in list_functions {
            self.functions.insert(name, signatures);
        }
    }
    
    fn load_utility_functions(&mut self) {
        let utility_functions = UtilityFunctions::get_functions();
        for (name, signatures) in utility_functions {
            self.functions.insert(name, signatures);
        }
    }
    
    fn load_file_functions(&mut self) {
        let file_functions = FileFunctions::get_functions();
        for (name, signatures) in file_functions {
            self.functions.insert(name, signatures);
        }
    }
}

impl Default for BuiltinRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Static instance for performance (similar to the original BUILTIN_FUNCTIONS)
lazy_static! {
    pub static ref BUILTIN_REGISTRY: BuiltinRegistry = BuiltinRegistry::new();
}

/// Compatibility function to get the function table in the same format as before
pub fn get_builtin_functions() -> HashMap<String, Vec<FunctionSignature>> {
    BUILTIN_REGISTRY.get_function_table()
}