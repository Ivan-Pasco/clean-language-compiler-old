//! HYDRATE_AUTO end-to-end regression test.
//!
//! Reproduces the splice-not-happening symptom from
//! `compiler-frame-ui-v2-client-init-output-not-spliced.md` and asserts that
//! frame.ui's `emit_ui_client_init` output reaches `frontend.wasm`'s `_start`
//! body.
//!
//! Layout (built in a temp dir under /tmp), matching the current frame.ui
//! plugin contract (manifest declares `owns = ["app/ui/web", ...]`):
//!   main.cln                                — declares plugins + entry
//!   app/ui/web/pages/index.cln              — entry module (start: printl)
//!   app/ui/web/components/MyToolbar.cln     — component declaration
//!
//! Drives `compile_multi_file_client_mode` directly (the same path the
//! `[[artifacts]]` orchestrator uses to produce `frontend.wasm`) and asserts
//! the resulting WASM module contains a call to `MyToolbar` and its
//! `onMount` method somewhere reachable from `_start`. RED before the
//! snapshot-the-build-state fix, GREEN after.
//!
//! Plugin discovery: probes `$CLEAN_FRAME_UI_PATH` first, then
//! `$HOME/.cleen/plugins/frame.ui/<.active-version>/plugin.wasm`, then any
//! `~/.cleen/plugins/frame.ui/*/plugin.wasm` shipped on the box. The test
//! silently skips when no plugin is available so CI without `cleen` doesn't
//! fail spuriously.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use wasmparser::{Operator, Parser as WasmParser, Payload};

/// Resolve the frame.ui plugin WASM in a way that works on any developer
/// machine and in CI. Order of precedence:
///   1. `$CLEAN_FRAME_UI_PATH` (explicit override).
///   2. `$HOME/.cleen/plugins/frame.ui/<.active-version>/plugin.wasm`.
///   3. The newest-looking versioned plugin under
///      `$HOME/.cleen/plugins/frame.ui/*/plugin.wasm`.
fn resolve_frame_ui_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CLEAN_FRAME_UI_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let plugin_root = home.join(".cleen").join("plugins").join("frame.ui");
    if !plugin_root.exists() {
        return None;
    }

    if let Ok(active) = fs::read_to_string(plugin_root.join(".active-version")) {
        let active = active.trim();
        if !active.is_empty() {
            let candidate = plugin_root.join(active).join("plugin.wasm");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // Fall back to the lexically-greatest version directory that contains
    // a plugin.wasm. Lexical max is good enough for SemVer-like names with
    // equal segment counts; ties go to the longer string so 2.12.21 beats
    // 2.12.2.
    let mut best: Option<(String, PathBuf)> = None;
    if let Ok(entries) = fs::read_dir(&plugin_root) {
        for entry in entries.flatten() {
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };
            if name.starts_with('.') {
                continue;
            }
            let wasm = entry.path().join("plugin.wasm");
            if !wasm.exists() {
                continue;
            }
            match &best {
                Some((existing, _)) if existing.as_str() >= name.as_str() => {}
                _ => best = Some((name, wasm)),
            }
        }
    }
    best.map(|(_, path)| path)
}

fn frame_ui_path() -> Option<&'static PathBuf> {
    static CACHED: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHED.get_or_init(resolve_frame_ui_path).as_ref()
}

fn frame_ui_available() -> bool {
    frame_ui_path().is_some()
}

fn workspace_root() -> PathBuf {
    PathBuf::from("/tmp/clean-language-compiler-hydrate-auto-e2e")
}

