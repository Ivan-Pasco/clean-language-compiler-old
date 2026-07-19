//! STATE-A heap-probe hunt — verifies the `--emit-heap-probes` codegen flag.
//!
//! Contract acked to node-server in prompt 410a2312-836c-11f1-9d55-da25a95a496b:
//!
//!   * flag off (default): NO `_probe_ptr` / `_probe_ptr_dump` / `_probe_ptr_reset`
//!     imports appear in the emitted module. Zero production cost.
//!   * flag on: the three env imports are present, and after every WASM `call`
//!     to `__string_builder_append` and `__string_builder_finalize` the codegen
//!     emits `local.tee <scratch>` + `i32.const <callsite_id>` +
//!     `local.get <scratch>` + `call $_probe_ptr`, preserving the returned
//!     pointer on the stack for the caller.
//!   * the callsite list captured on the thread-local matches the count of
//!     probe calls in the module.
//!
//! Integration piece: compiles a tiny `.cln` with two interpolations
//! (`"a=\{x} b=\{y}"` → HIR rewrites to string_builder_new + N appends +
//! finalize). Walks the emitted WASM and asserts every append/finalize is
//! immediately followed by `LocalTee` + `I32Const` + `LocalGet` + `Call`
//! against the `_probe_ptr` import.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::MemoryTier;

// Uses the string-accumulator pattern the HIR builder rewrites into
// `string_builder_new` + N `string_builder_append` + one
// `string_builder_finalize` (see
// `HirBuilder::rewrite_string_accumulator_singleshot`). A single-expression
// `"a" + x + "b"` does NOT trigger the rewrite — it must be a `string acc = ""`
// followed by chained self-appends (`acc = acc + eK`).
const REPRO: &str = r#"start:
	integer x = 1
	integer y = 2
	string acc = ""
	acc = acc + "a="
	acc = acc + x.toString()
	acc = acc + " b="
	acc = acc + y.toString()
	print(acc)
"#;

/// Compile the repro with the given `emit_heap_probes` flag and return the
/// WASM bytes plus the sidecar callsites captured on the thread-local.
fn compile_with_flag(
    tmp: &TempDir,
    emit_heap_probes: bool,
) -> (
    Vec<u8>,
    Vec<clean_language_compiler::codegen::mir_codegen::ProbeCallsite>,
) {
    let main = tmp.path().join("repro.cln");
    fs::write(&main, REPRO).expect("write repro.cln");

    clean_language_compiler::set_emit_heap_probes_override(emit_heap_probes);
    // Clear any stale callsite list from a previous compile on this thread.
    let _ = clean_language_compiler::take_heap_probe_callsites();

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
        panic!("repro compile must succeed");
    });

    let callsites = clean_language_compiler::take_heap_probe_callsites();
    // Reset the override so subsequent tests on this thread aren't polluted.
    clean_language_compiler::set_emit_heap_probes_override(false);

    (wasm, callsites)
}

/// Collect (import_name, funcidx) for every function import in `wasm`, in
/// declaration order.
fn function_imports(wasm: &[u8]) -> Vec<(String, u32)> {
    let mut result = Vec::new();
    let mut next_idx: u32 = 0;
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let Ok(import) = import else { continue };
                if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                    result.push((import.name.to_string(), next_idx));
                    next_idx += 1;
                }
            }
        }
    }
    result
}

