use crate::ast::Type;
use std::collections::HashMap;

/// Console input builtin functions
/// Handles user input operations with different type conversions
pub struct InputFunctions;

impl InputFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        let mut functions = HashMap::new();
        
        // Basic console input functions
        functions.insert("input".to_string(), vec![(vec![Type::String], Type::String, 1)]);
        functions.insert("inputInteger".to_string(), vec![(vec![Type::String], Type::Integer, 1)]);
        functions.insert("inputFloat".to_string(), vec![(vec![Type::String], Type::Number, 1)]);
        functions.insert("inputYesNo".to_string(), vec![(vec![Type::String], Type::Boolean, 1)]);
        
        // Input namespace methods
        functions.insert("input.integer".to_string(), vec![(vec![Type::String], Type::Integer, 1)]);
        functions.insert("input.number".to_string(), vec![(vec![Type::String], Type::Number, 1)]);
        functions.insert("input.float".to_string(), vec![(vec![Type::String], Type::Number, 1)]);
        functions.insert("input.boolean".to_string(), vec![(vec![Type::String], Type::Boolean, 1)]);
        functions.insert("input.yesNo".to_string(), vec![(vec![Type::String], Type::Boolean, 1)]);
        
        // Console class static methods
        functions.insert("Console.inputInteger".to_string(), vec![(vec![Type::String], Type::Integer, 1)]);
        functions.insert("Console.inputNumber".to_string(), vec![(vec![Type::String], Type::Number, 1)]);
        functions.insert("Console.inputBoolean".to_string(), vec![(vec![Type::String], Type::Boolean, 1)]);
        functions.insert("Console.inputYesNo".to_string(), vec![(vec![Type::String], Type::Boolean, 1)]);
        functions.insert("Console.inputRange".to_string(), vec![(vec![Type::String, Type::Integer, Type::Integer], Type::Integer, 3)]);
        
        functions
    }
}