//! Regression tests for the Import Minimality Rule
//! (platform-architecture/EXECUTION_LAYERS.md).
//!
//! A Clean program that does not reference Layer 3 server functions
//! (`_http_*`, `_req_*`, `_res_*`, `_session_*`, `_auth_*`) must NOT
//! emit those functions as WASM imports. Emitting them would force
//! every host (browser, CLI, embedded) to provide stubs for ~30 server
//! functions that will never be called.
//!
//! Corresponds to bug report `CODEGEN_UNUSED_IMPORTS` reported via the
//! MCP server against compiler v0.30.52.

use wasmparser::Parser as WasmParser;
use wasmparser::Payload;

/// Layer 3 server-only WASM import prefixes that a client-only program
/// must never emit.
const SERVER_PREFIXES: &[&str] = &["_http_", "_req_", "_res_", "_session_", "_auth_"];

fn compile_source(source: &str) -> Vec<u8> {
    clean_language_compiler::compile(source).expect("compilation should succeed")
}

fn list_imports(wasm_bytes: &[u8]) -> Vec<String> {
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
fn client_only_program_emits_no_server_imports() {
    // Minimal client-only program: just a print statement. Must not
    // cause any Layer 3 server imports to be emitted.
    let source = "start:\n\tprint(\"hello\")\n";
    let wasm = compile_source(source);
    let imports = list_imports(&wasm);

    let leaked: Vec<&String> = imports
        .iter()
        .filter(|name| {
            SERVER_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
        .collect();

    assert!(
        leaked.is_empty(),
        "client-only program leaked {} server import(s): {:?}",
        leaked.len(),
        leaked
    );
}

#[test]
fn client_only_program_has_reasonable_import_count() {
    // Regression guard: before the Import Minimality Rule fix, a minimal
    // client program emitted ~86 imports. After the fix the Layer 3
    // server imports (~24 of them) are stripped, bringing the count
    // down. This test asserts we stay below the pre-fix baseline so a
    // future change that silently reintroduces unconditional server
    // import emission will fail.
    let source = "start:\n\tprint(\"hello\")\n";
    let wasm = compile_source(source);
    let imports = list_imports(&wasm);

    assert!(
        imports.len() < 80,
        "expected < 80 imports for minimal client program, got {} \
         (Import Minimality Rule regression?)",
        imports.len()
    );
}
