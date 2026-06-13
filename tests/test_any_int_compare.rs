//! Regression test for ANY_INT_COMPARE_USES_POINTER
//! (fingerprint `3aba3137ba1e48150d74e13f071a8584432ba051f241544b14363b70594cdc74`,
//! reported against compiler 0.30.276).
//!
//! Background: when a binary comparison operator (`==`, `!=`, `<`, `>`,
//! `<=`, `>=`) has one operand of static type `any` (e.g. the result of
//! field access on an `any`-typed receiver: `data.count`) and the other
//! operand is an integer literal, the codegen used to emit a raw
//! `i32.eq`/`i32.ne` comparing the boxed any-pointer to the integer
//! literal. That always evaluated against the heap pointer rather than
//! the contained number, so `data.count == 0` was never true and
//! `data.count != 0` was always true when the pointer was non-null.
//!
//! Fix lives in src/mir/mir_builder/expressions.rs — the regular binary
//! operation builder now detects `Any` operands paired with a primitive
//! operand and emits `UnboxAnyToI32` / `UnboxAnyToF64` /
//! `UnboxAnyToBoolean` (via the existing `emit_unbox_any` helper) before
//! the BinaryOp, so the comparison operates on the contained value.
//!
//! This test compiles a minimal repro and walks the emitted WASM code
//! sections looking for the unbox shape: after a `Call` that produces
//! an any-pointer, we expect to see an `I32Load { offset == 0 }` (the
//! type tag check) before any `I32Eq` / `I32Ne` reduces the result. The
//! pre-fix bug shape is a bare `I32Eq` immediately after the call.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::MemoryTier;

const REPRO: &str = r#"start:
	any data = get_data()
	if data.count == 0
		print("BRANCH_ZERO\n")
	if data.count == 1
		print("BRANCH_ONE\n")
	if data.count != 0
		print("BRANCH_NOT_ZERO\n")

functions:
	any get_data()
		return "{\"count\": 0}"
"#;

#[test]
fn any_field_int_compare_emits_unbox_before_eq() {
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

    // Walk every code section. For each function body, track whether we
    // see the unbox shape (Call → … I32Load offset=0 … I32Eq/Ne) anywhere.
    // The repro has three eq/ne comparisons against any.count, so we
    // expect the unbox shape to appear at least three times across the
    // start function body.
    let mut unbox_then_eq_count = 0usize;
    let mut bare_eq_after_call_count = 0usize;

    for payload in WasmParser::new(0).parse_all(&wasm) {
        let Ok(Payload::CodeSectionEntry(body)) = payload else {
            continue;
        };
        let Ok(mut reader) = body.get_operators_reader() else {
            continue;
        };

        let mut saw_call = false;
        let mut saw_type_tag_load = false;

        loop {
            match reader.read() {
                Ok(Operator::Call { .. }) => {
                    // Any function call resets the window. A call that
                    // returned an any-pointer is the one we care about,
                    // but we treat every call as a possible start of the
                    // shape — false positives just don't trip the unbox
                    // counter, they're harmless.
                    saw_call = true;
                    saw_type_tag_load = false;
                }
                Ok(Operator::I32Load { memarg }) => {
                    // Type tag is the first 4 bytes of the any-struct:
                    // tag at offset 0, payload at offset 4.
                    if saw_call && memarg.offset == 0 {
                        saw_type_tag_load = true;
                    }
                }
                Ok(Operator::I32Eq) | Ok(Operator::I32Ne) => {
                    if saw_call {
                        if saw_type_tag_load {
                            unbox_then_eq_count += 1;
                        } else {
                            bare_eq_after_call_count += 1;
                        }
                        // Reset window — next eq must be set up by its own call.
                        saw_call = false;
                        saw_type_tag_load = false;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }

    assert!(
        unbox_then_eq_count >= 3,
        "expected at least 3 unbox-then-eq sequences (one per comparison \
         in the repro: `data.count == 0`, `data.count == 1`, \
         `data.count != 0`), but found {}. bare-eq-after-call count: {}. \
         This is the ANY_INT_COMPARE_USES_POINTER regression — the codegen \
         is comparing the boxed any-pointer to the integer literal instead \
         of unboxing first.",
        unbox_then_eq_count,
        bare_eq_after_call_count
    );
}
