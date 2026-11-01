//! Regression test: HTML-first projects fail with
//! "unknown import: `env::_emit_function` has not been defined".
//!
//! `process_html` runs on the v1 plugin linker, which did not provide the
//! typed-emission bridge surface. Every plugin that also exports
//! `expand_block_typed` (frame.ui) imports that surface, so wasmtime refused
//! to instantiate it and every `.html` page entry failed with PLG000.
//!
//! The test builds a minimal HTML-first project against the installed
//! frame.ui plugin and skips when no plugin is available (CI without cleen).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn frame_ui_installed() -> bool {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    let root = home.join(".cleen").join("plugins").join("frame.ui");
    let Ok(active) = fs::read_to_string(root.join(".active-version")) else {
        return false;
    };
    root.join(active.trim()).join("plugin.wasm").exists()
}

fn unique_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "clean_process_html_typed_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ))
}

#[test]
fn html_page_entry_instantiates_typed_emission_plugin() {
    if !frame_ui_installed() {
        eprintln!("skipping: frame.ui plugin not installed");
        return;
    }
    let root = unique_dir();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("app/ui/web/pages")).expect("create pages dir");
    fs::write(
        root.join("main.cln"),
        "package: HtmlEntry\n\tversion: \"1.0.0\"\n\n\ttarget: web\n\t\tplugins: [frame.ui]\n\t\tentry: app/ui/web/pages/index.html\n",
    )
    .expect("write main.cln");
    fs::write(
        root.join("app/ui/web/pages/index.html"),
        "<!DOCTYPE html>\n<html>\n<body>\n\t<h1>Hello</h1>\n</body>\n</html>\n",
    )
    .expect("write index.html");

    let result = clean_language_compiler::compile_multi_file_with_memory_tier(
        root.join("main.cln"),
        vec![root.clone()],
        0,
        None,
        clean_language_compiler::MemoryTier::Standard,
        false,
    );
    let _ = fs::remove_dir_all(&root);

    let wasm = match result {
        Ok((wasm, _build_state)) => wasm,
        Err(errors) => {
            let combined = errors
                .iter()
                .map(|e| format!("{e}"))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                !combined.contains("unknown import"),
                "process_html could not instantiate the plugin:\n{combined}"
            );
            panic!("HTML-first project failed to compile:\n{combined}");
        }
    };
    // The page body must survive into the module's string data — a page that
    // compiles but renders nothing means the HTML was lost on the way in.
    assert!(
        wasm.windows(b"Hello".len()).any(|w| w == b"Hello"),
        "compiled page does not contain its HTML content"
    );
}
