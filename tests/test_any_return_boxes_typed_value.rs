//! Regression test for CODEGEN-ANY-RETURN-STRING-NOT-BOXED
//! (fingerprint `4cb4ac3cf4e7838d68affafcaee8d496f0e21f9c925ef9bb97b6ca2baf04bbce`,
//! reported against compiler 0.30.316).
//!
//! Background: a function declared with return type `any` whose body
//! returns a typed value (string literal, string variable, integer,
//! number, boolean, or function call result) was emitting a raw value
//! return instead of a tagged-box return. Callers reading the result as
//! `any v = call()` stored the raw pointer/value in v, so downstream
//! `json.encode(v)` walked an empty tag and produced the literal `"null"`.
//!
//! The local-assignment path (`any v = string_expr; return v`) already
//! routed through `emit_box_any`, which is why pairs literals — boxed at
//! their construction site — also worked. Only the *direct* return path
//! was missing the boxing op.
//!
//! The fix runs in the MIR builder Return statement handler: when the
//! function's declared return type is `MirType::Any` and the return
//! expression's type is concrete, the value is routed through
//! `emit_box_any` before being placed on the return terminator.
//!
//! This test compiles a minimal repro that returns a string literal from
//! an `any`-typed function and walks the emitted WASM. The function body
//! must contain a Call to `mem_alloc` — the heap allocation that backs
//! the 12-byte tagged box. Pre-fix, the body had no allocation and
//! simply pushed the raw string pointer before `return`.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::MemoryTier;

const REPRO: &str = r#"start:
	any v = makeAnyStr()
	print(v)

functions:
	any makeAnyStr()
		return "hello"
"#;

#[test]
fn any_return_typed_string_routes_through_box_alloc() {
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

    // Locate the function index of `makeAnyStr` via the export section.
    // The MIR builder emits one user function per definition; we don't need
    // to introspect the export — but the export name is the cleanest hook.
    let mut make_any_str_idx: Option<u32> = None;
    let mut mem_alloc_idx: Option<u32> = None;
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
                    if export.name == "makeAnyStr"
                        && matches!(export.kind, wasmparser::ExternalKind::Func)
                    {
                        make_any_str_idx = Some(export.index);
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

    let make_any_str_idx =
        make_any_str_idx.expect("test setup: makeAnyStr must be exported by the compiled module");
    let mem_alloc_idx = mem_alloc_idx
        .expect("test setup: mem_alloc must be either imported or exported by the module");

    // The code section orders entries by *defined* function index, which is
    // `total_function_index - imported_function_count`. Use the export's
    // index minus imports to land on the right code entry.
    assert!(
        make_any_str_idx >= imported_func_count,
        "makeAnyStr at index {} is below the imported-function count {}; \
         this would mean the codegen exported an imported function under \
         the user's name — that's a different bug.",
        make_any_str_idx,
        imported_func_count
    );
    let body_index = (make_any_str_idx - imported_func_count) as usize;

    let mut current_body: usize = 0;
    let mut calls_mem_alloc_in_makeanystr = false;

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

        loop {
            match reader.read() {
                Ok(Operator::Call { function_index }) if function_index == mem_alloc_idx => {
                    calls_mem_alloc_in_makeanystr = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        break;
    }

    assert!(
        calls_mem_alloc_in_makeanystr,
        "expected `makeAnyStr` to call `mem_alloc` for its any-box allocation, \
         but the function body contains no such call. This is the \
         CODEGEN-ANY-RETURN-STRING-NOT-BOXED regression — the MIR builder \
         is no longer routing `return <typed-expr>` through `emit_box_any` \
         when the function's declared return type is `any`."
    );
}
