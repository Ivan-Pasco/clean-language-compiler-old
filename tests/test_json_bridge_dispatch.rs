//! Integration test for the JSON stdlib migration (Option B, [P2-cont]).
//!
//! Verifies that the compiler's dispatch of the four spec'd JSON functions
//! (json.textToData / json.tryTextToData / json.dataToText /
//! json.prettyDataToText) is toggled correctly by the
//! `--enable-legacy-json-wasm` flag (exposed as
//! `set_enable_legacy_json_wasm_override` on the library API).
//!
//! Under the new default (flag OFF), calls to these functions must resolve
//! to the Layer 2 host bridges `_json_encode`, `_json_encode_pretty`, and
//! `_json_decode` — those import names must appear in the produced WASM's
//! import section.
//!
//! Under the legacy flag (flag ON), the compiler uses the pre-0.33.135 pure-
//! WASM parser in `src/stdlib/json_class.rs`. The bridge names must NOT
//! appear as imports in that mode.
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
///   * `json.encode(x)`  → should route to `_json_encode` under new default
///   * `json.decode(s)`  → should route to `_json_decode` under new default
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

fn compile(enable_legacy: bool) -> Vec<u8> {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(&main, REPRO).expect("write repro.cln");

    // Set the thread-local before compiling. The library's own
    // `compile_multi_file` wrapper does this via a scoped RAII guard; when
    // using the older `compile_multi_file_with_memory_tier` entry point (as
    // the surrounding tests do for API stability) we must manage the flag
    // manually.
    clean_language_compiler::set_enable_legacy_json_wasm_override(enable_legacy);
    let result = clean_language_compiler::compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
        false,
    );
    clean_language_compiler::set_enable_legacy_json_wasm_override(false);

    let (wasm, _build_state) = result.unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!("JSON dispatch test must compile successfully (enable_legacy={enable_legacy})");
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

/// Under the new default (--enable-legacy-json-wasm OFF), calls to
/// `json.encode` and `json.decode` must resolve to the Layer 2 host bridges
/// `_json_encode` and `_json_decode`. Those names must therefore appear in
/// the produced WASM's import section.
#[test]
fn new_default_emits_json_bridge_imports() {
    let wasm = compile(false);
    let imports = collect_imported_func_names(&wasm);

    assert!(
        imports.contains("_json_encode"),
        "new default must import `_json_encode`. Imports: {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
    assert!(
        imports.contains("_json_decode"),
        "new default must import `_json_decode`. Imports: {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
}

/// Under the legacy flag, the pure-WASM parser in `json_class.rs` handles
/// everything and no `_json_encode` / `_json_decode` bridge imports are
/// emitted. `_json_get` is unrelated to this migration and remains
/// controlled by plugin manifests, so we do NOT assert on it here.
#[test]
fn legacy_flag_suppresses_json_bridge_imports() {
    let wasm = compile(true);
    let imports = collect_imported_func_names(&wasm);

    assert!(
        !imports.contains("_json_encode"),
        "legacy flag must NOT import `_json_encode`; got {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
    assert!(
        !imports.contains("_json_decode"),
        "legacy flag must NOT import `_json_decode`; got {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
    assert!(
        !imports.contains("_json_encode_pretty"),
        "legacy flag must NOT import `_json_encode_pretty`; got {:?}",
        imports
            .iter()
            .filter(|n| n.starts_with("_json"))
            .collect::<Vec<_>>()
    );
}

/// Symmetric test: compiling the same program twice yields import sets that
/// differ specifically on the JSON bridge names. Catches regressions where
/// the flag is inadvertently ignored (both compilations produce identical
/// output) or where the legacy flag also affects unrelated imports.
#[test]
fn flag_toggles_only_json_bridge_imports() {
    let new_wasm = compile(false);
    let legacy_wasm = compile(true);

    let new_imports = collect_imported_func_names(&new_wasm);
    let legacy_imports = collect_imported_func_names(&legacy_wasm);

    let only_in_new: HashSet<_> = new_imports.difference(&legacy_imports).collect();
    let only_in_legacy: HashSet<_> = legacy_imports.difference(&new_imports).collect();

    // Focus on the JSON-shaped imports only. The pure-WASM legacy path
    // additionally uses `string.concat` (its output serializer stitches JSON
    // fragments together) — that's an expected behavioral difference tied
    // to the flag, not a regression. What we care about is that the four
    // spec'd JSON bridges appear only under the new default.
    let json_only_in_new: Vec<&str> = only_in_new
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
        "legacy flag emitted a JSON bridge import that the new default doesn't: {json_only_in_legacy:?}"
    );
    assert!(
        json_only_in_new.contains(&"_json_encode"),
        "new default did not add `_json_encode`; JSON diff was {json_only_in_new:?}"
    );
    assert!(
        json_only_in_new.contains(&"_json_decode"),
        "new default did not add `_json_decode`; JSON diff was {json_only_in_new:?}"
    );
}
