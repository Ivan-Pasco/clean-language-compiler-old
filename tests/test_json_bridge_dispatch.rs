//! Integration test for the JSON stdlib lowering (stdlib-reference §8).
//!
//! JSON parsing, serialization and path extraction are Layer 2 host bridges:
//! `json.encode` / `json.dataToText` → `_json_encode_v2`,
//! `json.decode` / `json.textToData` → `_json_decode_v2`, `json.get` →
//! `_json_get`. Those import names must appear in the produced WASM, and the
//! module must carry no JSON parser or serializer of its own. The
//! pre-Delivery-2 `--enable-legacy-json-wasm` opt-out and the in-WASM parser
//! behind it were removed in [P5]/[P9].
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

/// Minimal Clean program exercising the JSON entry points:
///   * `json.encode(x)`  → `_json_encode_v2`
///   * `json.decode(s)`  → `_json_decode_v2`
///   * `json.get(s, p)`  → `_json_get`
///
/// We call the aliases (`json.encode` / `json.decode`) rather than the
/// long-form names so the test also validates the alias resolution path
/// through `language_to_bridge_map` (GEN004 branch).
const REPRO: &str = r#"start:
	string encoded = json.encode("hello")
	printl(encoded)
	any decoded = json.decode("{\"a\": 1}")
	printl(json.dataToText(decoded))
	string name = json.get("{\"n\": \"x\"}", "n")
	printl(name)
"#;

fn compile() -> Vec<u8> {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(&main, REPRO).expect("write repro.cln");

    let (wasm, _build_state) = clean_language_compiler::compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
        false,
    )
    .unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!("JSON dispatch test must compile successfully");
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

/// Calls to `json.encode`, `json.decode` and `json.get` resolve to the host
/// bridges `_json_encode_v2`, `_json_decode_v2` and `_json_get`. Those names
/// must appear in the produced WASM's import section. The legacy
/// `_json_encode` / `_json_decode` names must
/// NOT appear (they were dropped as the primary emission target in
/// [P2-cont-2b] and remain only as legacy entries in the registry for
/// backward-compat drift checking).
#[test]
fn json_calls_emit_bridge_imports() {
    let wasm = compile();
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
        imports.contains("_json_get"),
        "json.get must import `_json_get`. Imports: {:?}",
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

/// The module carries no JSON parser or serializer: none of the helper
/// functions of the removed in-WASM implementation is exported or named.
#[test]
fn module_embeds_no_json_parser() {
    let wasm = compile();
    let mut names: Vec<String> = Vec::new();
    for payload in WasmParser::new(0).parse_all(&wasm) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader.into_iter().flatten() {
                names.push(export.name.to_string());
            }
        }
    }
    for removed in [
        "__json_parse_value",
        "__json_parse_string",
        "__json_parse_object",
        "__json_parse_array",
        "__json_skip_string",
        "__json_stringify_value",
        "__json_pretty_stringify_value",
        "__json_quote_string",
        "__json_get_path",
        "json.textToData",
        "json.dataToText",
        "json.get",
    ] {
        assert!(
            !names.iter().any(|n| n == removed),
            "the module must not define `{removed}` — JSON is a host bridge (stdlib-reference §8)"
        );
    }
}
