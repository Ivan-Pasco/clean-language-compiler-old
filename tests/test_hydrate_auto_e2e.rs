//! HYDRATE_AUTO end-to-end regression test.
//!
//! Reproduces the splice-not-happening symptom from
//! `compiler-frame-ui-v2-client-init-output-not-spliced.md` and asserts that
//! frame.ui's `emit_ui_client_init` output reaches `frontend.wasm`'s `_start`
//! body.
//!
//! Layout (built in a temp dir under /tmp):
//!   main.cln                                — declares plugins + entry
//!   app/web/pages/index.cln                 — entry module (start: printl)
//!   app/web/components/MyToolbar.cln        — component declaration
//!
//! Drives `compile_multi_file_client_mode` directly (the same path the
//! `[[artifacts]]` orchestrator uses to produce `frontend.wasm`) and asserts
//! the resulting WASM module contains a call to `MyToolbar` and its
//! `onMount` method somewhere reachable from `_start`. RED before the
//! snapshot-the-build-state fix, GREEN after.
//!
//! Requires frame.ui 2.12.0+ installed at
//! ~/.cleen/plugins/frame.ui/2.12.0/plugin.wasm. The test silently skips if
//! the plugin isn't available so CI in environments without `cleen` doesn't
//! fail spuriously.

use std::fs;
use std::path::PathBuf;
use std::sync::Once;
use wasmparser::{Operator, Parser as WasmParser, Payload};

const FRAME_UI_PATH: &str = "/Users/earcandy/.cleen/plugins/frame.ui/2.12.0/plugin.wasm";

static SETUP: Once = Once::new();

fn frame_ui_available() -> bool {
    PathBuf::from(FRAME_UI_PATH).exists()
}

fn workspace_root() -> PathBuf {
    PathBuf::from("/tmp/clean-language-compiler-hydrate-auto-e2e")
}

fn setup_workspace() -> PathBuf {
    let root = workspace_root();
    SETUP.call_once(|| {
        // Remove any prior run's artifacts to keep a clean repro.
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app/web/pages")).expect("create pages dir");
        fs::create_dir_all(root.join("app/web/components")).expect("create components dir");

        fs::write(
            root.join("main.cln"),
            "package: HydrateAutoE2E\n\ttarget: web\n\t\tplugins: [frame.ui]\n\t\tentry: app/web/pages/index.cln\n",
        )
        .expect("write main.cln");
        fs::write(
            root.join("app/web/pages/index.cln"),
            "start:\n\tprintl(\"hello\")\n",
        )
        .expect("write index.cln");
        fs::write(
            root.join("app/web/components/MyToolbar.cln"),
            r##"component: tag="my-toolbar" client="on"
	events:
		void onMount()
			integer r = _ui_on_event("#btn", "click", "do_thing")
		void do_thing()
			integer r = _ui_add_class("#target", "active")
	html:
		<button id="btn">Click</button>
"##,
        )
        .expect("write MyToolbar.cln");
    });
    root
}

/// Walk every function body in the WASM and return the set of function
/// indices that are reachable from `_start` via `Operator::Call`. Used to
/// check whether the spliced `MyToolbar()` constructor / `onMount` call
/// appears in the synthetic client `_start` body or any function it calls
/// into transitively.
fn calls_reachable_from(wasm: &[u8], start_export: &str) -> std::collections::HashSet<u32> {
    let n_imports = count_func_imports(wasm);
    let start_idx = match find_exported_func_index(wasm, start_export) {
        Some(i) => i,
        None => return std::collections::HashSet::new(),
    };
    let mut bodies: Vec<Vec<u32>> = Vec::new();
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::CodeSectionEntry(body)) = payload {
            let mut callees = Vec::new();
            let mut reader = body.get_operators_reader().expect("operators");
            while let Ok(op) = reader.read() {
                if let Operator::Call { function_index } = op {
                    callees.push(function_index);
                }
            }
            bodies.push(callees);
        }
    }
    let body_of = |func_idx: u32| -> Option<&Vec<u32>> {
        if (func_idx as usize) < n_imports {
            None
        } else {
            bodies.get((func_idx as usize) - n_imports)
        }
    };
    let mut reachable = std::collections::HashSet::new();
    let mut queue = vec![start_idx];
    while let Some(f) = queue.pop() {
        if !reachable.insert(f) {
            continue;
        }
        if let Some(callees) = body_of(f) {
            for c in callees {
                queue.push(*c);
            }
        }
    }
    reachable
}

