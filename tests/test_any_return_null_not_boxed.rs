//! Regression test for CODEGEN-ANY-RETURN-NULL-BOXED-AS-NONZERO-BREAKS-GUARD-CHECK
//! (fingerprint `0de035ddd2b134635af1f744af9547f75caaf0212fb6e8676709257972c120d7`,
//! reported against compiler 0.30.318).
//!
//! Background: the fix in 2ab90a6f (CODEGEN-ANY-RETURN-STRING-NOT-BOXED)
//! routed every typed return through `emit_box_any` so json.encode could
//! see the type tag. The new path correctly handled string/int/number/
//! boolean returns — but unintentionally also boxed `return null`, wrapping
//! the null literal into a heap struct whose pointer is non-zero.
//!
//! Why that broke real apps: frame.ui auto-generates each page-render
//! handler as
//!
//!     guard_result = page.guard(request)
//!     if guard_result != 0:
//!         return guard_result
//!
//! Pre-fix `return null` from a guard returned a raw `i32.const 0`, so the
//! `if != 0` test failed and the handler fell through to `load() + render()`.
//! Post-2ab90a6f the guard returned a non-zero boxed-null pointer, the
//! handler thought "guard intervened", and every page with a guard
//! returned `HTTP 200 text/plain Content-Length: 0`.
//!
//! The fix: special-case `ConcreteType::Null` in the any-return-boxing
//! branch so the null literal continues to round-trip as raw 0 at the
//! WASM boundary. Other concrete types still box.
//!
//! This test compiles a minimal repro that returns `null` from an
//! `any`-typed function and walks the emitted WASM. The function body
//! must NOT contain a `Call` to `mem_alloc` — the presence of one is the
//! signature of the box allocation that this fix removes for null.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::MemoryTier;

const REPRO: &str = r#"start:
	any v = makeAnyNull()
	print(v)

functions:
	any makeAnyNull()
		return null
"#;

#[test]
fn any_return_null_does_not_allocate_a_box() {
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

    let mut make_any_null_idx: Option<u32> = None;
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
                    if export.name == "makeAnyNull"
                        && matches!(export.kind, wasmparser::ExternalKind::Func)
                    {
                        make_any_null_idx = Some(export.index);
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

    let make_any_null_idx =
        make_any_null_idx.expect("test setup: makeAnyNull must be exported by the compiled module");
    let mem_alloc_idx = mem_alloc_idx
        .expect("test setup: mem_alloc must be either imported or exported by the module");

    assert!(
        make_any_null_idx >= imported_func_count,
        "makeAnyNull at index {} is below the imported-function count {}; \
         this would mean the codegen exported an imported function under \
         the user's name — that's a different bug.",
        make_any_null_idx,
        imported_func_count
    );
    let body_index = (make_any_null_idx - imported_func_count) as usize;

    let mut current_body: usize = 0;
    let mut calls_mem_alloc_in_makeanynull = false;

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
                    calls_mem_alloc_in_makeanynull = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        break;
    }

    assert!(
        !calls_mem_alloc_in_makeanynull,
        "`makeAnyNull` should NOT call `mem_alloc` — `return null` from an \
         any-returning function must emit a raw `i32.const 0`, not a boxed \
         null. A box allocation here means the regression \
         CODEGEN-ANY-RETURN-NULL-BOXED-AS-NONZERO-BREAKS-GUARD-CHECK is \
         live: every frame.ui page with a guard() would return HTTP 200 \
         with an empty body."
    );
}
