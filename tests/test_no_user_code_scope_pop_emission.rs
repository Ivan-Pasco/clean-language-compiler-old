//! Regression test for COMPILER-SCOPE-POP-FREES-RETURN-VALUE
//! (dashboard fingerprint `3fdc228524deab933f9e3b5cd8760a4c4283a67b3b184d77a4f4cad9b3d83c0d`,
//! reported against compiler 0.30.326–0.30.330).
//!
//! Background: between 0.30.326 and 0.30.330, the MIR builder wrapped the
//! body of every for/while/range-for iteration in
//! `call $mem_scope_push` / `call $mem_scope_pop` pairs. The intent was to
//! reclaim transient body allocations at iteration end. But strings
//! allocated inside a body and then assigned to an outer accumulator
//! (`acc = acc + makeChunk(i)`) escape the iteration — the `mem_scope_pop`
//! still rewinds `__heap_ptr` past the escapee, leaving the outer pointer
//! dangling. Under cln-node-server 0.1.68 (which now actually wires
//! `mem_scope_pop` to `state.exports.scope_pop(snapshot)`), the rewind
//! succeeds and the dangling pointer is read back as garbage — observed as
//! `WASM malloc returned null in string.concat: need 857,941,631 bytes` on
//! a /tutorials endpoint that allocates 30 chunks per request.
//!
//! Pre-0.1.68, the host's `mem_scope_pop` bridge was a no-op, so the
//! pointer remained valid by accident (bump allocator never overwrote it).
//! The CNS 0.1.68 wire-up surfaced the latent compiler bug.
//!
//! Fix (commit 8c25d971, 0.30.331): remove the per-iteration push/pop
//! emission from for/while/range-for and from break/continue. The host
//! handles reclamation at request boundaries via the *exported*
//! `scope_push` / `scope_pop` (which use a different code path that
//! manipulates `__heap_ptr` directly — see `codegen/native_stdlib/memory.rs`).
//!
//! This test is a **structural** guard: it compiles a repro that
//! historically emitted `call $mem_scope_pop` and asserts that the
//! resulting WASM contains ZERO such calls anywhere in the code section.
//! A future change that re-introduces per-iteration, per-function, or
//! per-block scope_pop emission in user code will fire this test
//! immediately — independently of whether the execution-level symptom
//! (dangling pointer corruption) happens to be observable on the current
//! host implementation.
//!
//! Why a structural test over an execution test:
//! - The execution symptom requires the host to actually honor the
//!   `mem_scope_pop` bridge with a real `__heap_ptr` rewind. The bundled
//!   `wasmtime_runner` may register it as a no-op (as the historical host
//!   did), masking the bug. A structural assertion catches the emission
//!   regardless of host behavior.
//! - The original bug was latent for months precisely because the host
//!   was lenient. Guarding the compiler's emission directly removes that
//!   dependency on host behavior for regression detection.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::MemoryTier;

/// The escape pattern from the bug report: a function that allocates a
/// string and returns it, called inside a while loop, with the return
/// value assigned to an outer accumulator. Pre-fix, the while body was
/// wrapped in `mem_scope_push`/`mem_scope_pop`, freeing `makeChunk`'s
/// return value before the caller's concat could safely use it.
const REPRO: &str = r#"start:
	string acc = ""
	integer i = 0
	while i < 5
		acc = acc + makeChunk(i)
		i = i + 1
	print(acc)

functions:
	string makeChunk(integer i)
		return "chunk-" + i.toString() + "-payload"
"#;

#[test]
fn user_code_must_not_call_mem_scope_pop() {
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
        panic!("repro must compile cleanly");
    });

    // Walk imports to locate the mem_scope_push / mem_scope_pop function
    // indices. Both are emitted as imports unconditionally (see
    // codegen_module_builder.rs) so a missing import is itself a setup
    // failure, not a regression signal.
    let mut imported_func_count: u32 = 0;
    let mut mem_scope_pop_idx: Option<u32> = None;
    let mut mem_scope_push_idx: Option<u32> = None;

    for payload in WasmParser::new(0).parse_all(&wasm) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let Ok(import) = import else { continue };
                if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                    if import.name == "mem_scope_pop" {
                        mem_scope_pop_idx = Some(imported_func_count);
                    } else if import.name == "mem_scope_push" {
                        mem_scope_push_idx = Some(imported_func_count);
                    }
                    imported_func_count += 1;
                }
            }
        }
    }

    let mem_scope_pop_idx = mem_scope_pop_idx
        .expect("test setup: mem_scope_pop must be imported by the compiled module");
    let mem_scope_push_idx = mem_scope_push_idx
        .expect("test setup: mem_scope_push must be imported by the compiled module");

    // Walk EVERY function body. The historical bug emitted per-iteration
    // push/pop in any loop, so we don't single out a particular function —
    // any call anywhere in the code section is the regression.
    //
    // Note: the *exported* `__scope_push` / `__scope_pop` functions (used
    // by hosts to wrap a whole request) are generated from
    // `native_stdlib::memory::gen_scope_push` / `gen_scope_pop`, which
    // emit a `GlobalGet`/`GlobalSet HEAP_PTR_GLOBAL` sequence — they do
    // NOT call the imported `mem_scope_*` bridge functions. So a clean
    // module legitimately contains zero such calls.
    let mut bad_call_count = 0usize;
    let mut bad_push_count = 0usize;

    for payload in WasmParser::new(0).parse_all(&wasm) {
        let Ok(Payload::CodeSectionEntry(body)) = payload else {
            continue;
        };
        let Ok(mut reader) = body.get_operators_reader() else {
            continue;
        };
        loop {
            match reader.read() {
                Ok(Operator::Call { function_index }) if function_index == mem_scope_pop_idx => {
                    bad_call_count += 1;
                }
                Ok(Operator::Call { function_index }) if function_index == mem_scope_push_idx => {
                    bad_push_count += 1;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }

    assert_eq!(
        bad_call_count, 0,
        "COMPILER-SCOPE-POP-FREES-RETURN-VALUE: emitted WASM contains {bad_call_count} \
         `call $mem_scope_pop` instruction(s) in the code section. Per-iteration / \
         per-function `mem_scope_pop` emission is unsound — any allocation that \
         escapes the popped scope (assigned to an outer accumulator, returned to \
         the caller, etc.) becomes a dangling pointer the moment the host honors \
         the bridge with a real `__heap_ptr` rewind. Scopes must be established \
         and torn down by the host at request boundaries via the EXPORTED \
         `scope_push`/`scope_pop` only."
    );
    assert_eq!(
        bad_push_count, 0,
        "COMPILER-SCOPE-POP-FREES-RETURN-VALUE: emitted WASM contains {bad_push_count} \
         `call $mem_scope_push` instruction(s). The push is the matching half of \
         the unsound pop pair; it shouldn't appear in user code either."
    );
}
