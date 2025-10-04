use crate::ast::Type;
use std::collections::HashMap;

/// Comparison and conditional operation builtin functions
/// Handles comparison operations and conditional expressions for type safety
pub struct ComparisonFunctions;

impl ComparisonFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        let mut functions = HashMap::new();
        
        // Integer comparison functions
        functions.insert("compare.integer.equal".to_string(), vec![(vec![Type::Integer, Type::Integer], Type::Boolean, 2)]);
        functions.insert("compare.integer.notEqual".to_string(), vec![(vec![Type::Integer, Type::Integer], Type::Boolean, 2)]);
        functions.insert("compare.integer.lessThan".to_string(), vec![(vec![Type::Integer, Type::Integer], Type::Boolean, 2)]);
        functions.insert("compare.integer.greaterThan".to_string(), vec![(vec![Type::Integer, Type::Integer], Type::Boolean, 2)]);
        functions.insert("compare.integer.lessEqual".to_string(), vec![(vec![Type::Integer, Type::Integer], Type::Boolean, 2)]);
        functions.insert("compare.integer.greaterEqual".to_string(), vec![(vec![Type::Integer, Type::Integer], Type::Boolean, 2)]);
        
        // Number comparison functions
        functions.insert("compare.number.equal".to_string(), vec![(vec![Type::Number, Type::Number], Type::Boolean, 2)]);
        functions.insert("compare.number.notEqual".to_string(), vec![(vec![Type::Number, Type::Number], Type::Boolean, 2)]);
        functions.insert("compare.number.lessThan".to_string(), vec![(vec![Type::Number, Type::Number], Type::Boolean, 2)]);
        functions.insert("compare.number.greaterThan".to_string(), vec![(vec![Type::Number, Type::Number], Type::Boolean, 2)]);
        functions.insert("compare.number.lessEqual".to_string(), vec![(vec![Type::Number, Type::Number], Type::Boolean, 2)]);
        functions.insert("compare.number.greaterEqual".to_string(), vec![(vec![Type::Number, Type::Number], Type::Boolean, 2)]);
        
        // Conditional expression functions (ternary operator equivalents)
        functions.insert("conditional.integer".to_string(), vec![(vec![Type::Boolean, Type::Integer, Type::Integer], Type::Integer, 3)]);
        functions.insert("conditional.number".to_string(), vec![(vec![Type::Boolean, Type::Number, Type::Number], Type::Number, 3)]);
        functions.insert("conditional.string".to_string(), vec![(vec![Type::Boolean, Type::String, Type::String], Type::String, 3)]);
        functions.insert("conditional.boolean".to_string(), vec![(vec![Type::Boolean, Type::Boolean, Type::Boolean], Type::Boolean, 3)]);
        
        // Logical operation functions
        functions.insert("logical.and".to_string(), vec![(vec![Type::Boolean, Type::Boolean], Type::Boolean, 2)]);
        functions.insert("logical.or".to_string(), vec![(vec![Type::Boolean, Type::Boolean], Type::Boolean, 2)]);
        functions.insert("logical.not".to_string(), vec![(vec![Type::Boolean], Type::Boolean, 1)]);
        
        functions
    }
}