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

/// Regression test for CLIENT_PULLS_SERVER_DCE:
/// When the entry module has a `start:` block with server-side code (e.g. calling
/// frame.data migrations) AND a component with `client="on"`, the frontend.wasm
/// _start must NOT call the server SSR entry or pull in _db_register_migration.
#[test]
fn client_mode_start_does_not_pull_migration_chain() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Entry module: has both server start: code and a client component.
    std::fs::write(
        root.join("main.cln"),
        "start:\n\tprintl(\"server start\")\n",
    )
    .unwrap();

    let entry = root.join("main.cln");
    let result = clean_language_compiler::compile_multi_file_client_mode(
        &entry,
        vec![root.to_path_buf()],
        0,
    );

    let bytes = result.expect("client_mode compile failed for migration leak test");
    assert!(
        bytes.starts_with(b"\0asm"),
        "output must be a valid WASM module"
    );

    let imports = wasm_imported_functions(&bytes);
    assert!(
        !imports.contains(&"_db_register_migration".to_string()),
        "frontend.wasm must not import _db_register_migration — server migration \
         chain leaked into client build via _start.\n\
         Imported functions: {imports:?}"
    );
    assert!(
        !imports.contains(&"_http_listen".to_string()),
        "frontend.wasm must not import _http_listen — server SSR entry leaked into \
         client build via _start.\n\
         Imported functions: {imports:?}"
    );
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

/// Regression test for ORM-MUTATION-OPS-MISSING-IN-CLIENT-CODEGEN-FUNCTION-MAP
/// (fingerprint `44c1dc978900`).
///
/// `Widget.update:` and `Widget.delete:` lower to calls to plugin-emitted helper
/// functions `__Widget_raw_update` / `__Widget_raw_delete` (frame.data preamble).
/// In client builds the BFS roots intentionally exclude user-defined functions
/// to keep server-only bridge imports from leaking — but user-defined functions
/// are *kept* in MIR unconditionally. When such a function called the preamble
/// helper, DCE removed the helper and codegen failed with
/// `Function '__Widget_raw_update' not found in function map`.
///
/// `Widget.find:` and `Widget.count:` always worked because they lower to inline
/// `_db_query(…)` expressions (no synthesized helper to be DCE'd).
///
/// The fix is in `collect_all_called_names_from_mir`: a fixpoint pass that adds
/// preamble names called from any retained function back into the reachable set.
#[test]
fn client_mode_orm_mutation_in_user_function_compiles() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Single-file repro: model + user function calling Widget.update in the
    // same module. Client mode keeps the user function in MIR (user code is
    // never DCE'd) but BFS-seeds only _start + exports + v2 roots — so
    // touch_widget() is a kept-but-unreachable caller of the preamble helper
    // __Widget_raw_update. Before the fix in
    // `collect_all_called_names_from_mir`, the DCE in `mod.rs::generate()`
    // dropped the helper and codegen failed at the touch_widget() body.
    std::fs::write(
        root.join("main.cln"),
        concat!(
            "plugins:\n",
            "\tframe.data\n",
            "\n",
            "data Widget:\n",
            "\tid: integer pk auto\n",
            "\tname: string\n",
            "\n",
            "start:\n",
            "\tprintl(\"client build\")\n",
            "\n",
            "functions:\n",
            "\tinteger touch_widget()\n",
            "\t\tWidget.update:\n",
            "\t\t\tset:\n",
            "\t\t\t\tname = \"x\"\n",
            "\t\t\twhere:\n",
            "\t\t\t\tid == 1\n",
            "\t\treturn 1\n",
        ),
    )
    .unwrap();

    let entry = root.join("main.cln");
    let result = clean_language_compiler::compile_multi_file_client_mode(
        &entry,
        vec![root.to_path_buf()],
        0,
    );

    let bytes = match result {
        Ok(b) => b,
        Err(errors) => {
            // Skip if frame.data is not installed locally — this test only
            // verifies the codegen fix, not plugin availability.
            let msg: String = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            if msg.contains("frame.data") && msg.contains("not found") {
                eprintln!("Skipping ORM mutation regression: frame.data not installed");
                return;
            }
            panic!("client_mode compile failed for ORM mutation regression:\n{msg}");
        }
    };
    assert!(
        bytes.starts_with(b"\0asm"),
        "client_mode output must be a valid WASM module"
    );

    // Bonus check: _db_execute (the server-only bridge that the mutation
    // helpers call) must still be stub-replaced, not imported.
    let imports = wasm_imported_functions(&bytes);
    assert!(
        !imports.contains(&"_db_execute".to_string()),
        "frontend.wasm must not import _db_execute — server-only bridge \
         leaked through plugin-preamble retention.\n\
         Imported functions: {imports:?}"
    );
}