#[test]
fn flag_off_emits_no_probe_imports() {
    let tmp = TempDir::new().expect("tempdir");
    let (wasm, callsites) = compile_with_flag(&tmp, false);

    let imports = function_imports(&wasm);
    let probe_names = ["_probe_ptr", "_probe_ptr_dump", "_probe_ptr_reset"];
    for probe in probe_names {
        assert!(
            !imports.iter().any(|(name, _)| name == probe),
            "flag off must NOT emit `{}` import; found imports = {:?}",
            probe,
            imports.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
    }
    assert!(
        callsites.is_empty(),
        "flag off must not populate the callsite list, got {:?}",
        callsites
    );
}

#[test]
fn flag_on_emits_three_probe_imports() {
    let tmp = TempDir::new().expect("tempdir");
    let (wasm, callsites) = compile_with_flag(&tmp, true);

    let imports = function_imports(&wasm);
    for probe in ["_probe_ptr", "_probe_ptr_dump", "_probe_ptr_reset"] {
        assert!(
            imports.iter().any(|(name, _)| name == probe),
            "flag on must emit `{}` import; found imports = {:?}",
            probe,
            imports.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
    }

    // Every callsite recorded on the thread-local must be for one of the two
    // instrumented functions.
    assert!(
        !callsites.is_empty(),
        "flag on with string interpolations must record at least one callsite"
    );
    for cs in &callsites {
        assert!(
            cs.function == "string_builder_append" || cs.function == "string_builder_finalize",
            "unexpected callsite function: {:?}",
            cs
        );
        assert!(cs.id >= 1, "callsite ids start at 1: {:?}", cs);
    }
}

#[test]
fn flag_on_emits_probe_call_after_each_string_builder_call() {
    let tmp = TempDir::new().expect("tempdir");
    let (wasm, callsites) = compile_with_flag(&tmp, true);

    let imports = function_imports(&wasm);
    let probe_ptr_idx = imports
        .iter()
        .find(|(name, _)| name == "_probe_ptr")
        .map(|(_, idx)| *idx)
        .expect("_probe_ptr import must exist");

    // Locate the funcidx of every string_builder_* alias by scanning exports.
    // Exports are the language-facing bridge names — the internal aliases
    // (__string_builder_append / __string_builder_finalize) point at the
    // same indices as the codegen `add_function_alias` registrations.
    let mut sb_append_idx: Option<u32> = None;
    let mut sb_finalize_idx: Option<u32> = None;
    for payload in WasmParser::new(0).parse_all(&wasm) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader {
                let Ok(export) = export else { continue };
                if !matches!(export.kind, wasmparser::ExternalKind::Func) {
                    continue;
                }
                match export.name {
                    "__string_builder_append" | "string_builder_append" => {
                        sb_append_idx = Some(export.index);
                    }
                    "__string_builder_finalize" | "string_builder_finalize" => {
                        sb_finalize_idx = Some(export.index);
                    }
                    _ => {}
                }
            }
        }
    }
    let sb_append_idx = sb_append_idx.expect("__string_builder_append must be exported");
    let sb_finalize_idx = sb_finalize_idx.expect("__string_builder_finalize must be exported");

    // Walk every code body and count how many times we see a
    // `Call(sb_append|sb_finalize)` and how many of those are immediately
    // followed by the probe sequence:
    //   LocalTee, I32Const, LocalGet, Call(_probe_ptr)
    let mut sb_call_total: usize = 0;
    let mut probe_matched: usize = 0;

    for payload in WasmParser::new(0).parse_all(&wasm) {
        if let Ok(Payload::CodeSectionEntry(body)) = payload {
            let reader = body.get_operators_reader().expect("operators reader");
            let ops: Vec<Operator> = reader.into_iter().filter_map(|op| op.ok()).collect();

            for i in 0..ops.len() {
                let is_sb_call = matches!(
                    &ops[i],
                    Operator::Call { function_index }
                        if *function_index == sb_append_idx || *function_index == sb_finalize_idx
                );
                if !is_sb_call {
                    continue;
                }
                sb_call_total += 1;

                // Guard against tail-position matches running off the end.
                if i + 4 >= ops.len() {
                    continue;
                }
                let matches_probe = matches!(&ops[i + 1], Operator::LocalTee { .. })
                    && matches!(&ops[i + 2], Operator::I32Const { .. })
                    && matches!(&ops[i + 3], Operator::LocalGet { .. })
                    && matches!(
                        &ops[i + 4],
                        Operator::Call { function_index } if *function_index == probe_ptr_idx
                    );
                if matches_probe {
                    probe_matched += 1;
                }
            }
        }
    }

    assert!(
        sb_call_total > 0,
        "repro must contain at least one string_builder_* call, got 0"
    );
    assert_eq!(
        probe_matched, sb_call_total,
        "every string_builder_* call must be followed by the 4-op probe sequence; \
         got {} matched out of {} total sb calls",
        probe_matched, sb_call_total
    );
    assert_eq!(
        callsites.len(),
        sb_call_total,
        "sidecar callsite count ({}) must match instrumented sb call count ({})",
        callsites.len(),
        sb_call_total
    );
}
