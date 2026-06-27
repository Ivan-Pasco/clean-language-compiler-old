//! Regression test for #6a754781d652 — SEM007 in cln 0.30.370/0.30.371.
//!
//! `<type> <name> = Model.find: where: ...` lowers in the frame.data plugin
//! to a `_db_query(...)` call whose SQL string literal contains the lowered
//! comparison (e.g. `... WHERE is_active = true ...`). The compiler's
//! `call_expand` reattach-binding-header step previously used
//! `first_line.find('=')` to decide whether the plugin had already emitted
//! its own `<type> <name> = ...` prefix. That heuristic spotted the `=`
//! inside the SQL string literal, decided the binding was already there,
//! and skipped re-prepending the user's `string result = ` header. The
//! `result` local was never declared and every downstream read of it
//! surfaced as `SEM007: Undefined variable 'result'` with location `:0:0`.
//!
//! The fix in `src/plugins/wasm_adapter.rs::starts_with_binding_header`
//! compares the plugin output to the actual `binding_header` token
//! sequence the compiler stripped, instead of searching for any `=`.
//!
//! This test reproduces the exact shape that triggered the regression
//! against the actual frame.data plugin (if installed) so the fix is
//! enforced end-to-end. The test is skipped — not failed — when
//! frame.data is unavailable so the rest of the suite still runs in
//! environments without plugins installed.

use clean_language_compiler::compilation::{MultiFileCompiler, MultiFileCompilerConfig};

#[test]
fn typed_find_with_where_clause_preserves_binding() {
    use clean_language_compiler::plugins::WasmPluginLoader;
    use std::sync::Arc;

    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => {
            eprintln!("Skipping #6a754781d652 regression: WasmPluginLoader unavailable");
            return;
        }
    };

    let registry = match loader.load_plugins(&["frame.data".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping #6a754781d652 regression: frame.data not installed ({e})");
            return;
        }
    };

    // The trigger is a typed find with a `where:` clause. The plugin's
    // lowered SQL contains `... WHERE is_active = true ...` — the `=`
    // inside that string literal previously fooled the reattach detector.
    let source = "\
plugins:
\tframe.data

data Language
\tid: integer pk auto
\tname: string
\tis_default: boolean
\tis_active: boolean

start:
\tstring result = Language.find:
\t\twhere:
\t\t\tis_active == true
\t\torder:
\t\t\tis_default desc
\t\t\tname asc
\tprint(result)
";

    let sources = vec![("entry".to_string(), source.to_string())];

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

    if let Err(errors) = result {
        let combined = errors
            .iter()
            .map(|e| format!("{e}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "#6a754781d652 regression: typed `Model.find:` with `where:` clause failed to compile.\n\
             The plugin output's first line contains `=` inside the lowered SQL string literal — \
             the reattach-binding-header check must not treat that as `already bound`.\n\
             Compiler errors:\n{}",
            combined
        );
    }
}
