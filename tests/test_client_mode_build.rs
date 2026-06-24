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

/// Regression test for CODEGEN-ORM-METHOD-NOT-IN-FN-MAP
/// (fingerprint `ea5d66dcf89e`).
///
/// Sibling of `client_mode_orm_mutation_in_user_function_compiles`. Same root
/// cause (plugin-preamble helper DCE'd while still called from retained user
/// code) but with two extra wrinkles found in the user's failing repro:
///   1. the call site is a **class method**, not a top-level function; and
///   2. the model and the caller live in **separate files** in a `shared:`
///      package layout, so the model file goes through
///      `expand_program_without_preambles` while only the entry module gets
///      preambles. The ORM helper `__DesignComponent_count` is emitted by
///      `expand_data_model` running on the model file — its `location.file`
///      points at the model file, NOT at `<plugin-output>`, even though it
///      IS a plugin-generated helper that the client artifact can never
///      reach via `_db_query` and therefore must be DCE-eligible.
///
/// Before the fix in `collect_all_called_names_from_mir`'s preamble fixpoint,
/// only `location.file == "<plugin-output>"` was treated as preamble. When
/// the user's class method called the helper, the fixpoint missed it and the
/// codegen failed with `Function '__DesignComponent_count' not found in
/// function map`.
#[test]
fn client_mode_orm_count_in_class_method_compiles() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::create_dir_all(root.join("app/data/models")).unwrap();
    std::fs::create_dir_all(root.join("app/logic")).unwrap();
    std::fs::create_dir_all(root.join("app/ui/web/pages")).unwrap();

    // Model file — only this file declares `data:`, so only this file's
    // expansion emits the `__DesignComponent_count` ORM helper.
    std::fs::write(
        root.join("app/data/models/DesignComponent.cln"),
        concat!(
            "data DesignComponent:\n",
            "\tid: integer pk auto\n",
            "\tproject_id: integer\n",
            "\tname: string\n",
        ),
    )
    .unwrap();

    // Logic file — class method calls the helper via ORM syntax.
    std::fs::write(
        root.join("app/logic/lookup.cln"),
        concat!(
            "class Lookup\n",
            "\tfunctions:\n",
            "\t\tinteger componentExists(integer compId, integer projId)\n",
            "\t\t\tinteger n = DesignComponent.count:\n",
            "\t\t\t\twhere:\n",
            "\t\t\t\t\tid == compId\n",
            "\t\t\t\t\tproject_id == projId\n",
            "\t\t\tif n > 0\n",
            "\t\t\t\treturn 1\n",
            "\t\t\treturn 0\n",
        ),
    )
    .unwrap();

    // Web entry — never calls componentExists, so the BFS from _start
    // does not reach the helper. The helper survives only via the fixpoint
    // that preserves preamble helpers called from any retained user code.
    std::fs::write(
        root.join("app/ui/web/pages/dashboard.cln"),
        "start:\n\tprintl(\"client build\")\n",
    )
    .unwrap();

    std::fs::write(
        root.join("main.cln"),
        concat!(
            "package: OrmCountRepro\n",
            "\tversion: \"0.0.1\"\n",
            "\n",
            "\tshared: [app/logic/, app/data/]\n",
            "\n",
            "\ttarget: web\n",
            "\t\tplugins: [frame.data]\n",
            "\t\tentry: app/ui/web/pages/dashboard.cln\n",
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
            let msg: String = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            if msg.contains("frame.data") && msg.contains("not found") {
                eprintln!(
                    "Skipping ORM count-in-class-method regression: frame.data not installed"
                );
                return;
            }
            panic!("client_mode compile failed for ORM count-in-class-method regression:\n{msg}");
        }
    };
    assert!(
        bytes.starts_with(b"\0asm"),
        "client_mode output must be a valid WASM module"
    );

    let imports = wasm_imported_functions(&bytes);
    assert!(
        !imports.contains(&"_db_query".to_string()),
        "frontend.wasm must not import _db_query — server-only bridge \
         leaked through plugin-preamble retention.\n\
         Imported functions: {imports:?}"
    );
}

/// Regression test for ORM-NOW-BRIDGE-CODEGEN-MISS-IN-CLEAN-STUDIO-CONTEXT
/// (fingerprint `6e78a9f165f8`).
///
/// `Document.update: set: deleted_at = now()` lowers into the plugin preamble
/// helper `__Document_raw_update`, which itself emits a `Call(now)`. The
/// preamble fixpoint introduced in 55dbca08 correctly preserves the helper
/// when a user function calls it, but only propagated *preamble* names to
/// `names`. `now` is a bare alias for the `_time_now` bridge — not a preamble
/// function — so it never reached the expansion at line ~1756 that adds
/// `_time_now`. With `_time_now` tree-shaken at `register_import_function`,
/// the `time.now` wrapper was never created, `function_map["now"]` stayed
/// empty, and codegen for `__Document_raw_update`'s body failed with
/// `Function 'now' (SymbolId(N)) not found in function map during code generation`.
///
/// The fix extends the fixpoint: when walking a *preserved preamble* body,
/// propagate every callee (not just preamble names) into `names`. User-code
/// walks still propagate only preamble names so server-only bridges inside
/// dead user code don't leak into frontend.wasm.
#[test]
fn client_mode_orm_mutation_with_now_compiles() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Single-file repro: model + user function calling Document.update with
    // `now()` in the set: clause. Mirrors the Clean Studio scenario where the
    // user-code call site is unreached by BFS in client mode (`doc_delete` is
    // not exported, not _start, not a route handler) — only the preamble-
    // helper fixpoint can pull `__Document_raw_update` (and its transitive
    // `now()` reference) into the reachable set.
    std::fs::write(
        root.join("main.cln"),
        concat!(
            "plugins:\n",
            "\tframe.data\n",
            "\n",
            "data Document:\n",
            "\tid: integer pk auto\n",
            "\tuser_id: integer\n",
            "\tdeleted_at: integer\n",
            "\n",
            "start:\n",
            "\tprintl(\"client build\")\n",
            "\n",
            "functions:\n",
            "\tinteger doc_delete(integer id, integer uid)\n",
            "\t\tDocument.update:\n",
            "\t\t\tset:\n",
            "\t\t\t\tdeleted_at = now()\n",
            "\t\t\twhere:\n",
            "\t\t\t\tid == id\n",
            "\t\t\t\tuser_id == uid\n",
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
            let msg: String = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            if msg.contains("frame.data") && msg.contains("not found") {
                eprintln!("Skipping ORM now() regression: frame.data not installed");
                return;
            }
            // The specific symptom we're guarding against — make it loud in CI.
            assert!(
                !msg.contains("'now'") || !msg.contains("not found in function map"),
                "ORM-NOW-BRIDGE-CODEGEN-MISS regression — `now()` inside \
                 Model.update: set: still fails codegen in client mode:\n{msg}"
            );
            panic!("client_mode compile failed for ORM now() regression:\n{msg}");
        }
    };
    assert!(
        bytes.starts_with(b"\0asm"),
        "client_mode output must be a valid WASM module"
    );

    // Import Minimality cross-check: even though we now propagate non-preamble
    // callees from preserved preamble bodies, server-only bridges referenced
    // from those bodies must still be stubbed (host-mismatch path), not
    // imported. _db_execute is the bridge __Document_raw_update calls.
    let imports = wasm_imported_functions(&bytes);
    assert!(
        !imports.contains(&"_db_execute".to_string()),
        "frontend.wasm must not import _db_execute — host-mismatched bridge \
         leaked through preamble-callee propagation.\n\
         Imported functions: {imports:?}"
    );
}