fn setup_workspace() -> PathBuf {
    static SETUP: OnceLock<()> = OnceLock::new();
    let root = workspace_root();
    SETUP.get_or_init(|| {
        // Remove any prior run's artifacts to keep a clean repro.
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app/ui/web/pages")).expect("create pages dir");
        fs::create_dir_all(root.join("app/ui/web/components")).expect("create components dir");

        fs::write(
            root.join("main.cln"),
            "package: HydrateAutoE2E\n\ttarget: web\n\t\tplugins: [frame.ui]\n\t\tentry: app/ui/web/pages/index.cln\n",
        )
        .expect("write main.cln");
        fs::write(
            root.join("app/ui/web/pages/index.cln"),
            "start:\n\tprintl(\"hello\")\n",
        )
        .expect("write index.cln");
        // events: methods follow `foundation/spec/plugins/frame-ui.ebnf
        // §event_handler_function` — spec form `identifier(params):` with
        // no return type and a trailing colon, not the typed `void name()`
        // form. This is what frame.ui v2.12.4+ emits from
        // expand_component once normalize_handlers is a pass-through.
        fs::write(
            root.join("app/ui/web/components/MyToolbar.cln"),
            r##"component: tag="my-toolbar" client="on"
	events:
		onMount():
			integer r = _ui_on_event("#btn", "click", "do_thing")
		do_thing():
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

/// Flat set of every exported function name in the module. Used by the
/// per-handler-export assertion to check whether `do_thing`, `fmt_bold`, etc.
/// appear as bare-named exports the loader can dispatch to via
/// `instance.exports[handlerName]()`.
fn all_exported_function_names(wasm: &[u8]) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader {
                let export = export.expect("valid export");
                if matches!(export.kind, wasmparser::ExternalKind::Func) {
                    names.insert(export.name.to_string());
                }
            }
        }
    }
    names
}

#[test]
fn client_init_splice_reaches_start_body() {
    if !frame_ui_available() {
        eprintln!(
            "skipping: frame.ui plugin not found. Searched `$CLEAN_FRAME_UI_PATH` and \
             `$HOME/.cleen/plugins/frame.ui/<version>/plugin.wasm`. Install with \
             `cleen frame install latest` or point CLEAN_FRAME_UI_PATH at the wasm."
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

/// HYDRATE_AUTO Gap 2 — per-handler exports.
///
/// loader.js dispatches click events to component handlers via
/// `instance.exports[handlerName]()`. For that lookup to succeed, every method
/// declared in a component's `events:` block must appear in the WASM export
/// table under its *bare* name — `onMount`, `do_thing`, etc. — not the
/// qualified `mytoolbar.do_thing` form.
///
/// Today only `onMount` is bare-named-exported, because the `client_init`
/// slice emits `instance_my_toolbar.onMount()` as a direct call from `_start`.
/// Methods that are referenced only as string literals to `_ui_on_event`
/// (`do_thing` in the fixture) have no static call site reaching them from the
/// BFS roots in `collect_all_called_names_from_mir`, so they are dead-code
/// eliminated by the PLUGIN_OUTPUT_MARKER DCE pass in
/// `mir_codegen/mod.rs::generate` and never make it to the export table.
///
/// Closing this gap is a frame.ui responsibility: either emit a top-level
/// shim function for each event-block method (with the bare name, so the
/// compiler's "regular function" export rule applies), or have the lifecycle
/// slot output mark the event handlers as roots in some compiler-visible way.
/// Either path needs the bare-named export to land in `frontend.wasm`. This
/// test asserts that endpoint regardless of how frame.ui chooses to get there.
///
/// RED today: only `onMount` is exported; `do_thing` is missing.
/// GREEN once frame.ui ships per-handler bare-name exports.
#[test]
fn event_handlers_exported_by_bare_name() {
    if !frame_ui_available() {
        eprintln!(
            "skipping: frame.ui plugin not found. Searched `$CLEAN_FRAME_UI_PATH` and \
             `$HOME/.cleen/plugins/frame.ui/<version>/plugin.wasm`. Install with \
             `cleen frame install latest` or point CLEAN_FRAME_UI_PATH at the wasm."
        );
        return;
    }

    let root = setup_workspace();
    let entry = root.join("main.cln");

    let wasm = match clean_language_compiler::compile_multi_file_client_mode(
        &entry,
        vec![root.clone()],
        2,
    ) {
        Ok(bytes) => bytes,
        Err(errors) => {
            // The client-mode compile failure is itself a HYDRATE_AUTO symptom
            // (see `client_init_splice_reaches_start_body`). Report it so this
            // test does not silently mask it, but distinguish it from the
            // bare-name-export gap this test is specifically guarding.
            for e in &errors {
                eprintln!("compile error: {e}");
            }
            panic!(
                "client-mode compile of the HYDRATE_AUTO repro failed — {} errors. \
                 Resolve the splice-time compile error first; this test then becomes \
                 a clean signal for the bare-name-export gap.",
                errors.len()
            );
        }
    };

    let exports = all_exported_function_names(&wasm);

    // onMount is the baseline: it must be bare-named-exported because the
    // `client_init` splice calls it directly. If this fails, the splice
    // itself is broken (and `client_init_splice_reaches_start_body` should
    // already be red).
    assert!(
        exports.contains("onMount"),
        "BASELINE: `onMount` must be a bare-named export — the client_init splice \
         calls `instance_my_toolbar.onMount()` directly so it should always be \
         reachable from `_start` and survive the PLUGIN_OUTPUT_MARKER DCE pass. \
         Its absence means the splice has regressed. Exports present: {:?}",
        exports
            .iter()
            .filter(|n| !n.starts_with("__")
                && !n.contains('.')
                && n.as_str() != "memory"
                && n.as_str() != "__heap_ptr")
            .collect::<Vec<_>>()
    );

    // The actual gap: `do_thing` is named only as a string literal in
    // `_ui_on_event(..., "do_thing")`. The compiler has no static signal that
    // this string names an export target, so the method is DCE'd. frame.ui
    // must emit something that keeps `do_thing` alive AND surfaces it as a
    // bare-named export. The shape of that something is a frame.ui call —
    // a top-level shim function `void do_thing()` that delegates to the
    // instance is the cleanest, but anything that puts `do_thing` in the
    // export table satisfies this assertion.
    assert!(
        exports.contains("do_thing"),
        "GAP (HYDRATE_AUTO Gap 2): `do_thing` is referenced as the third argument \
         of `_ui_on_event(\"#btn\", \"click\", \"do_thing\")` but does NOT appear \
         as a bare-named export in `frontend.wasm`. loader.js will look it up via \
         `instance.exports[\"do_thing\"]()` and log `Event handler export do_thing \
         not found`. \n\
         \n\
         Root cause: the `do_thing` method on the component class is tagged with \
         PLUGIN_OUTPUT_MARKER (frame.ui's `expand_block` output), and the BFS \
         roots in `collect_all_called_names_from_mir` ([src/codegen/mir_codegen/\
         utilities.rs:1334](src/codegen/mir_codegen/utilities.rs#L1334)) reach \
         only those plugin-emitted functions transitively called from user code \
         or from the splice. String literals are not call sites, so `do_thing` \
         falls out of the reachable set and is DCE'd by [src/codegen/mir_codegen/\
         mod.rs:928](src/codegen/mir_codegen/mod.rs#L928).\n\
         \n\
         Fix: frame.ui-side. Either (a) emit a top-level shim function for each \
         events:-block method (bare name, delegates to the instance global — the \
         compiler's regular-function export rule then places it in the export \
         table by name), or (b) tag the component class's event-block methods \
         with PLUGIN_OUTPUT_V2_ROOT_MARKER so the BFS treats them as explicit \
         roots. The shim approach is cleaner because the bare name is then a \
         genuine top-level function and not a method needing instance dispatch.\n\
         \n\
         Reachable exports observed: {:?}",
        exports
            .iter()
            .filter(|n| !n.contains('.')
                && !n.starts_with("__")
                && n.as_str() != "memory"
                && n.as_str() != "__heap_ptr"
                && n.as_str() != "_start")
            .collect::<Vec<_>>()
    );
}
