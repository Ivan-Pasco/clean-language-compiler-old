//! Verifies that the compiler emits WASM imports in **both** naming styles
//! for bridge functions that have `expand_strings = true`.
//!
//! Dual-naming behaviour (cln 0.30.120+):
//!   canonical:  `_db_query`  — raw import, strings expanded to (ptr, len) pairs
//!   dot-alias:  `db.query`   — LP-string import, same host callable, different ABI
//!
//! Both names MUST appear in the WASM import section. Hosts are required to
//! register both. Missing either causes a LinkError at WASM instantiation.
//!
//! See: foundation/platform-architecture/HOST_BRIDGE.md § Dual Naming

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
fn db_query_emits_both_underscore_and_dot_imports() {
    use clean_language_compiler::plugins::WasmPluginLoader;

    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => {
            eprintln!("Skipping test_dual_naming_imports: WasmPluginLoader unavailable");
            return;
        }
    };

    let registry = match loader.load_plugins(&["frame.data".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping test_dual_naming_imports: frame.data not installed ({e})");
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

    assert!(
        imports.contains(&"_db_query".to_string()),
        "WASM import section must contain `_db_query` (raw expand_strings import). Got: {:?}",
        imports
    );

    assert!(
        imports.contains(&"db.query".to_string()),
        "WASM import section must contain `db.query` (dot-notation LP-string import). Got: {:?}",
        imports
    );
}
