use crate::ast::Type;
use std::collections::HashMap;

/// List operation builtin functions
/// Handles list manipulation and query operations
pub struct ListFunctions;

impl ListFunctions {
    pub fn get_functions() -> HashMap<String, Vec<(Vec<Type>, Type, usize)>> {
        let mut functions = HashMap::new();
        
        // List operations (static method calls)
        let list_type = Type::List(Box::new(Type::Any));
        
        // Basic list operations
        functions.insert("list.length".to_string(), vec![(vec![list_type.clone()], Type::Integer, 1)]);
        functions.insert("list.isEmpty".to_string(), vec![(vec![list_type.clone()], Type::Boolean, 1)]);
        functions.insert("list.add".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Void, 2)]);
        functions.insert("list.get".to_string(), vec![(vec![list_type.clone(), Type::Integer], Type::Any, 2)]);
        functions.insert("list.set".to_string(), vec![(vec![list_type.clone(), Type::Integer, Type::Any], Type::Void, 3)]);
        functions.insert("list.clear".to_string(), vec![(vec![list_type.clone()], Type::Void, 1)]);
        functions.insert("list.sort".to_string(), vec![(vec![list_type.clone()], Type::Void, 1)]);
        functions.insert("list.reverse".to_string(), vec![(vec![list_type.clone()], Type::Void, 1)]);
        functions.insert("list.contains".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Boolean, 2)]);
        
        // remove() method - support both overloads: remove(index) and remove()
        functions.insert("list.remove".to_string(), vec![
            (vec![list_type.clone(), Type::Integer], Type::Any, 2), // remove(index)
            (vec![list_type.clone()], Type::Any, 1) // remove() - behavior-dependent
        ]);
        
        // List instance methods (for method call syntax like myList.size())
        functions.insert("list.size".to_string(), vec![(vec![list_type.clone()], Type::Integer, 1)]);
        functions.insert("list.peek".to_string(), vec![(vec![list_type.clone()], Type::Any, 1)]);
        functions.insert("list.isNotEmpty".to_string(), vec![(vec![list_type.clone()], Type::Boolean, 1)]);
        
        // Missing standalone list functions (these were implemented in stdlib but not registered)
        functions.insert("list_push".to_string(), vec![(vec![list_type.clone(), Type::Any], list_type.clone(), 2)]);
        functions.insert("list_pop".to_string(), vec![(vec![list_type.clone()], Type::Any, 1)]);
        functions.insert("list_length".to_string(), vec![(vec![list_type.clone()], Type::Integer, 1)]);
        functions.insert("list_contains".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Boolean, 2)]);
        functions.insert("list_index_of".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Integer, 2)]);
        functions.insert("list_slice".to_string(), vec![(vec![list_type.clone(), Type::Integer, Type::Integer], list_type.clone(), 3)]);
        functions.insert("list_concat".to_string(), vec![(vec![list_type.clone(), list_type.clone()], list_type.clone(), 2)]);
        functions.insert("list_reverse".to_string(), vec![(vec![list_type.clone()], list_type.clone(), 1)]);
        functions.insert("list_join".to_string(), vec![(vec![list_type.clone(), Type::String], Type::String, 2)]);
        functions.insert("list_insert".to_string(), vec![(vec![list_type.clone(), Type::Integer, Type::Any], list_type.clone(), 3)]);
        functions.insert("list_remove".to_string(), vec![(vec![list_type.clone(), Type::Integer], Type::Any, 2)]);
        
        functions
    }
}