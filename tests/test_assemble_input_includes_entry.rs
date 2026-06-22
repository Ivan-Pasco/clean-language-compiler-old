//! Regression test for COMPILER-ASSEMBLE-INPUT-OMITS-ENTRY-AND-NON-OWNED-FILES
//! (dashboard fingerprint `38323eb59c33`, reported against 0.30.326+).
//!
//! Background: `multi_file_compiler::build_from_file` populated
//! `AssembleInput.source_files` exclusively from `info.shared_folders`
//! (manifest `shared:` directives plus every loaded plugin's
//! `[paths].owns`). The entry .cln file and any module discovered through
//! Stage 2.5 expansion never appeared in the JSON handed to plugin
//! `assemble` hooks. Concrete user-visible effect: frame.ui's
//! page-companion detection registered zero routes for `app/pages/*.cln`
//! when that folder was outside the plugin's [paths].owns — the same
//! project worked correctly when the pages lived in the plugin-owned
//! `app/ui/web/pages/`. Reported on the framework dashboard as
//! PAGE-COMPANION-NO-ROUTE-GENERATED (fp b5f210c3c09d); root cause lives
//! here, in the compiler's source_files enumeration.
//!
//! Fix: after collecting shared-folder files, iterate every module in the
//! compilation unit and append any not already present (dedup by
//! canonical path). The AssembleInput contract — see
//! `src/plugins/plugin_abi.rs::AssembleInput::source_files`, "All source
//! files in the compilation unit" — now matches the implementation.
//!
//! This test installs a mock plugin whose `assemble` hook records every
//! `path` it sees, then runs `build_from_file` on a project whose entry
//! lives at `app/pages/hello.cln` — outside any shared folder or plugin
//! [paths].owns. The captured list must include that path. Pre-fix it
//! was empty.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clean_language_compiler::ast::Statement;
use clean_language_compiler::compilation::{MultiFileCompiler, MultiFileCompilerConfig};
use clean_language_compiler::plugins::plugin_abi::{AssembleInput, AssembleOutput};
use clean_language_compiler::plugins::{
    FrameworkBlock, FrameworkPlugin, PluginRegistry, PluginResult,
};

/// Mock plugin that records the `source_files` paths it receives on each
/// `assemble` call and then returns an empty (success) output. All other
/// trait methods take the defaults except `name`/`handles`/`expand` which
/// the trait requires.
struct RecorderPlugin {
    recorded: Arc<Mutex<Vec<String>>>,
}

impl FrameworkPlugin for RecorderPlugin {
    fn name(&self) -> &'static str {
        "test.recorder"
    }

    fn handles(&self) -> &'static [&'static str] {
        &["__test_recorder_block__"]
    }

    fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        Ok(Vec::new())
    }

    fn assemble(&self, input: &AssembleInput) -> PluginResult<AssembleOutput> {
        let mut paths = self.recorded.lock().expect("poisoned recorder lock");
        for f in &input.source_files {
            paths.push(f.path.clone());
        }
        Ok(AssembleOutput::default())
    }
}

#[test]
fn assemble_input_includes_entry_outside_shared_folders() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Manifest at the project root. The entry lives at app/pages/hello.cln —
    // outside any `shared:` directive (none declared) and outside the
    // mock plugin's [paths].owns (none — Rust mocks have no manifest).
    let manifest_source = concat!(
        "package: TestApp\n",
        "\tversion: \"1.0.0\"\n",
        "\ttarget: web\n",
        "\t\tplugins: [test.recorder]\n",
        "\t\tentry: app/pages/hello.cln\n",
    );
    std::fs::write(root.join("main.cln"), manifest_source).expect("write manifest");

    let pages_dir = root.join("app").join("pages");
    std::fs::create_dir_all(&pages_dir).expect("mkdir app/pages");

    // Entry .cln. The content is irrelevant to the assemble hook — only
    // the path needs to surface in source_files.
    std::fs::write(
        pages_dir.join("hello.cln"),
        "functions:\n\tany load(string req)\n\t\treturn \"hello\"\n",
    )
    .expect("write entry");

    let recorded: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let mut registry = PluginRegistry::new();
    #[allow(deprecated)] // pre-builder API is the simplest path for mocking
    registry
        .register(Arc::new(RecorderPlugin {
            recorded: Arc::clone(&recorded),
        }))
        .expect("register mock plugin");

    let config = MultiFileCompilerConfig::default().with_plugin_registry(Arc::new(registry));
    let compiler = MultiFileCompiler::with_config(config);

    // The build may downstream-fail (no real frame.ui = no html template
    // load helper, etc.). Either outcome is fine for THIS assertion —
    // the assemble hook fires before any of those downstream checks, so
    // we only care whether it ran with the right `source_files`.
    let _ = compiler.build_from_file(root.join("main.cln"));

    let entry_canonical: PathBuf = pages_dir
        .join("hello.cln")
        .canonicalize()
        .expect("canonicalize entry");

    let captured = recorded.lock().expect("poisoned recorder lock");

    assert!(
        captured.iter().any(|p| {
            let p = PathBuf::from(p);
            p.canonicalize()
                .map(|c| c == entry_canonical)
                .unwrap_or(false)
        }),
        "AssembleInput.source_files must include the entry file even when \
         it is outside every shared folder. \
         Expected to find `{}` in the captured list, got: {:?}",
        entry_canonical.display(),
        *captured
    );
}
