//! Regression test for the return-form ORM verb dispatch bug.
//!
//! `return Model.<verb>:` was being rejected with SEM001 even when the verb
//! was registered by a plugin (e.g. frame.data's `count`, `exists`, `first`).
//! The expander only had arms for `VariableDecl` and `Expression`-statement
//! forms of `OrmQuery`; the `Return` form fell through, so the `OrmQuery`
//! reached the HIR builder and tripped the SEM001 catch-all.
//!
//! Fix: route `Statement::Return { value: Some(OrmQuery {..}) }` through
//! the same plugin dispatch as `VariableDecl`, then re-wrap the resulting
//! single expression as `Return { value: Some(expr) }`.

use clean_language_compiler::compilation::{MultiFileCompiler, MultiFileCompilerConfig};

#[test]
fn return_orm_verb_does_not_emit_sem001() {
    use clean_language_compiler::plugins::WasmPluginLoader;
    use std::sync::Arc;

    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => {
            eprintln!("Skipping: WasmPluginLoader unavailable");
            return;
        }
    };

    let registry = match loader.load_plugins(&["frame.data".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping: frame.data not installed ({e})");
            return;
        }
    };

    // `count` is a real frame.data verb. `return Model.count:` should compile.
    let source = "\
plugins:
\tframe.data

data Project
\tid: integer pk auto
\tuser_id: integer

start:
\tP p = P()
\tinteger n = p.count_them(1)
\tprint(n)

class P
\tfunctions:
\t\tinteger count_them(integer uid)
\t\t\treturn Project.count:
\t\t\t\twhere:
\t\t\t\t\tuser_id == uid
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

    match result {
        Ok(_) => {}
        Err(errors) => {
            let messages: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
            let has_sem001 = messages
                .iter()
                .any(|m| m.contains("SEM001") && m.contains("count"));
            assert!(
                !has_sem001,
                "Regression: `return Project.count:` raised SEM001 even though `count` is a \
                 registered frame.data verb. Errors: {:?}",
                messages
            );
            panic!(
                "Compilation failed for `return Project.count:` (non-SEM001). \
                 Errors: {:?}",
                messages
            );
        }
    }
}
