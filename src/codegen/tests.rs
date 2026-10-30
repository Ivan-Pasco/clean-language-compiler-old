//! Integration and unit tests for the code generator module.

// Make parent module items accessible
use super::*;

#[test]
fn test_type_manager() {
    let mut type_manager = type_manager::TypeManager::new();

    // Test adding a function type
    let params = vec![WasmType::I32, WasmType::I32];
    let return_types = vec![WasmType::I32];
    let result = type_manager.add_function_type(&params, &return_types);
    assert!(
        result.is_ok(),
        "Failed to add function type: {:?}",
        result.err()
    );

    // Check if the type was added
    let type_index = result.unwrap();
    assert_eq!(type_index, 0);
}
