use crate::ast::Type;
use std::collections::HashMap;

/// String operation builtin functions
/// Handles string manipulation operations in the string namespace
pub struct StringFunctions;

impl StringFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        let mut functions = HashMap::new();
        
        // String operations in the string namespace
        let string_function_definitions = vec![
            ("concat", vec![(vec![Type::String, Type::String], Type::String, 2)]),
            ("compare", vec![(vec![Type::String, Type::String], Type::Integer, 2)]),
            ("indexOf", vec![(vec![Type::String, Type::String], Type::Integer, 2)]),
            ("contains", vec![(vec![Type::String, Type::String], Type::Boolean, 2)]),
            ("lastIndexOf", vec![(vec![Type::String, Type::String], Type::Integer, 2)]),
            ("startsWith", vec![(vec![Type::String, Type::String], Type::Boolean, 2)]),
            ("endsWith", vec![(vec![Type::String, Type::String], Type::Boolean, 2)]),
            ("toUpperCase", vec![(vec![Type::String], Type::String, 1)]),
            ("toLowerCase", vec![(vec![Type::String], Type::String, 1)]),
            ("length", vec![(vec![Type::String], Type::Integer, 1)]),
            ("isEmpty", vec![(vec![Type::String], Type::Boolean, 1)]),
            ("replace", vec![(vec![Type::String, Type::String, Type::String], Type::String, 3)]),
            // Additional string functions needed by comprehensive tests
            ("substring", vec![(vec![Type::String, Type::Integer, Type::Integer], Type::String, 3)]),
            ("trim", vec![(vec![Type::String], Type::String, 1)]),
            ("replaceAll", vec![(vec![Type::String, Type::String, Type::String], Type::String, 3)]),
            ("split", vec![(vec![Type::String, Type::String], Type::List(Box::new(Type::String)), 2)]),
            ("charAt", vec![(vec![Type::String, Type::Integer], Type::String, 2)]),
            ("charCodeAt", vec![(vec![Type::String, Type::Integer], Type::Integer, 2)]),
            ("slice", vec![(vec![Type::String, Type::Integer, Type::Integer], Type::String, 3)]),
            ("isNumeric", vec![(vec![Type::String], Type::Boolean, 1)]),
            ("isAlpha", vec![(vec![Type::String], Type::Boolean, 1)]),
            ("isAlphaNumeric", vec![(vec![Type::String], Type::Boolean, 1)]),
            ("pad", vec![(vec![Type::String, Type::Integer, Type::String], Type::String, 3)]),
            ("padStart", vec![(vec![Type::String, Type::Integer, Type::String], Type::String, 3)]),
            ("padEnd", vec![(vec![Type::String, Type::Integer, Type::String], Type::String, 3)]),
            ("reverse", vec![(vec![Type::String], Type::String, 1)]),
            ("repeat", vec![(vec![Type::String, Type::Integer], Type::String, 2)]),
            ("trimStart", vec![(vec![Type::String], Type::String, 1)]),
            ("trimEnd", vec![(vec![Type::String], Type::String, 1)]),
            ("leftTrim", vec![(vec![Type::String], Type::String, 1)]),
            ("rightTrim", vec![(vec![Type::String], Type::String, 1)]),
            ("isEmpty", vec![(vec![Type::String], Type::Boolean, 1)]),
            ("isNotEmpty", vec![(vec![Type::String], Type::Boolean, 1)]),
            ("join", vec![(vec![Type::List(Box::new(Type::String)), Type::String], Type::String, 2)]),
            ("format", vec![(vec![Type::String], Type::String, 1)]), // For string.format() calls
        ];
        
        // Register all string functions with the "string." prefix
        // This handles both namespace calls (string.toUpperCase(text)) and method calls (text.toUpperCase())
        for (func_name, overloads) in string_function_definitions {
            functions.insert(format!("string.{}", func_name), overloads);
        }
        
        functions
    }
}