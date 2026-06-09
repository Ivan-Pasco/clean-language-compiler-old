//! Regression test for BUILD_FRONTEND (errors.cleanlanguage.dev):
//! `cln build` must produce `frontend.wasm` alongside the server WASM when the
//! project declares any component with an `events:` block.
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
