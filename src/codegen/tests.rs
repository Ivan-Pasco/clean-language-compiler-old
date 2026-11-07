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

/// A host function is imported once: a bridge registered through
/// `register_import_function` after a built-in registration emitted the same
/// field through `emit_import` reuses that import's index (and vice versa),
/// instead of adding a second identical import.
#[test]
fn test_import_is_emitted_once_per_host_function() {
    let mut gen = CodeGenerator::new();
    let ty = gen
        .add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))
        .unwrap();
    assert!(gen.emit_import("env", "http_get", wasm_encoder::EntityType::Function(ty)));
    let first = gen.function_count;
    gen.function_count += 1;
    let again = gen
        .register_import_function(
            "env",
            "http_get",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )
        .unwrap();
    assert_eq!(
        again,
        Some(first),
        "the second registration reuses the import"
    );

    let code = gen
        .register_import_function("env", "http_get_response_code", &[], Some(WasmType::I32))
        .unwrap()
        .expect("no reachability filter is set");
    let code_ty = gen.add_function_type(&[], Some(WasmType::I32)).unwrap();
    assert!(
        !gen.emit_import(
            "env",
            "http_get_response_code",
            wasm_encoder::EntityType::Function(code_ty)
        ),
        "an import registered first is not emitted again"
    );
    assert_eq!(gen.get_function_index("http_get_response_code"), Some(code));
    assert_eq!(gen.import_section.len(), 2);
}
