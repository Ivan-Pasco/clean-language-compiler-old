use crate::ast::Type;
use std::collections::HashMap;

/// Type conversion builtin functions
/// Handles conversions between primitive types (integer, number, string, boolean)
pub struct ConversionFunctions;

impl ConversionFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        let mut functions = HashMap::new();
        
        // String conversion functions
        functions.insert("to_string".to_string(), vec![(vec![Type::Integer], Type::String, 1)]);
        functions.insert("int_to_string".to_string(), vec![(vec![Type::Integer], Type::String, 1)]);
        functions.insert("number_to_string".to_string(), vec![(vec![Type::Number], Type::String, 1)]);
        functions.insert("float_to_string".to_string(), vec![(vec![Type::Number], Type::String, 1)]);
        functions.insert("bool_to_string".to_string(), vec![(vec![Type::Boolean], Type::String, 1)]);
        
        // Numeric conversion functions
        functions.insert("to_number".to_string(), vec![(vec![Type::String], Type::Number, 1)]);
        functions.insert("to_integer".to_string(), vec![(vec![Type::Number], Type::Integer, 1)]);
        functions.insert("string_to_int".to_string(), vec![(vec![Type::String], Type::Integer, 1)]);
        functions.insert("string_to_float".to_string(), vec![(vec![Type::String], Type::Number, 1)]);
        functions.insert("float_to_int".to_string(), vec![(vec![Type::Number], Type::Integer, 1)]);
        functions.insert("int_to_float".to_string(), vec![(vec![Type::Integer], Type::Number, 1)]);
        
        // toString() methods for basic types (method-style calls)
        functions.insert("number.toString".to_string(), vec![(vec![Type::Number], Type::String, 1)]);
        functions.insert("integer.toString".to_string(), vec![(vec![Type::Integer], Type::String, 1)]);
        functions.insert("boolean.toString".to_string(), vec![(vec![Type::Boolean], Type::String, 1)]);
        functions.insert("string.toString".to_string(), vec![(vec![Type::String], Type::String, 1)]); // identity for string
        
        // toString() methods for sized integer types
        for bits in [8, 16, 32, 64] {
            let signed_type = Type::IntegerSized { bits, unsigned: false };
            let unsigned_type = Type::IntegerSized { bits, unsigned: true };
            functions.insert(format!("integer:{}.toString", bits), vec![(vec![signed_type.clone()], Type::String, 1)]);
            functions.insert(format!("integer:{}u.toString", bits), vec![(vec![unsigned_type.clone()], Type::String, 1)]);
        }
        
        // toString() methods for sized number types
        for bits in [32, 64] {
            let number_type = Type::NumberSized { bits };
            functions.insert(format!("number:{}.toString", bits), vec![(vec![number_type], Type::String, 1)]);
        }
        
        // toInteger() methods for basic types (method-style calls)
        functions.insert("number.toInteger".to_string(), vec![(vec![Type::Number], Type::Integer, 1)]);
        functions.insert("string.toInteger".to_string(), vec![(vec![Type::String], Type::Integer, 1)]);
        functions.insert("boolean.toInteger".to_string(), vec![(vec![Type::Boolean], Type::Integer, 1)]);
        functions.insert("integer.toInteger".to_string(), vec![(vec![Type::Integer], Type::Integer, 1)]); // identity for integer
        
        // toNumber() methods for basic types (method-style calls)
        functions.insert("integer.toNumber".to_string(), vec![(vec![Type::Integer], Type::Number, 1)]);
        functions.insert("string.toNumber".to_string(), vec![(vec![Type::String], Type::Number, 1)]);
        functions.insert("boolean.toNumber".to_string(), vec![(vec![Type::Boolean], Type::Number, 1)]);
        functions.insert("number.toNumber".to_string(), vec![(vec![Type::Number], Type::Number, 1)]); // identity for number
        
        // toBoolean() methods for basic types (method-style calls)
        functions.insert("integer.toBoolean".to_string(), vec![(vec![Type::Integer], Type::Boolean, 1)]);
        functions.insert("number.toBoolean".to_string(), vec![(vec![Type::Number], Type::Boolean, 1)]);
        functions.insert("string.toBoolean".to_string(), vec![(vec![Type::String], Type::Boolean, 1)]);
        functions.insert("boolean.toBoolean".to_string(), vec![(vec![Type::Boolean], Type::Boolean, 1)]); // identity for boolean
        
        // length() methods for basic types (number of digits/characters)
        functions.insert("number.length".to_string(), vec![(vec![Type::Number], Type::Integer, 1)]);
        functions.insert("integer.length".to_string(), vec![(vec![Type::Integer], Type::Integer, 1)]);
        functions.insert("string.length".to_string(), vec![(vec![Type::String], Type::Integer, 1)]);
        
        functions
    }
}