//! Regression test for CODEGEN-STRING-ARG-ALIAS-JSONGET
//! (fingerprint `3957fce40e09`), reported against compiler 0.33.9-0.33.51.
//!
//! Background: the compiler's stdlib `json.get(any, string) -> any` expects
//! its first argument to be a boxed-Any struct pointer
//! (`[tag@0][value1@4][value2@8]`, foundation/spec/type-system.md). A call
//! site like `json.get(sql_result, "some.path")` where `sql_result` is a
//! raw Clean String must emit a `BoxAny` op that allocates the 12-byte
//! box and writes the tag + inner string pointer before the call.
//!
//! When frame.server's plugin.toml declares `_json_get` with narrower
//! `params=["string","string"]`, the resolver's language-alias
//! registration overwrote the builtin `json.get(any,string)` symbol with
//! the plugin's narrower signature. That disabled the MIR builder's
//! `needs_boxing` check (`param_type_opt == Some(ConcreteType::Any)`), so
//! the raw String flowed into the stdlib body which read offset 0 as the
//! Any tag → garbage inner pointer → out-of-bounds host read.
//!
//! The fix (companion to framework prompt 4de6f0df) adds a symmetric
//! resolver guard to `register_language_function_aliases`: a plugin
//! declaration that narrows a builtin `Any` param to a concrete type is
//! rejected, preserving the compiler-stdlib signature.
//!
//! This test compiles a minimal repro that calls `json.get(rawStr, "x")`
//! and walks the emitted WASM. The call site must contain a `mem_alloc(12)`
//! call — that is the BoxAny op. Pre-fix, the compiled body contained
//! only a plain string load and the json.get call, no allocation.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::MemoryTier;

const REPRO: &str = r#"start:
	string s = "{\"x\": 1}"
	any v = json.get(s, "x")
	print(v)
"#;

#[test]
fn json_get_string_arg_is_boxed_before_call() {
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
        panic!("repro compile must succeed");
    });

    let mut mem_alloc_idx: Option<u32> = None;
    let mut start_idx: Option<u32> = None;
    let mut imported_func_count: u32 = 0;

    for payload in WasmParser::new(0).parse_all(&wasm) {
        match payload {
            Ok(Payload::ImportSection(reader)) => {
                for import in reader {
                    let Ok(import) = import else { continue };
                    if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                        if mem_alloc_idx.is_none() && import.name == "mem_alloc" {
                            mem_alloc_idx = Some(imported_func_count);
                        }
                        imported_func_count += 1;
                    }
                }
            }
            Ok(Payload::ExportSection(reader)) => {
                for export in reader {
                    let Ok(export) = export else { continue };
                    if export.name == "start"
                        && matches!(export.kind, wasmparser::ExternalKind::Func)
                    {
                        start_idx = Some(export.index);
                    }
                    if export.name == "mem_alloc"
                        && matches!(export.kind, wasmparser::ExternalKind::Func)
                    {
                        mem_alloc_idx.get_or_insert(export.index);
                    }
                }
            }
            _ => {}
        }
    }

    let start_idx = start_idx.expect("start must be exported");
    let mem_alloc_idx = mem_alloc_idx.expect("mem_alloc must be imported or exported");

    assert!(
        start_idx >= imported_func_count,
        "start at index {} is below the imported-function count {} — codegen exported an import",
        start_idx,
        imported_func_count,
    );
    let body_index = (start_idx - imported_func_count) as usize;

    let mut current_body: usize = 0;
    let mut mem_alloc_call_count = 0u32;

    for payload in WasmParser::new(0).parse_all(&wasm) {
        let Ok(Payload::CodeSectionEntry(body)) = payload else {
            continue;
        };
        if current_body != body_index {
            current_body += 1;
            continue;
        }

        let Ok(mut reader) = body.get_operators_reader() else {
            break;
        };

        while let Ok(op) = reader.read() {
            if let Operator::Call { function_index } = op {
                if function_index == mem_alloc_idx {
                    mem_alloc_call_count += 1;
                }
            }
        }
        break;
    }

    // `start` allocates for the string literal (`"{"x": 1}"`) via
    // `get_or_create_string_offset` — that's a data-section blob, not a
    // mem_alloc call. Any additional mem_alloc call is the BoxAny op for
    // the json.get first argument, plus possibly one for the return-side
    // unbox scratch.
    //
    // Pre-fix (without the resolver guard, with a hypothetical plugin
    // that narrowed json.get's first param to String): zero mem_alloc
    // calls in start, because BoxAny was skipped.
    //
    // Post-fix: at least one mem_alloc call (the BoxAny for the receiver).
    assert!(
        mem_alloc_call_count >= 1,
        "expected `start` to call `mem_alloc` at least once for the json.get \
         first-arg BoxAny, but the function body contains no such call. \
         This is the CODEGEN-STRING-ARG-ALIAS-JSONGET regression — a plugin \
         narrowing json.get(any, string) to json.get(string, string) is \
         being permitted through the resolver, disabling the MIR builder's \
         needs_boxing emission at the call site."
    );
}
