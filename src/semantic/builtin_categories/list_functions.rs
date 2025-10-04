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
        // list.add returns the modified list per specification
        functions.insert("list.add".to_string(), vec![(vec![list_type.clone(), Type::Any], list_type.clone(), 2)]);
        functions.insert("list.get".to_string(), vec![(vec![list_type.clone(), Type::Integer], Type::Any, 2)]);
        functions.insert("list.set".to_string(), vec![(vec![list_type.clone(), Type::Integer, Type::Any], Type::Void, 3)]);
        functions.insert("list.clear".to_string(), vec![(vec![list_type.clone()], Type::Void, 1)]);
        // list.sort and list.reverse return the modified list per specification
        functions.insert("list.sort".to_string(), vec![(vec![list_type.clone()], list_type.clone(), 1)]);
        functions.insert("list.reverse".to_string(), vec![(vec![list_type.clone()], list_type.clone(), 1)]);
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

        // Search operations
        functions.insert("list.indexOf".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Integer, 2)]);
        functions.insert("list.lastIndexOf".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Integer, 2)]);

        // List manipulation and transformation
        functions.insert("list.insert".to_string(), vec![(vec![list_type.clone(), Type::Integer, Type::Any], list_type.clone(), 3)]);
        functions.insert("list.slice".to_string(), vec![(vec![list_type.clone(), Type::Integer, Type::Integer], list_type.clone(), 3)]);
        functions.insert("list.concat".to_string(), vec![(vec![list_type.clone(), list_type.clone()], list_type.clone(), 2)]);

        // Element access
        functions.insert("list.first".to_string(), vec![(vec![list_type.clone()], Type::Any, 1)]);
        functions.insert("list.last".to_string(), vec![(vec![list_type.clone()], Type::Any, 1)]);

        // String operations
        functions.insert("list.join".to_string(), vec![(vec![list_type.clone(), Type::String], Type::String, 2)]);

        // List creation helpers
        functions.insert("list.fill".to_string(), vec![(vec![Type::Integer, Type::Any], list_type.clone(), 2)]);
        functions.insert("list.range".to_string(), vec![(vec![Type::Integer, Type::Integer], list_type.clone(), 2)]);
        
        // Underscore versions (used by resolver when converting namespace calls)
        // Basic operations
        functions.insert("list_add".to_string(), vec![(vec![list_type.clone(), Type::Any], list_type.clone(), 2)]);
        functions.insert("list_get".to_string(), vec![(vec![list_type.clone(), Type::Integer], Type::Any, 2)]);
        functions.insert("list_set".to_string(), vec![(vec![list_type.clone(), Type::Integer, Type::Any], Type::Void, 3)]);
        functions.insert("list_clear".to_string(), vec![(vec![list_type.clone()], Type::Void, 1)]);
        functions.insert("list_sort".to_string(), vec![(vec![list_type.clone()], list_type.clone(), 1)]);
        functions.insert("list_reverse".to_string(), vec![(vec![list_type.clone()], list_type.clone(), 1)]);
        functions.insert("list_contains".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Boolean, 2)]);
        functions.insert("list_remove".to_string(), vec![
            (vec![list_type.clone(), Type::Integer], Type::Any, 2), // remove(index)
            (vec![list_type.clone()], Type::Any, 1) // remove() - behavior-dependent
        ]);

        // Instance-style methods
        functions.insert("list_size".to_string(), vec![(vec![list_type.clone()], Type::Integer, 1)]);
        functions.insert("list_length".to_string(), vec![(vec![list_type.clone()], Type::Integer, 1)]);
        functions.insert("list_isEmpty".to_string(), vec![(vec![list_type.clone()], Type::Boolean, 1)]);
        functions.insert("list_isNotEmpty".to_string(), vec![(vec![list_type.clone()], Type::Boolean, 1)]);
        functions.insert("list_peek".to_string(), vec![(vec![list_type.clone()], Type::Any, 1)]);

        // Search operations
        functions.insert("list_indexOf".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Integer, 2)]);
        functions.insert("list_index_of".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Integer, 2)]);
        functions.insert("list_lastIndexOf".to_string(), vec![(vec![list_type.clone(), Type::Any], Type::Integer, 2)]);

        // Manipulation and transformation
        functions.insert("list_insert".to_string(), vec![(vec![list_type.clone(), Type::Integer, Type::Any], list_type.clone(), 3)]);
        functions.insert("list_slice".to_string(), vec![(vec![list_type.clone(), Type::Integer, Type::Integer], list_type.clone(), 3)]);
        functions.insert("list_concat".to_string(), vec![(vec![list_type.clone(), list_type.clone()], list_type.clone(), 2)]);

        // Element access
        functions.insert("list_first".to_string(), vec![(vec![list_type.clone()], Type::Any, 1)]);
        functions.insert("list_last".to_string(), vec![(vec![list_type.clone()], Type::Any, 1)]);

        // String operations
        functions.insert("list_join".to_string(), vec![(vec![list_type.clone(), Type::String], Type::String, 2)]);

        // List creation helpers
        functions.insert("list_fill".to_string(), vec![(vec![Type::Integer, Type::Any], list_type.clone(), 2)]);
        functions.insert("list_range".to_string(), vec![(vec![Type::Integer, Type::Integer], list_type.clone(), 2)]);

        // Legacy names (kept for compatibility)
        functions.insert("list_push".to_string(), vec![(vec![list_type.clone(), Type::Any], list_type.clone(), 2)]);
        functions.insert("list_pop".to_string(), vec![(vec![list_type.clone()], Type::Any, 1)]);
        
        functions
    }
}