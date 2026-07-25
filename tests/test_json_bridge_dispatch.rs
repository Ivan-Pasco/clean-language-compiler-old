//! Integration test for the JSON stdlib migration (Delivery 2, [P4c]).
//!
//! Verifies that the compiler's dispatch of the four spec'd JSON functions
//! (json.textToData / json.tryTextToData / json.dataToText /
//! json.prettyDataToText) is toggled correctly by the JSON stdlib dispatch
//! selector (exposed as `set_enable_json_bridge_override` on the library API
//! and as `--enable-legacy-json-wasm` on the CLI).
//!
//! Semantics (0.33.139+):
//!
//!   * Default (bridge ON) — calls to json.encode / json.decode resolve to
//!     the Delivery-2 host bridges `_json_encode_v2` and `_json_decode_v2`.
//!     Those import names must appear in the produced WASM. Requires a
//!     Delivery-2-compatible host (clean-server 1.9.99+ / clean-framework
//!     2.12.187+); without one the produced WASM traps at instantiation.
//!   * `--enable-legacy-json-wasm` (opt-out) — the compiler falls back to
//!     the pre-Delivery-2 pure-WASM parser in `src/stdlib/json_class.rs`.
//!     The `_v2` host-bridge imports must NOT appear in the produced WASM.
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
///   * `json.encode(x)`  → should route to `_json_encode_v2` under the
///     bridge default
///   * `json.decode(s)`  → should route to `_json_decode_v2` under the
///     bridge default
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
    // Restore to the process-wide default (bridge ON) rather than leaking
    // an inversion into subsequent tests sharing this thread.
    clean_language_compiler::set_enable_json_bridge_override(true);

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

/// Under the default (bridge ON), calls to `json.encode` and `json.decode`
/// resolve to the Delivery-2 host bridges `_json_encode_v2` and
/// `_json_decode_v2`. Those names must appear in the produced WASM's
/// import section. The legacy `_json_encode` / `_json_decode` names must
/// NOT appear (they were dropped as the primary emission target in
/// [P2-cont-2b] and remain only as legacy entries in the registry for
/// backward-compat drift checking).
#[test]
fn default_emits_json_bridge_imports() {
    let wasm = compile(true);
    let imports = collect_imported_func_names(&wasm);

    assert!(
        imports.contains("_json_encode_v2"),
        "default must import `_json_encode_v2`. Imports: {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
    assert!(
        imports.contains("_json_decode_v2"),
        "default must import `_json_decode_v2`. Imports: {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
    assert!(
        !imports.contains("_json_encode"),
        "default must NOT import the legacy `_json_encode`. Imports: {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
    assert!(
        !imports.contains("_json_decode"),
        "default must NOT import the legacy `_json_decode`. Imports: {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
}

/// Under `--enable-legacy-json-wasm` (opt-out), the pure-WASM parser in
/// `json_class.rs` handles everything and no Delivery-2 `_v2` bridge
/// imports are emitted. The legacy `_json_encode` / `_json_decode` names
/// remain reserved but unused. `_json_get` is unrelated to this migration
/// and remains controlled by plugin manifests, so we do NOT assert on it.
#[test]
fn legacy_flag_suppresses_json_bridge_imports() {
    let wasm = compile(false);
    let imports = collect_imported_func_names(&wasm);

    for name in [
        "_json_encode_v2",
        "_json_encode_pretty_v2",
        "_json_decode_v2",
        // Legacy pre-Delivery-2 names must also not appear under the
        // opt-out path (they were never emitted post-0.33.137).
        "_json_encode",
        "_json_decode",
        "_json_encode_pretty",
    ] {
        assert!(
            !imports.contains(name),
            "--enable-legacy-json-wasm must NOT import `{name}`; got {:?}",
            imports
                .iter()
                .filter(|n| n.starts_with("_json"))
                .collect::<Vec<_>>()
        );
    }
}

/// Symmetric test: compiling the same program twice yields import sets that
/// differ specifically on the JSON bridge names. Catches regressions where
/// the flag is inadvertently ignored (both compilations produce identical
/// output) or where the opt-out path emits bridges unintentionally.
#[test]
fn flag_toggles_only_json_bridge_imports() {
    let bridge_wasm = compile(true);
    let legacy_wasm = compile(false);

    let bridge_imports = collect_imported_func_names(&bridge_wasm);
    let legacy_imports = collect_imported_func_names(&legacy_wasm);

    let only_in_bridge: HashSet<_> = bridge_imports.difference(&legacy_imports).collect();
    let only_in_legacy: HashSet<_> = legacy_imports.difference(&bridge_imports).collect();

    // Focus on the JSON-shaped imports only. The pure-WASM opt-out path
    // additionally uses `string.concat` (its output serializer stitches JSON
    // fragments together) — that's an expected behavioral difference tied
    // to the flag, not a regression. What we care about is that the four
    // spec'd JSON bridges appear only under the bridge default.
    let json_only_in_bridge: Vec<&str> = only_in_bridge
        .iter()
        .map(|s| s.as_str())
        .filter(|n| n.starts_with("_json_"))
        .collect();
    let json_only_in_legacy: Vec<&str> = only_in_legacy
        .iter()
        .map(|s| s.as_str())
        .filter(|n| n.starts_with("_json_"))
        .collect();

    assert!(
        json_only_in_legacy.is_empty(),
        "--enable-legacy-json-wasm emitted a JSON bridge import that the bridge default doesn't: {json_only_in_legacy:?}"
    );
    assert!(
        json_only_in_bridge.contains(&"_json_encode_v2"),
        "bridge default did not add `_json_encode_v2`; JSON diff was {json_only_in_bridge:?}"
    );
    assert!(
        json_only_in_bridge.contains(&"_json_decode_v2"),
        "bridge default did not add `_json_decode_v2`; JSON diff was {json_only_in_bridge:?}"
    );
}
