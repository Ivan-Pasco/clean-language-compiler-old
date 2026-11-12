//! SEM005 and plugin-injected modules (developer decision 2026-11-12).
//!
//! A module a plugin's `assemble` hook injects (`_inject_source_file`,
//! contracts/assemble.md §6.1) may call private functions of user modules —
//! frame.ui's `__plugin_frame_ui__/companion_routes.cln` calls each page's
//! private `load`. A file the user wrote calling the same private function
//! is still SEM005.

use std::path::Path;

use clean_language_compiler::ast::Statement;
use clean_language_compiler::plugins::plugin_abi::{AssembleInput, AssembleOutput, InjectedSource};
use clean_language_compiler::plugins::{
    FrameworkBlock, FrameworkPlugin, PluginRegistry, PluginResult,
};

/// Injects a module that calls the private `pageData()` of a user page.
struct RoutesPlugin;

impl FrameworkPlugin for RoutesPlugin {
    fn name(&self) -> &'static str {
        "test.routes"
    }

    fn handles(&self) -> &'static [&'static str] {
        &["__test_routes_block__"]
    }

    fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        Ok(Vec::new())
    }

    fn assemble(&self, _input: &AssembleInput) -> PluginResult<AssembleOutput> {
        let mut out = AssembleOutput::default();
        out.injected_sources.push(InjectedSource {
            virtual_path: "__plugin_test_routes__/routes.cln".to_string(),
            content: "start:\n\tprintl(pageData())\n".to_string(),
        });
        Ok(out)
    }
}

const MANIFEST: &str = "package: TestApp\n\
\tversion: \"1.0.0\"\n\
\ttarget: web\n\
\t\tplugins: [test.routes]\n\
\t\tentry: app/pages/home.cln\n";

/// Writes the project and compiles it with the mock plugin.
fn compile(root: &Path, entry_source: &str) -> Result<Vec<u8>, Vec<String>> {
    std::fs::write(root.join("main.cln"), MANIFEST).unwrap();
    let pages = root.join("app").join("pages");
    std::fs::create_dir_all(&pages).unwrap();
    std::fs::write(
        pages.join("page_data.cln"),
        "functions:\n\tstring pageData()\n\t\treturn \"data\"\n",
    )
    .unwrap();
    std::fs::write(pages.join("home.cln"), entry_source).unwrap();
    let registry = PluginRegistry::builder()
        .add(RoutesPlugin)
        .build()
        .expect("register mock plugin");
    let main = root.join("main.cln");
    clean_language_compiler::compile_with_plugins(
        MANIFEST,
        main.to_str().expect("utf-8 temp path"),
        &registry,
    )
    .map_err(|errors| errors.iter().map(|e| e.to_string()).collect())
}

#[test]
fn plugin_injected_module_may_call_private_user_function() {
    let tmp = tempfile::tempdir().unwrap();
    let result = compile(
        tmp.path(),
        "import:\n\tpage_data\n\nstart:\n\tprintl(\"home\")\n",
    );
    assert!(
        result.is_ok(),
        "the injected module's call must compile: {result:?}"
    );
}

#[test]
fn user_file_calling_the_same_private_function_is_sem005() {
    let tmp = tempfile::tempdir().unwrap();
    let errors = compile(
        tmp.path(),
        "import:\n\tpage_data\n\nstart:\n\tprintl(pageData())\n",
    )
    .expect_err("a user file may not call another file's private function");
    let joined = errors.join("\n");
    assert!(
        joined.contains("'pageData' is private and cannot be accessed from outside 'page_data'"),
        "{joined}"
    );
    // Only the user's call is reported, not the injected module's.
    assert_eq!(
        joined.matches("'pageData' is private").count(),
        1,
        "{joined}"
    );
}