fn count_func_imports(wasm: &[u8]) -> usize {
    let mut count = 0;
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let import = import.expect("valid import");
                if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                    count += 1;
                }
            }
        }
    }
    count
}

fn find_exported_func_index(wasm: &[u8], name: &str) -> Option<u32> {
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader {
                let export = export.expect("valid export");
                if export.name == name && matches!(export.kind, wasmparser::ExternalKind::Func) {
                    return Some(export.index);
                }
            }
        }
    }
    None
}

/// Map from function index to a human-readable name from the export table,
/// when one exists. Used to check whether reachable-from-_start includes the
/// MyToolbar constructor and onMount method.
fn export_names(wasm: &[u8]) -> std::collections::HashMap<u32, String> {
    let mut map = std::collections::HashMap::new();
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader {
                let export = export.expect("valid export");
                if matches!(export.kind, wasmparser::ExternalKind::Func) {
                    map.insert(export.index, export.name.to_string());
                }
            }
        }
    }
    map
}

#[test]
fn client_init_splice_reaches_start_body() {
    if !frame_ui_available() {
        eprintln!(
            "skipping: frame.ui plugin not installed at {} — install with `cleen frame install latest`",
            FRAME_UI_PATH
        );
        return;
    }

    let root = setup_workspace();
    let entry = root.join("main.cln");

    let wasm =
        clean_language_compiler::compile_multi_file_client_mode(&entry, vec![root.clone()], 2)
            .unwrap_or_else(|errors| {
                for e in &errors {
                    eprintln!("compile error: {e}");
                }
                panic!(
                    "client-mode compile of the HYDRATE_AUTO repro failed — {} errors",
                    errors.len()
                );
            });

    assert!(
        wasm.len() > 1024,
        "client-mode WASM unexpectedly tiny ({} bytes); plugin may have failed to expand",
        wasm.len()
    );

    let reachable = calls_reachable_from(&wasm, "_start");
    let names = export_names(&wasm);

    // The spliced statements emitted by frame.ui's emit_ui_client_init for the
    // MyToolbar component are:
    //   MyToolbar instance_my_toolbar = MyToolbar()
    //   instance_my_toolbar.onMount()
    //
    // After splice + lower, the constructor call resolves to MyToolbar's
    // constructor export (named "MyToolbar" or "MyToolbar.constructor"
    // depending on emitter convention) and the onMount call resolves to the
    // method export.
    //
    // We accept any reachable function whose export name mentions "MyToolbar"
    // or "onMount" as evidence the splice took effect. The negative case
    // (the bug) reaches only the synthetic empty body — function 228 in the
    // shipped repro — and no MyToolbar-related export.
    let reachable_export_names: Vec<&str> = reachable
        .iter()
        .filter_map(|f| names.get(f).map(String::as_str))
        .collect();

    let mentions_toolbar = reachable_export_names
        .iter()
        .any(|n| n.contains("MyToolbar") || n.contains("onMount"));

    assert!(
        mentions_toolbar,
        "BUG (HYDRATE_AUTO): _start reaches {} functions but none are the spliced \
         MyToolbar instantiation / onMount call. Reachable exported names: {:?}. \
         The frame.ui `client_init` slot produced no statements because the BuildContext \
         passed to emit_ui_client_init had an empty `build_state` map — \
         PluginRegistry::invoke_lifecycle_slot never snapshots the shared build_state \
         Arc into the context before dispatching. Plumbing exists \
         (BuildContext::snapshot_build_state) but is only called from tests.",
        reachable.len(),
        reachable_export_names
    );
}
