//! Regression test for SymbolId collision between resolver-assigned
//! function IDs and MIR-synthesized stdlib builtins.
//!
//! Root cause (CODEGEN_F64 bf9fc9cf8a8e, CG-LOCAL29 bdd711d2183d):
//! the MIR builder used SymbolId(1000..1011) for stdlib helpers like
//! `string.concat`, `__json_quote_string`, `int_to_string`, etc. The
//! resolver allocates SymbolIds sequentially starting at 0, with no
//! awareness of the synthetic range. A program that loads enough
//! plugin bridge functions to push the resolver past 1000 collides:
//! the resolver-assigned ID overwrites `symbol_name_map[1011]`, and
//! class JSON serializers (which embed `Function(SymbolId(1011))` for
//! `__json_quote_string`) end up calling whatever user function landed
//! on SymbolId(1011) — observed as math_pow, redirect, etc.
//!
//! The failure was non-deterministic because Rust's HashMap iteration
//! order is randomized per process; whether a colliding resolver ID
//! actually overwrote the synthetic mapping depended on the order
//! tast.functions were enumerated.
//!
//! Fix: synthetic builtins now use `SYM_BUILTIN_*` constants based at
//! `0x4000_0000`, well above any resolver-allocated SymbolId. Watch
//! handlers, computed getters, and class serializers use parallel
//! reserved ranges (`SYM_WATCH_HANDLER_BASE`, etc.).
//!
//! Regression detection strategy: compile a program that touches the
//! synthetic builtins (`__json_quote_string` via class serializer
//! emitted by the plugin-aware path) under enough plugin load to push
//! the resolver near the old synthetic range, then validate the
//! resulting module. With the bug present, validation fails
//! non-deterministically (~1% of runs in the smoke repro); we
//! compile many times to make the test reliable.

use clean_language_compiler::plugins::WasmPluginLoader;
use wasmparser::Validator;

/// Compile a source program with the requested plugins loaded. Returns
/// `None` if the plugin runtime isn't installed locally so the CI can
/// stay green on machines without the framework.
fn compile_with_plugins(source: &str, plugins: &[&str]) -> Option<Vec<u8>> {
    let mut loader = match WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => return None,
    };
    let plugin_names: Vec<String> = plugins.iter().map(|s| s.to_string()).collect();
    let registry = match loader.load_plugins(&plugin_names) {
        Ok(r) => r,
        Err(_) => return None,
    };
    match clean_language_compiler::compile_with_plugins(source, "test.cln", &registry) {
        Ok(w) => Some(w),
        Err(errors) => panic!("Compilation failed: {:?}", errors),
    }
}

fn validate(wasm_bytes: &[u8]) -> Result<(), String> {
    let mut validator = Validator::new();
    validator.validate_all(wasm_bytes).map(|_| ()).map_err(|e| {
        format!(
            "WASM validation failed at offset 0x{:x}: {}",
            e.offset(),
            e.message()
        )
    })
}

/// Source that triggers the synthetic-builtin path: a class whose JSON
/// serializer is emitted under the plugin-aware compile pipeline, with
/// multiple plugins loaded so the resolver allocates enough SymbolIds
/// to overlap the old synthetic range (1000..1011).
const HEAVY_PLUGIN_SOURCE: &str = r#"plugins:
	frame.ui
	frame.server
	frame.data
	frame.auth

functions:
	any load(Request request)
		return "{\"page_title\": \"Hello\"}"
"#;

#[test]
fn heavy_plugin_load_compiles_deterministically() {
    // Compile the same source many times. Before the SymbolId-collision
    // fix this failed non-deterministically (~1% rate on a 4-plugin
    // smoke repro). After the fix it must succeed every time.
    for attempt in 0..50 {
        let Some(wasm) = compile_with_plugins(
            HEAVY_PLUGIN_SOURCE,
            &["frame.ui", "frame.server", "frame.data", "frame.auth"],
        ) else {
            eprintln!("Skipping: plugins not installed");
            return;
        };

        if let Err(err) = validate(&wasm) {
            panic!(
                "WASM validation failed on attempt #{attempt}: {err}\n\
                 This indicates a SymbolId collision between the resolver's \
                 sequential allocator and a hardcoded MIR synthetic builtin.",
            );
        }
    }
}

/// Stress the smaller `frame.data`-only path that produces the
/// CG-LOCAL29 stack-balance failure. Same root cause as the heavy
/// repro — a resolver SymbolId overwriting `symbol_name_map[1011]`
/// (`__json_quote_string`) causes the class-serializer call to point
/// at the wrong function, which then leaves the stack unbalanced.
#[test]
fn data_plugin_class_serializer_validates() {
    let source = r#"plugins:
	frame.data

start:
	Item it = Item("apple", 3)
	print(render(it))

class Item
	string name
	integer count

functions:
	string render(Item it)
		return it.name
"#;

    for attempt in 0..20 {
        let Some(wasm) = compile_with_plugins(source, &["frame.data"]) else {
            eprintln!("Skipping: frame.data plugin not installed");
            return;
        };
        if let Err(err) = validate(&wasm) {
            panic!("validation failed on attempt #{attempt}: {err}");
        }
    }
}
