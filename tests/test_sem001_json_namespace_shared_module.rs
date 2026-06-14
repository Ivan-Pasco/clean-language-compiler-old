//! Regression test: built-in `json` namespace must remain available in shared
//! modules when frame.server is loaded as a target plugin.
//!
//! frame.server declares a language alias `json -> _res_json` (the response
//! helper for returning JSON HTTP responses).  Before this fix,
//! `register_language_function_aliases` called `register_builtin_fn("json",…)`
//! which overwrote the `json` Namespace symbol in the global scope with a
//! Function symbol.  Any subsequent `json.get(…)` call in a shared module was
//! then misresolved as a method call on a variable and failed in the MIR
//! builder with "Undefined variable: json" (SEM001).
//!
//! The fix: skip registering a language alias when the bare name already exists
//! as a builtin Namespace in the symbol table.  Direct `json(data)` calls from
//! endpoint handlers continue to work through the call-site rewrite in the
//! resolver's Call handler.

use clean_language_compiler::compilation::{MultiFileCompiler, MultiFileCompilerConfig};

/// A shared helper module that uses `json.get` must compile successfully even
/// when the entry module's target includes frame.server.
#[test]
fn json_namespace_available_in_shared_module_with_frame_server() {
    use clean_language_compiler::plugins::WasmPluginLoader;
    use std::sync::Arc;

    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => {
            eprintln!("Skipping SEM001 regression: WasmPluginLoader unavailable");
            return;
        }
    };

    let registry = match loader
        .load_plugins(&["frame.server".to_string(), "frame.data".to_string()])
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping SEM001 regression: frame.server/frame.data not installed ({e})");
            return;
        }
    };

    // Shared logic module: uses json.get — no frame.server plugin
    let helper_source = "\
plugins:
\tframe.data

functions:
\tstring get_name(string input)
\t\treturn json.get(input, \"name\")
";

    // Entry module: frame.server — defines a load function
    let entry_source = "\
plugins:
\tframe.server

import:
\thelper

functions:
\tany load(Request request)
\t\treturn null
";

    let sources = vec![
        ("entry".to_string(), entry_source.to_string()),
        ("helper".to_string(), helper_source.to_string()),
    ];

    let config = MultiFileCompilerConfig {
        search_paths: vec![],
        opt_level: 0,
        debug: false,
        plugin_registry: Some(Arc::new(registry)),
        release_mode: false,
        client_mode: false,
    };

    let compiler = MultiFileCompiler::with_config(config);
    let result = compiler.build_from_sources("entry", &sources);

    match result {
        Ok(_) => {} // success — json.get resolved correctly
        Err(errors) => {
            for e in &errors {
                eprintln!("Error: {e}");
            }
            panic!("SEM001 regression: json namespace was shadowed by frame.server language alias");
        }
    }
}
