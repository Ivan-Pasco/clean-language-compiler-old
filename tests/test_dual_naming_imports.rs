//! Bridge contract test: compiler must emit only canonical import names.
//!
//! The compiler must emit exactly one WASM import per bridge function: the
//! canonical `_namespace_fn` name defined in `function-registry.toml`.
//! Dot-notation aliases (e.g. `db.query`) are syntactic sugar resolved at
//! compile time — they must NOT appear as separate WASM imports.
//!
//! Dual emission of both `_db_query` and `db.query` is a bug, not a feature.
//! This test guards against that regression.

use wasmparser::{Parser as WasmParser, Payload};

fn list_import_names(wasm_bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in WasmParser::new(0).parse_all(wasm_bytes) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let import = import.expect("valid import");
                names.push(import.name.to_string());
            }
        }
    }
    names
}

#[test]
fn db_query_emits_canonical_name_only_no_dot_notation() {
    use clean_language_compiler::plugins::WasmPluginLoader;

    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => {
            eprintln!("Skipping bridge contract test: WasmPluginLoader unavailable");
            return;
        }
    };

    let registry = match loader.load_plugins(&["frame.data".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping bridge contract test: frame.data not installed ({e})");
            return;
        }
    };

    let source = "plugins:\n\tframe.data\n\nstart:\n\tstring result = db.query(\"SELECT 1\", \"\")\n\tprint(result)\n";

    let wasm = match clean_language_compiler::compile_with_plugins(source, "test.cln", &registry) {
        Ok(w) => w,
        Err(errors) => {
            panic!("Compilation failed: {:?}", errors);
        }
    };

    let imports = list_import_names(&wasm);

    // Canonical name must be present.
    assert!(
        imports.contains(&"_db_query".to_string()),
        "WASM import section must contain canonical `_db_query`. Got: {:?}",
        imports
    );

    // Dot-notation alias must NOT appear as a WASM import.
    assert!(
        !imports.contains(&"db.query".to_string()),
        "WASM import section must NOT contain dot-notation `db.query` — dual emission is a bug. Got: {:?}",
        imports
    );

    // No import name should appear more than once.
    let mut seen = std::collections::HashSet::new();
    for name in &imports {
        assert!(
            seen.insert(name.clone()),
            "Duplicate WASM import `{name}` — each bridge function must appear exactly once. All imports: {:?}",
            imports
        );
    }
}
