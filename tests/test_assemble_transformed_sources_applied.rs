//! Regression test for CLN-0-30-345-PLUGIN-BUILD-EMITS-UNDEFINED-FN-REF-CAUSING-SEM007
//! (dashboard fingerprint `d275648c7452`, reported against 0.30.345).
//!
//! Background: `multi_file_compiler::build_from_file`'s "Pass 3" applied
//! the plugin's `transformed_sources` only to files iterated through
//! `shared_files` — the set of modules walked under
//! `info.shared_folders` (manifest `shared:` plus every loaded plugin's
//! `[paths].owns`). When the entry file lived outside every shared
//! folder (e.g. `app/pages/index.cln` while frame.ui only declares
//! `app/ui/*` ownership), the matching transformation in
//! `AssembleOutput.transformed_sources` was silently dropped. The
//! injected route module still called the plugin-renamed companion
//! (`pages_<name>_load`), but the entry module retained the
//! user-written `load(Request)`, so the semantic analyzer reported
//! `error[SEM007]: Function 'pages_<name>_load' not found`.
//!
//! This was the second half of the assemble enumeration mismatch
//! introduced when COMPILER-ASSEMBLE-INPUT-OMITS-ENTRY-AND-NON-OWNED-FILES
//! (test_assemble_input_includes_entry.rs) closed the input side: the
//! plugin saw the entry in `source_files` and emitted a
//! `transformed_sources` entry for it, but the apply pass didn't follow
//! through.
//!
//! Fix: after the shared-folder pass, walk the leftover `transformed_map`
//! keys and apply each transformation to any module already in the
//! compilation unit whose canonical path matches.
//!
//! Test strategy: register a mock plugin whose `assemble` returns a
//! `transformed_sources` entry renaming `string load(string)` to
//! `string pages_hello_load(string)` in the entry file, then call
//! `build_from_file` and inspect the resulting `CompilationUnit`'s
//! entry-module source directly. Pre-fix the entry module's source
//! still contained the un-renamed `string load(`; post-fix it contains
//! the renamed `string pages_hello_load(`.

use std::path::PathBuf;
use std::sync::Arc;

use clean_language_compiler::ast::Statement;
use clean_language_compiler::compilation::{MultiFileCompiler, MultiFileCompilerConfig};
use clean_language_compiler::plugins::plugin_abi::{
    AssembleInput, AssembleOutput, TransformedSource,
};
use clean_language_compiler::plugins::{
    FrameworkBlock, FrameworkPlugin, PluginRegistry, PluginResult,
};

/// Mock plugin that renames the user-written `string load(string)` in
/// the entry file to `string pages_hello_load(string)` via a
/// transformed_sources entry. If the compiler applies the
/// transformation correctly, the entry module's source in the
/// returned CompilationUnit reflects the rename; otherwise the
/// original source survives untouched.
struct PageRenamePlugin;

impl FrameworkPlugin for PageRenamePlugin {
    fn name(&self) -> &'static str {
        "test.page_rename"
    }

    fn handles(&self) -> &'static [&'static str] {
        &["__page_rename_test_block__"]
    }

    fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        Ok(Vec::new())
    }

    fn assemble(&self, input: &AssembleInput) -> PluginResult<AssembleOutput> {
        let entry = input
            .source_files
            .iter()
            .find(|f| f.path.ends_with("hello.cln"));

        let mut transformed_sources = Vec::new();

        if let Some(entry) = entry {
            let renamed = entry
                .content
                .replace("string load(", "string pages_hello_load(");

            transformed_sources.push(TransformedSource {
                path: entry.path.clone(),
                content: renamed,
            });
        }

        Ok(AssembleOutput {
            injected_sources: Vec::new(),
            transformed_sources,
        })
    }
}

#[test]
fn assemble_transformed_sources_applied_to_entry_outside_shared_folders() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Manifest at the project root. The entry lives at app/pages/hello.cln —
    // outside any `shared:` directive (none declared) and outside the
    // mock plugin's [paths].owns (none — Rust mocks have no manifest).
    let manifest_source = concat!(
        "package: TestApp\n",
        "\tversion: \"1.0.0\"\n",
        "\ttarget: web\n",
        "\t\tplugins: [test.page_rename]\n",
        "\t\tentry: app/pages/hello.cln\n",
    );
    std::fs::write(root.join("main.cln"), manifest_source).expect("write manifest");

    let pages_dir = root.join("app").join("pages");
    std::fs::create_dir_all(&pages_dir).expect("mkdir app/pages");

    let original_source = "functions:\n\tstring load(string req)\n\t\treturn \"hello\"\n";
    let entry_file = pages_dir.join("hello.cln");
    std::fs::write(&entry_file, original_source).expect("write entry");

    let mut registry = PluginRegistry::new();
    #[allow(deprecated)] // pre-builder API is the simplest path for mocking
    registry
        .register(Arc::new(PageRenamePlugin))
        .expect("register mock plugin");

    let config = MultiFileCompilerConfig::default().with_plugin_registry(Arc::new(registry));
    let compiler = MultiFileCompiler::with_config(config);

    // build_from_file walks discovery (including the assemble pass)
    // and returns the populated CompilationUnit. The downstream
    // parse/typecheck/codegen outcome depends on host-bridge
    // availability this mock environment can't provide, but the field
    // we care about is `module.source`, which reflects assemble's
    // transformed_sources by the time the unit is handed back.
    let unit = compiler
        .build_from_file(root.join("main.cln"))
        .expect("multi-file discovery + assemble succeeds");

    let entry_canonical: PathBuf = entry_file.canonicalize().expect("canonicalize entry");
    let module_id = unit
        .module_id_for_path(&entry_canonical)
        .expect("entry module is registered");
    let module = unit
        .get_module(module_id)
        .expect("entry module is retrievable");

    assert!(
        module.source.contains("pages_hello_load"),
        "AssembleOutput.transformed_sources for the entry file must be \
         applied to its compilation-unit module even when the entry \
         lives outside every shared folder. The mock plugin returned a \
         transformed source renaming `load` to `pages_hello_load`, but \
         the unit module's source did not pick it up. \
         Entry file: `{}`. Module source:\n{}",
        entry_canonical.display(),
        module.source
    );

    // Belt-and-braces: the pre-rename `string load(` should NOT
    // survive when the rename is applied. If it does, pass 3 is still
    // ignoring transformed_sources for non-shared modules.
    assert!(
        !module.source.contains("string load("),
        "Original `string load(` should have been renamed by the \
         applied transformation. \
         Entry file: `{}`. Module source:\n{}",
        entry_canonical.display(),
        module.source
    );
}
