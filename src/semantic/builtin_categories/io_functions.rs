use crate::ast::Type;
use std::collections::HashMap;

/// Input/Output builtin functions
/// Handles print, println, and test assertion functions
pub struct IoFunctions;

impl IoFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        let mut functions = HashMap::new();
        
        // Core print functions
        functions.insert("print".to_string(), vec![(vec![Type::String], Type::Void, 1)]);
        functions.insert("println".to_string(), vec![(vec![Type::String], Type::Void, 1)]);
        functions.insert("printl".to_string(), vec![(vec![Type::String], Type::Void, 1)]);
        
        // Test assertion functions
        functions.insert("mustBeTrue".to_string(), vec![(vec![Type::Boolean], Type::Void, 1)]);
        functions.insert("mustBeFalse".to_string(), vec![(vec![Type::Boolean], Type::Void, 1)]);
        functions.insert("mustBeEqual".to_string(), vec![(vec![Type::Any, Type::Any], Type::Void, 2)]);
        
        functions
    }
}