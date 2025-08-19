//! Unit tests for TypeManager functionality

use super::type_manager::TypeManager;
use crate::ast::Type;
use wasm_encoder::ValType;

#[test]
fn test_type_manager_creation() {
    let type_manager = TypeManager::new();
    assert_eq!(type_manager.get_default_type(), Type::Integer);
}

#[test]
fn test_ast_to_wasm_type_conversion() {
    let type_manager = TypeManager::new();
    
    assert_eq!(type_manager.ast_to_wasm_type(&Type::Integer), ValType::I32);
    assert_eq!(type_manager.ast_to_wasm_type(&Type::Number), ValType::F64);
    assert_eq!(type_manager.ast_to_wasm_type(&Type::Boolean), ValType::I32);
    assert_eq!(type_manager.ast_to_wasm_type(&Type::String), ValType::I32);
}

#[test]
fn test_wasm_to_ast_type_conversion() {
    let type_manager = TypeManager::new();
    
    assert_eq!(type_manager.wasm_to_ast_type(&ValType::I32), Type::Integer);
    assert_eq!(type_manager.wasm_to_ast_type(&ValType::F64), Type::Number);
    assert_eq!(type_manager.wasm_to_ast_type(&ValType::F32), Type::Number);
    assert_eq!(type_manager.wasm_to_ast_type(&ValType::I64), Type::Integer);
}

#[test]
fn test_list_type_conversion() {
    let type_manager = TypeManager::new();
    
    let list_type = Type::List(Box::new(Type::Integer));
    assert_eq!(type_manager.ast_to_wasm_type(&list_type), ValType::I32);
    
    let matrix_type = Type::Matrix(Box::new(Type::Number));
    assert_eq!(type_manager.ast_to_wasm_type(&matrix_type), ValType::I32);
}

#[test]
fn test_type_compatibility() {
    let type_manager = TypeManager::new();
    
    assert!(type_manager.is_compatible(&Type::Integer, &Type::Integer));
    assert!(type_manager.is_compatible(&Type::Number, &Type::Number));
    assert!(!type_manager.is_compatible(&Type::Integer, &Type::String));
    assert!(!type_manager.is_compatible(&Type::Boolean, &Type::Number));
}

#[test]
fn test_type_coercion() {
    let type_manager = TypeManager::new();
    
    // Integer should coerce to Number
    assert!(type_manager.can_coerce(&Type::Integer, &Type::Number));
    
    // Boolean should coerce to Integer
    assert!(type_manager.can_coerce(&Type::Boolean, &Type::Integer));
    
    // String should not coerce to Number
    assert!(!type_manager.can_coerce(&Type::String, &Type::Number));
}

#[test]
fn test_function_signature_validation() {
    let type_manager = TypeManager::new();
    
    let param_types = vec![Type::Integer, Type::String];
    let return_type = Type::Boolean;
    
    assert!(type_manager.validate_function_signature(&param_types, &return_type));
    
    // Test with complex types
    let complex_params = vec![
        Type::List(Box::new(Type::Integer)),
        Type::Matrix(Box::new(Type::Number))
    ];
    assert!(type_manager.validate_function_signature(&complex_params, &Type::Integer));
}

#[test] 
fn test_type_size_calculation() {
    let type_manager = TypeManager::new();
    
    assert_eq!(type_manager.get_type_size(&Type::Integer), 4);
    assert_eq!(type_manager.get_type_size(&Type::Number), 8);
    assert_eq!(type_manager.get_type_size(&Type::Boolean), 4);
    assert_eq!(type_manager.get_type_size(&Type::String), 4); // Reference size
    
    // Complex types should return reference size
    let list_type = Type::List(Box::new(Type::Integer));
    assert_eq!(type_manager.get_type_size(&list_type), 4);
}