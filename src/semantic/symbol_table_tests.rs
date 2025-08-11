//! Tests for the comprehensive symbol table system

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Type, SourceLocation, Visibility, FunctionModifier};
    use crate::semantic::{SymbolTable, ScopeType, SymbolKind};

    fn create_test_location() -> Option<SourceLocation> {
        Some(SourceLocation::new(1, 1, "test.cln"))
    }

    #[test]
    fn test_symbol_table_creation() {
        let symbol_table = SymbolTable::new();
        assert_eq!(symbol_table.current_scope_level(), 0);
        assert_eq!(symbol_table.current_scope_type(), &ScopeType::Global);
    }

    #[test]
    fn test_scope_management() {
        let mut symbol_table = SymbolTable::new();
        
        // Enter function scope
        let function_scope_id = symbol_table.enter_scope(ScopeType::Function("test_func".to_string()));
        assert_eq!(symbol_table.current_scope_level(), 1);
        assert!(symbol_table.in_function_scope());
        assert_eq!(symbol_table.current_function_name(), Some("test_func"));
        
        // Enter block scope within function
        let _block_scope_id = symbol_table.enter_scope(ScopeType::Block);
        assert_eq!(symbol_table.current_scope_level(), 2);
        assert!(symbol_table.in_function_scope()); // Still in function
        
        // Exit block scope
        let unused_symbols = symbol_table.exit_scope().unwrap();
        assert_eq!(symbol_table.current_scope_level(), 1);
        assert!(unused_symbols.is_empty());
        
        // Exit function scope
        let _unused_symbols = symbol_table.exit_scope().unwrap();
        assert_eq!(symbol_table.current_scope_level(), 0);
        assert!(!symbol_table.in_function_scope());
        assert_eq!(symbol_table.current_function_name(), None);
    }

    #[test]
    fn test_variable_definition_and_lookup() {
        let mut symbol_table = SymbolTable::new();
        
        // Define a variable in global scope
        assert!(symbol_table.define_variable(
            "global_var".to_string(),
            Type::Integer,
            create_test_location(),
            true
        ).is_ok());
        
        // Lookup the variable
        let symbol = symbol_table.lookup_symbol("global_var");
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().get_type(), Type::Integer);
        
        // Enter function scope and define local variable
        let _function_scope = symbol_table.enter_scope(ScopeType::Function("test".to_string()));
        assert!(symbol_table.define_variable(
            "local_var".to_string(),
            Type::String,
            create_test_location(),
            false
        ).is_ok());
        
        // Should be able to lookup both global and local variables
        assert!(symbol_table.lookup_symbol("global_var").is_some());
        assert!(symbol_table.lookup_symbol("local_var").is_some());
        
        // Exit function scope
        let _unused = symbol_table.exit_scope().unwrap();
        
        // Should still be able to lookup global variable
        assert!(symbol_table.lookup_symbol("global_var").is_some());
        // But not local variable
        assert!(symbol_table.lookup_symbol("local_var").is_none());
    }

    #[test]
    fn test_variable_shadowing() {
        let mut symbol_table = SymbolTable::new();
        
        // Define variable in global scope
        assert!(symbol_table.define_variable(
            "x".to_string(),
            Type::Integer,
            create_test_location(),
            true
        ).is_ok());
        
        // Verify it's Integer type
        assert_eq!(symbol_table.lookup_symbol("x").unwrap().get_type(), Type::Integer);
        
        // Enter function scope and shadow the variable
        let _function_scope = symbol_table.enter_scope(ScopeType::Function("test".to_string()));
        assert!(symbol_table.define_variable(
            "x".to_string(),
            Type::String,
            create_test_location(),
            false
        ).is_ok());
        
        // Should see the shadowed variable (String type)
        assert_eq!(symbol_table.lookup_symbol("x").unwrap().get_type(), Type::String);
        
        // Exit function scope
        let _unused = symbol_table.exit_scope().unwrap();
        
        // Should see the original variable again
        assert_eq!(symbol_table.lookup_symbol("x").unwrap().get_type(), Type::Integer);
    }

    #[test]
    fn test_redefinition_error() {
        let mut symbol_table = SymbolTable::new();
        
        // Define a variable
        assert!(symbol_table.define_variable(
            "duplicate".to_string(),
            Type::Integer,
            create_test_location(),
            true
        ).is_ok());
        
        // Try to redefine in same scope - should fail
        assert!(symbol_table.define_variable(
            "duplicate".to_string(),
            Type::String,
            create_test_location(),
            false
        ).is_err());
    }

    #[test]
    fn test_function_definition() {
        let mut symbol_table = SymbolTable::new();
        
        let parameters = vec![Type::Integer, Type::String];
        let return_type = Type::Boolean;
        
        // Define a function
        assert!(symbol_table.define_function(
            "test_func".to_string(),
            parameters.clone(),
            return_type.clone(),
            create_test_location(),
            Visibility::Public,
            vec![],
            false
        ).is_ok());
        
        // Lookup the function
        let symbol = symbol_table.lookup_symbol("test_func");
        assert!(symbol.is_some());
        assert!(symbol.unwrap().is_function());
        assert_eq!(symbol.unwrap().get_type(), return_type);
    }

    #[test]
    fn test_class_definition() {
        let mut symbol_table = SymbolTable::new();
        
        let mut fields = std::collections::HashMap::new();
        fields.insert("name".to_string(), Type::String);
        fields.insert("age".to_string(), Type::Integer);
        
        let mut methods = std::collections::HashMap::new();
        methods.insert("getName".to_string(), Type::String);
        methods.insert("getAge".to_string(), Type::Integer);
        
        // Define a class
        assert!(symbol_table.define_class(
            "Person".to_string(),
            fields,
            methods,
            None,
            create_test_location(),
            Visibility::Public
        ).is_ok());
        
        // Lookup the class
        let symbol = symbol_table.lookup_symbol("Person");
        assert!(symbol.is_some());
        assert!(symbol.unwrap().is_class());
        assert_eq!(symbol.unwrap().get_type(), Type::Object("Person".to_string()));
    }

    #[test]
    fn test_unused_symbol_detection() {
        let mut symbol_table = SymbolTable::new();
        
        // Enter function scope
        let _function_scope = symbol_table.enter_scope(ScopeType::Function("test".to_string()));
        
        // Define a variable that we'll use
        assert!(symbol_table.define_variable(
            "used_var".to_string(),
            Type::Integer,
            create_test_location(),
            true
        ).is_ok());
        
        // Define a variable that we won't use
        assert!(symbol_table.define_variable(
            "unused_var".to_string(),
            Type::String,
            create_test_location(),
            false
        ).is_ok());
        
        // Use the first variable
        let _type = symbol_table.lookup_and_use_symbol("used_var");
        assert!(_type.is_some());
        
        // Exit scope and check for unused symbols
        let unused_symbols = symbol_table.exit_scope().unwrap();
        assert_eq!(unused_symbols.len(), 1);
        assert_eq!(unused_symbols[0].name, "unused_var");
    }

    #[test]
    fn test_scope_type_checking() {
        let mut symbol_table = SymbolTable::new();
        
        // Initially not in any special scope
        assert!(!symbol_table.in_function_scope());
        assert!(!symbol_table.in_class_scope());
        assert!(!symbol_table.in_loop_scope());
        
        // Enter function scope
        let _function_scope = symbol_table.enter_scope(ScopeType::Function("test".to_string()));
        assert!(symbol_table.in_function_scope());
        assert!(!symbol_table.in_class_scope());
        assert!(!symbol_table.in_loop_scope());
        
        // Enter class scope within function (nested scope)
        let _class_scope = symbol_table.enter_scope(ScopeType::Class("TestClass".to_string()));
        assert!(symbol_table.in_function_scope());
        assert!(symbol_table.in_class_scope());
        assert!(!symbol_table.in_loop_scope());
        
        // Enter loop scope
        let _loop_scope = symbol_table.enter_scope(ScopeType::Loop);
        assert!(symbol_table.in_function_scope());
        assert!(symbol_table.in_class_scope());
        assert!(symbol_table.in_loop_scope());
        
        // Exit scopes one by one
        let _unused = symbol_table.exit_scope().unwrap(); // Exit loop
        assert!(symbol_table.in_function_scope());
        assert!(symbol_table.in_class_scope());
        assert!(!symbol_table.in_loop_scope());
        
        let _unused = symbol_table.exit_scope().unwrap(); // Exit class
        assert!(symbol_table.in_function_scope());
        assert!(!symbol_table.in_class_scope());
        assert!(!symbol_table.in_loop_scope());
        
        let _unused = symbol_table.exit_scope().unwrap(); // Exit function
        assert!(!symbol_table.in_function_scope());
        assert!(!symbol_table.in_class_scope());
        assert!(!symbol_table.in_loop_scope());
    }

    #[test]
    fn test_symbol_suggestions() {
        let mut symbol_table = SymbolTable::new();
        
        // Define some symbols
        assert!(symbol_table.define_variable("userName".to_string(), Type::String, create_test_location(), true).is_ok());
        assert!(symbol_table.define_variable("userAge".to_string(), Type::Integer, create_test_location(), true).is_ok());
        assert!(symbol_table.define_function(
            "userLogin".to_string(),
            vec![Type::String],
            Type::Boolean,
            create_test_location(),
            Visibility::Public,
            vec![],
            false
        ).is_ok());
        
        // Get all symbol names
        let names = symbol_table.get_all_symbol_names();
        assert!(names.contains(&"userName".to_string()));
        assert!(names.contains(&"userAge".to_string()));
        assert!(names.contains(&"userLogin".to_string()));
    }

    #[test]
    fn test_symbol_marking_as_used() {
        let mut symbol_table = SymbolTable::new();
        
        // Define a variable
        assert!(symbol_table.define_variable(
            "test_var".to_string(),
            Type::Integer,
            create_test_location(),
            true
        ).is_ok());
        
        // Initially not used
        let symbol = symbol_table.lookup_symbol("test_var").unwrap();
        assert!(!symbol.is_used);
        
        // Use the variable (lookup_and_use_symbol marks it as used)
        let _type = symbol_table.lookup_and_use_symbol("test_var");
        
        // Should now be marked as used
        let symbol = symbol_table.lookup_symbol("test_var").unwrap();
        assert!(symbol.is_used);
    }

    #[test]
    fn test_complex_scope_hierarchy() {
        let mut symbol_table = SymbolTable::new();
        
        // Global variable
        assert!(symbol_table.define_variable("global_var".to_string(), Type::Integer, create_test_location(), true).is_ok());
        
        // Enter class scope
        let _class_scope = symbol_table.enter_scope(ScopeType::Class("TestClass".to_string()));
        assert!(symbol_table.define_variable("class_var".to_string(), Type::String, create_test_location(), false).is_ok());
        
        // Enter method scope within class
        let _method_scope = symbol_table.enter_scope(ScopeType::Function("method".to_string()));
        assert!(symbol_table.define_variable("method_var".to_string(), Type::Boolean, create_test_location(), true).is_ok());
        
        // Enter block scope within method
        let _block_scope = symbol_table.enter_scope(ScopeType::Block);
        assert!(symbol_table.define_variable("block_var".to_string(), Type::Number, create_test_location(), false).is_ok());
        
        // Should be able to see all variables
        assert!(symbol_table.lookup_symbol("global_var").is_some());
        assert!(symbol_table.lookup_symbol("class_var").is_some());
        assert!(symbol_table.lookup_symbol("method_var").is_some());
        assert!(symbol_table.lookup_symbol("block_var").is_some());
        
        // Verify scope type checks
        assert!(symbol_table.in_class_scope());
        assert!(symbol_table.in_function_scope());
        assert_eq!(symbol_table.current_class_name(), Some("TestClass"));
        assert_eq!(symbol_table.current_function_name(), Some("method"));
        
        // Exit all scopes
        let _unused = symbol_table.exit_scope().unwrap(); // block
        let _unused = symbol_table.exit_scope().unwrap(); // method
        let _unused = symbol_table.exit_scope().unwrap(); // class
        
        // Should only see global variable now
        assert!(symbol_table.lookup_symbol("global_var").is_some());
        assert!(symbol_table.lookup_symbol("class_var").is_none());
        assert!(symbol_table.lookup_symbol("method_var").is_none());
        assert!(symbol_table.lookup_symbol("block_var").is_none());
    }
}