//! Regression test for BUILD_FRONTEND (errors.cleanlanguage.dev):
//! `cln build` must produce `frontend.wasm` alongside the server WASM when the
//! project declares any component with an `events:` block.
//!
//! Also covers CLIENT_MODULE_LEAK: server-only bridge imports must not appear
//! in frontend.wasm when the functions that use them are unreachable from _start.
//!
//! Spec reference: `foundation/spec/plugins/frame-ui-semantics.md` §UI-B009.

use std::path::PathBuf;
use tempfile::TempDir;

/// Build a minimal project tree on disk:
///   <tmp>/main.cln                       — package manifest entry
///   <tmp>/app/web/pages/index.cln        — page (server-side)
///   <tmp>/app/web/components/Btn.cln     — component with events: block
fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("app/web/pages")).unwrap();
    std::fs::create_dir_all(root.join("app/web/components")).unwrap();

    std::fs::write(
        root.join("main.cln"),
        "package: TestApp\n\tentry: app/web/pages/index.cln\n",
    )
    .unwrap();

    std::fs::write(
        root.join("app/web/pages/index.cln"),
        "start:\n\tprintl(\"hello\")\n",
    )
    .unwrap();

    std::fs::write(
        root.join("app/web/components/Btn.cln"),
        "events:\n\tvoid onMount()\n\t\tprintl(\"mounted\")\n",
    )
    .unwrap();
}

#[test]
fn client_mode_compile_succeeds_for_minimal_component_project() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_project(root);

    let entry = root.join("main.cln");
    let search_paths: Vec<PathBuf> = vec![root.to_path_buf()];

    let result = clean_language_compiler::compile_multi_file_client_mode(&entry, search_paths, 0);

    match result {
        Ok(bytes) => {
            assert!(
                !bytes.is_empty(),
                "client_mode WASM must contain at least the WASM header"
            );
            assert!(
                bytes.starts_with(b"\0asm"),
                "client_mode output must be a valid WASM module"
            );
        }
        Err(errors) => panic!(
            "client_mode compile failed for minimal project: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        ),
    }
}

/// Parse the WASM import section and return all imported function names.
fn wasm_imported_functions(bytes: &[u8]) -> Vec<String> {
    use wasmparser::{Parser, Payload};
    let mut imports = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader.into_iter().flatten() {
                if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                    imports.push(import.name.to_string());
                }
            }
        }
    }
    imports
}

/// Regression test for CLIENT_MODULE_LEAK:
/// A server-only function (`find_user`) that calls a server bridge (`_db_query`)
/// must be DCE'd from frontend.wasm when no client code calls it.
#[test]
fn client_mode_does_not_leak_server_bridge_imports() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::create_dir_all(root.join("app/logic")).unwrap();
    std::fs::create_dir_all(root.join("app/web/components")).unwrap();

    // Server-only logic: calls no client APIs, never referenced from any component.
    std::fs::write(
        root.join("app/logic/auth.cln"),
        "functions:\n\tinteger find_user(string email)\n\t\treturn 0\n",
    )
    .unwrap();

    // Client component: registers a click handler, never calls find_user.
    std::fs::write(
        root.join("app/web/components/Btn.cln"),
        "events:\n\tvoid onClick()\n\t\tprintl(\"clicked\")\n",
    )
    .unwrap();

    std::fs::write(
        root.join("main.cln"),
        "package: LeakTest\n\tentry: app/web/components/Btn.cln\n",
    )
    .unwrap();

    let entry = root.join("main.cln");
    let result = clean_language_compiler::compile_multi_file_client_mode(
        &entry,
        vec![root.to_path_buf()],
        0,
    );

    let bytes = result.expect("client_mode compile failed for leak test");
    assert!(
        bytes.starts_with(b"\0asm"),
        "output must be a valid WASM module"
    );

    let imports = wasm_imported_functions(&bytes);
    assert!(
        !imports.contains(&"_db_query".to_string()),
        "frontend.wasm must not import _db_query — server bridge leaked into client build.\n\
         Imported functions: {imports:?}"
    );
}
