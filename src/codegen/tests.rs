//! Integration and unit tests for the code generator module.

// Make parent module items accessible
use super::*;
// Import ast module for tests if needed for constructing test cases

use crate::ast::{Expression, Value};

// StringPool has been removed as it was unused
// use wasmtime::{Engine, Module, Store, Instance, Val};
// use wasm_encoder::{Instruction, ConstExpr, GlobalType, ValType};

// Commented out tests for methods that don't exist in current CodeGenerator
/*
#[test]
fn test_add_memory() {
    let mut codegen = CodeGenerator::new();
    let result = codegen.add_memory(1, Some(10));
    assert!(result.is_ok(), "Failed to add memory: {:?}", result.err());
}

#[test]
fn test_add_global() {
    let mut codegen = CodeGenerator::new();
    let global_type = GlobalType {
        val_type: ValType::I32,
        mutable: true,
    };
    let init_expr = ConstExpr::i32_const(42);
    codegen.add_global("test_global", global_type, &init_expr);
    // No direct way to verify this worked, but it shouldn't panic
}
*/

// Removed test_string_pool as StringPool was removed

#[test]
fn test_memory_utils() {
    let heap_start = 1024; // Start heap at 1KB, leaving room for allocations
    let mut memory_utils = memory::MemoryUtils::new(heap_start);

    // Test string allocation
    let string_result = memory_utils.allocate_string("hello");
    assert!(
        string_result.is_ok(),
        "Failed to allocate string: {:?}",
        string_result.err()
    );
    let string_ptr = string_result.unwrap();
    assert!(
        string_ptr >= heap_start,
        "String pointer should be >= heap start"
    );

    // Test array allocation
    let array_values = vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)];
    let array_result = memory_utils.allocate_array(&array_values);
    assert!(
        array_result.is_ok(),
        "Failed to allocate array: {:?}",
        array_result.err()
    );
    let array_ptr = array_result.unwrap();
    assert!(
        array_ptr > string_ptr,
        "Array pointer should be after string pointer"
    );

    // Test matrix allocation
    let matrix_values = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let matrix_result = memory_utils.allocate_matrix(&matrix_values);
    assert!(
        matrix_result.is_ok(),
        "Failed to allocate matrix: {:?}",
        matrix_result.err()
    );
    let matrix_ptr = matrix_result.unwrap();
    assert!(
        matrix_ptr > array_ptr,
        "Matrix pointer should be after array pointer"
    );
}

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

    // Test is_string_type
    let string_expr = Expression::Literal(Value::String("test".to_string()));
    let int_expr = Expression::Literal(Value::Integer(42));
    assert!(type_manager.is_string_type(&string_expr));
    assert!(!type_manager.is_string_type(&int_expr));

    // Test type conversion
    assert!(type_manager.can_convert(WasmType::I32, WasmType::F64));
    assert!(type_manager.can_convert(WasmType::F64, WasmType::I32));
    assert!(type_manager.can_convert(WasmType::I32, WasmType::I32));
    assert!(!type_manager.can_convert(WasmType::I32, WasmType::F32));
}

#[test]
fn test_memory_operations() {
    let mut codegen = CodeGenerator::new_for_testing().unwrap();
    // Test string allocation
    let hello_str = "hello";
    let string_ptr_result = codegen.memory_utils.allocate_string(hello_str);
    assert!(
        string_ptr_result.is_ok(),
        "Failed to allocate string: {:?}",
        string_ptr_result.err()
    );
    let string_ptr = string_ptr_result.unwrap();
    assert!(string_ptr > 0, "String pointer should be positive");

    // Test retrieving the string back (might require runtime/mock memory access)
    // This part usually requires executing the generated WASM or mocking memory.
    // For a pure codegen test, we might just check that data segments were created.
    // let retrieved_string_result = codegen.get_string_from_memory(string_ptr as u64);
    // assert!(retrieved_string_result.is_ok(), "Failed to retrieve string: {:?}", retrieved_string_result.err());
    // assert_eq!(retrieved_string_result.unwrap(), hello_str);

    // Array and matrix allocation tested via integration tests
}
