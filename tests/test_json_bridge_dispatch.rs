//! Integration test for the JSON stdlib migration (Option B, [P2-cont-2a]).
//!
//! Verifies that the compiler's dispatch of the four spec'd JSON functions
//! (json.textToData / json.tryTextToData / json.dataToText /
//! json.prettyDataToText) is toggled correctly by the `--enable-json-bridge`
//! flag (exposed as `set_enable_json_bridge_override` on the library API).
//!
//! Semantics (0.33.137+, inverted from 0.33.135-0.33.136):
//!
//!   * Default (flag OFF) — the compiler uses the pre-0.33.135 pure-WASM
//!     parser in `src/stdlib/json_class.rs`. The four `_json_encode` /
//!     `_json_encode_pretty` / `_json_decode` host-bridge imports must NOT
//!     appear in the produced WASM.
//!   * `--enable-json-bridge` ON (opt-in) — calls to json.encode / json.decode
//!     resolve to the Layer 2 host bridges `_json_encode` and `_json_decode`.
//!     Those import names must appear in the produced WASM. The user opts in
//!     only when a compatible host (clean-server 2.9.6+ / clean-framework 3.x
//!     with the host implementations) is available; without one the produced
//!     WASM traps at instantiation.
//!
//! See:
//!   * `foundation/spec/stdlib-reference.md` §8
//!   * `foundation/spec/platform/runtime-abi/v1.toml` (json bridges)
//!   * `foundation/spec/platform/BOXED_ANY_ABI.md`
//!   * `foundation/docs/governance/JSON_MIGRATION_DELIVERY2.md`

use std::collections::HashSet;
use std::fs;
use tempfile::TempDir;
use wasmparser::{Parser as WasmParser, Payload};

use clean_language_compiler::MemoryTier;

/// Minimal Clean program exercising the two ends of the migration:
///   * `json.encode(x)`  → should route to `_json_encode` under `--enable-json-bridge`
///   * `json.decode(s)`  → should route to `_json_decode` under `--enable-json-bridge`
///
/// We call the aliases (`json.encode` / `json.decode`) rather than the
/// long-form names so the test also validates the alias resolution path
/// through `language_to_bridge_map` (GEN004 branch).
const REPRO: &str = r#"start:
	string encoded = json.encode("hello")
	printl(encoded)
	any decoded = json.decode("{\"a\": 1}")
	printl(json.dataToText(decoded))
"#;

fn compile(enable_json_bridge: bool) -> Vec<u8> {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(&main, REPRO).expect("write repro.cln");

    // Set the thread-local before compiling. The library's own
    // `compile_multi_file_with_options` sets this via a scoped RAII guard;
    // when using the older `compile_multi_file_with_memory_tier` entry point
    // (as the surrounding tests do for API stability) we must manage the flag
    // manually.
    clean_language_compiler::set_enable_json_bridge_override(enable_json_bridge);
    let result = clean_language_compiler::compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
        false,
    );
    clean_language_compiler::set_enable_json_bridge_override(false);

    let (wasm, _build_state) = result.unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!(
            "JSON dispatch test must compile successfully (enable_json_bridge={enable_json_bridge})"
        );
    });
    wasm
}

fn collect_imported_func_names(wasm: &[u8]) -> HashSet<String> {
    let mut names = HashSet::new();
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let Ok(import) = import else { continue };
                if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                    names.insert(import.name.to_string());
                }
            }
        }
    }
    names
}

/// Under the default (--enable-json-bridge NOT set), the pure-WASM parser
/// in `json_class.rs` handles everything and no `_json_encode` /
/// `_json_decode` / `_json_encode_pretty` bridge imports are emitted.
/// `_json_get` is unrelated to this migration and remains controlled by
/// plugin manifests, so we do NOT assert on it here.
#[test]
fn default_suppresses_json_bridge_imports() {
    let wasm = compile(false);
    let imports = collect_imported_func_names(&wasm);

    assert!(
        !imports.contains("_json_encode"),
        "default must NOT import `_json_encode`; got {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
    assert!(
        !imports.contains("_json_decode"),
        "default must NOT import `_json_decode`; got {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
    assert!(
        !imports.contains("_json_encode_pretty"),
        "default must NOT import `_json_encode_pretty`; got {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
}

/// Under `--enable-json-bridge` (opt-in), calls to `json.encode` and
/// `json.decode` must resolve to the Layer 2 host bridges `_json_encode`
/// and `_json_decode`. Those names must therefore appear in the produced
/// WASM's import section.
#[test]
fn bridge_opt_in_emits_json_bridge_imports() {
    let wasm = compile(true);
    let imports = collect_imported_func_names(&wasm);

    assert!(
        imports.contains("_json_encode"),
        "--enable-json-bridge must import `_json_encode`. Imports: {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
    assert!(
        imports.contains("_json_decode"),
        "--enable-json-bridge must import `_json_decode`. Imports: {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
}

/// Symmetric test: compiling the same program twice yields import sets that
/// differ specifically on the JSON bridge names. Catches regressions where
/// the flag is inadvertently ignored (both compilations produce identical
/// output) or where the default emits bridges unintentionally.
#[test]
fn flag_toggles_only_json_bridge_imports() {
    let default_wasm = compile(false);
    let bridge_wasm = compile(true);

    let default_imports = collect_imported_func_names(&default_wasm);
    let bridge_imports = collect_imported_func_names(&bridge_wasm);

    let only_in_default: HashSet<_> = default_imports.difference(&bridge_imports).collect();
    let only_in_bridge: HashSet<_> = bridge_imports.difference(&default_imports).collect();

    // Focus on the JSON-shaped imports only. The pure-WASM default path
    // additionally uses `string.concat` (its output serializer stitches JSON
    // fragments together) — that's an expected behavioral difference tied
    // to the flag, not a regression. What we care about is that the four
    // spec'd JSON bridges appear only under `--enable-json-bridge`.
    let json_only_in_default: Vec<&str> = only_in_default
        .iter()
        .map(|s| s.as_str())
        .filter(|n| n.starts_with("_json_"))
        .collect();
    let json_only_in_bridge: Vec<&str> = only_in_bridge
        .iter()
        .map(|s| s.as_str())
        .filter(|n| n.starts_with("_json_"))
        .collect();

    assert!(
        json_only_in_default.is_empty(),
        "default emitted a JSON bridge import that --enable-json-bridge doesn't: {json_only_in_default:?}"
    );
    assert!(
        json_only_in_bridge.contains(&"_json_encode"),
        "--enable-json-bridge did not add `_json_encode`; JSON diff was {json_only_in_bridge:?}"
    );
    assert!(
        json_only_in_bridge.contains(&"_json_decode"),
        "--enable-json-bridge did not add `_json_decode`; JSON diff was {json_only_in_bridge:?}"
    );
}
