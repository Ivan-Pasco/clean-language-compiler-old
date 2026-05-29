//! Regression test for E001: preamble functions must not be duplicated across
//! shared modules in a multi-file build.
//!
//! When a package has an entry file declaring `plugins: [frame.server]` AND
//! shared logic modules that do NOT declare any plugins, the plugin expander
//! was previously injecting preamble helpers (e.g. `redirect`, `json`, `error`)
//! into EVERY module.  After HIR merge the resolver saw each helper defined N
//! times and emitted "Function 'redirect' is already defined" (E001).
//!
//! The fix: `parse_module_to_hir` calls `expand_program_without_preambles` for
//! shared (non-entry) modules so preamble symbols appear exactly once.

use clean_language_compiler::compilation::{MultiFileCompiler, MultiFileCompilerConfig};

/// Build a minimal two-module project that mirrors the E001 repro:
/// - entry.cln: declares frame.server plugin (produces preamble helpers on expansion)
/// - shared.cln: plain helper functions, no plugins: declaration
///
/// The build must succeed without any "already defined" error.
#[test]
fn multi_module_with_server_plugin_no_duplicate_preamble() {
    use clean_language_compiler::plugins::WasmPluginLoader;
    use std::sync::Arc;

    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => {
            eprintln!("Skipping E001 regression: WasmPluginLoader unavailable");
            return;
        }
    };

    let registry = match loader.load_plugins(&["frame.server".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping E001 regression: frame.server not installed ({e})");
            return;
        }
    };

    // Entry module: declares the plugin and imports the shared module
    let entry_source =
        "plugins:\n\tframe.server\n\nimport:\n\tshared\n\nstart:\n\tprint(\"hello\")\n";

    // Shared logic module: no plugins: block — must NOT receive preamble helpers
    let shared_source = "functions:\n\tinteger double(integer n)\n\t\treturn n * 2\n";

    let sources = vec![
        ("entry".to_string(), entry_source.to_string()),
        ("shared".to_string(), shared_source.to_string()),
    ];

    let config = MultiFileCompilerConfig::default().with_plugin_registry(Arc::new(registry));
    let compiler = MultiFileCompiler::with_config(config);

    let result = compiler.build_from_sources("entry", &sources);

    match result {
        Ok(_) => {
            // Build succeeded — no duplicate preamble symbols
        }
        Err(errors) => {
            let messages: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
            panic!(
                "E001 regression: multi-module build with frame.server failed.\n\
                 Expected no duplicate preamble errors. Got:\n{}",
                messages.join("\n")
            );
        }
    }
}
