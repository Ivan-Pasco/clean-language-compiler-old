//! Regression test for SILENT-DROP: an ORM expression block whose verb is not
//! handled by any installed plugin must fail at compile time with SEM001
//! rather than silently lowering to a void no-op.
//!
//! Before the fix:
//!   - Parser produced `Expression::OrmQuery { model, verb, content }`.
//!   - Plugin expander checked `handles_as_expression(...)`; for an unknown
//!     verb this returned false, so the OrmQuery was pushed back onto the AST
//!     unchanged.
//!   - The HIR builder's expression catch-all silently converted the OrmQuery
//!     to a `Value::Void` literal with no diagnostic.
//!
//! The user's `print(_)` of the discarded result then "worked" — the program
//! compiled and ran, but the verb invocation had been deleted from the binary.
//! Plugin-side typo guards never fired because the plugin was never invoked.
//!
//! After the fix the HIR builder rejects OrmQuery with a SEM001 diagnostic.

use clean_language_compiler::compilation::{MultiFileCompiler, MultiFileCompilerConfig};

#[test]
fn unknown_orm_verb_fails_with_sem001() {
    use clean_language_compiler::plugins::WasmPluginLoader;
    use std::sync::Arc;

    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => {
            eprintln!("Skipping SILENT-DROP regression: WasmPluginLoader unavailable");
            return;
        }
    };

    let registry = match loader.load_plugins(&["frame.data".to_string()]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping SILENT-DROP regression: frame.data not installed ({e})");
            return;
        }
    };

    // `fake_op_xyz` is not in any frame.data handles.expressions pattern and
    // not exported as expand_fake_op_xyz, so no plugin claims it.
    let source = "\
plugins:
\tframe.data

data User
\tid: integer pk auto
\tname: string

start:
\tinteger result = User.fake_op_xyz:
\t\twhere:
\t\t\tid == 1
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

    match result {
        Ok(_) => panic!(
            "SILENT-DROP regression: unknown ORM verb `User.fake_op_xyz:` \
             compiled successfully — it should fail with SEM001"
        ),
        Err(errors) => {
            let has_sem001 = errors.iter().any(|e| {
                let msg = format!("{e}");
                msg.contains("SEM001") && msg.contains("fake_op_xyz")
            });
            assert!(
                has_sem001,
                "Expected SEM001 mentioning fake_op_xyz; got: {:?}",
                errors.iter().map(|e| format!("{e}")).collect::<Vec<_>>()
            );
        }
    }
}
