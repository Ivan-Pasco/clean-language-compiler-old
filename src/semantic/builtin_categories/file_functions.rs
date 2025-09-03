use crate::ast::Type;
use std::collections::HashMap;

/// File I/O builtin functions
/// Handles file read, write, append, exists, delete operations
pub struct FileFunctions;

impl FileFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        let mut functions = HashMap::new();
        
        // File operations namespace functions
        functions.insert("file.read".to_string(), vec![(vec![Type::String], Type::String, 1)]);
        functions.insert("file.write".to_string(), vec![(vec![Type::String, Type::String], Type::Boolean, 2)]);
        functions.insert("file.append".to_string(), vec![(vec![Type::String, Type::String], Type::Boolean, 2)]);
        functions.insert("file.exists".to_string(), vec![(vec![Type::String], Type::Boolean, 1)]);
        functions.insert("file.delete".to_string(), vec![(vec![Type::String], Type::Boolean, 1)]);
        functions.insert("file.size".to_string(), vec![(vec![Type::String], Type::Integer, 1)]);
        functions.insert("file.copy".to_string(), vec![(vec![Type::String, Type::String], Type::Boolean, 2)]);
        functions.insert("file.move".to_string(), vec![(vec![Type::String, Type::String], Type::Boolean, 2)]);
        
        // Directory operations
        functions.insert("dir.create".to_string(), vec![(vec![Type::String], Type::Boolean, 1)]);
        functions.insert("dir.list".to_string(), vec![(vec![Type::String], Type::List(Box::new(Type::String)), 1)]);
        
        functions
    }
}