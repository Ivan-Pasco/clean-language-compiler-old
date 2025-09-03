use crate::ast::Type;
use std::collections::HashMap;

/// Mathematical builtin functions
/// Supports both lowercase (math.) and uppercase (Math.) variants
pub struct MathFunctions;

impl MathFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        let mut functions = HashMap::new();
        
        // Define all math functions with their signatures
        let math_function_definitions = vec![
            ("abs", vec![(vec![Type::Integer], Type::Integer, 1), (vec![Type::Number], Type::Number, 1)]),
            ("sqrt", vec![(vec![Type::Number], Type::Number, 1)]),
            ("sin", vec![(vec![Type::Number], Type::Number, 1)]),
            ("cos", vec![(vec![Type::Number], Type::Number, 1)]),
            ("tan", vec![(vec![Type::Number], Type::Number, 1)]),
            ("ln", vec![(vec![Type::Number], Type::Number, 1)]),
            ("log10", vec![(vec![Type::Number], Type::Number, 1)]),
            ("log2", vec![(vec![Type::Number], Type::Number, 1)]),
            ("exp", vec![(vec![Type::Number], Type::Number, 1)]),
            ("exp2", vec![(vec![Type::Number], Type::Number, 1)]),
            ("sinh", vec![(vec![Type::Number], Type::Number, 1)]),
            ("cosh", vec![(vec![Type::Number], Type::Number, 1)]),
            ("tanh", vec![(vec![Type::Number], Type::Number, 1)]),
            ("asin", vec![(vec![Type::Number], Type::Number, 1)]),
            ("acos", vec![(vec![Type::Number], Type::Number, 1)]),
            ("atan", vec![(vec![Type::Number], Type::Number, 1)]),
            ("atan2", vec![(vec![Type::Number, Type::Number], Type::Number, 2)]),
            ("pi", vec![(vec![], Type::Number, 0)]),
            ("e", vec![(vec![], Type::Number, 0)]),
            ("tau", vec![(vec![], Type::Number, 0)]),
            ("floor", vec![(vec![Type::Number], Type::Number, 1)]),
            ("ceil", vec![(vec![Type::Number], Type::Number, 1)]),
            ("round", vec![(vec![Type::Number], Type::Number, 1)]),
            ("trunc", vec![(vec![Type::Number], Type::Number, 1)]),
            ("min", vec![(vec![Type::Number, Type::Number], Type::Number, 2)]),
            ("max", vec![(vec![Type::Number, Type::Number], Type::Number, 2)]),
            ("mod", vec![(vec![Type::Number, Type::Number], Type::Number, 2)]),
            ("pow", vec![(vec![Type::Number, Type::Number], Type::Number, 2)]),
        ];
        
        // Register both lowercase (math.) and uppercase (Math.) variants
        for (func_name, overloads) in math_function_definitions {
            functions.insert(format!("math.{}", func_name), overloads.clone());
            functions.insert(format!("Math.{}", func_name), overloads);
        }
        
        functions
    }
}