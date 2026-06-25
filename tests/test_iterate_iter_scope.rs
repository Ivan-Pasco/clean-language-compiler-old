//! Regression test for COMPILER-NO-PER-ITERATION-SCOPE-POP-LEAKS-INTRA-REQUEST-HEAP
//! (dashboard fingerprint `fd14b588`, reported against 0.30.355 — symptom:
//! handlers that walk data with `iterate` + `string.split` + `string.trim`
//! exhaust WASM linear memory within a single request because every transient
//! string allocation accumulates until the host's per-request `scope_pop`).
//!
//! Background: `8c25d971` removed per-iteration `mem_scope_push`/`mem_scope_pop`
//! from every loop form (for/while/range_for) to fix
//! `RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED`. `a3a8e521` then restored the
//! scope pair for `while` loops gated by `body_is_iter_scope_safe` —
//! re-enabling per-iter reclamation when the body cannot leak a pointer
//! across the pop. That commit explicitly deferred the same treatment for
//! `iterate` (TastStatement::For with a collection iterable) and range
//! `iterate` (`build_range_for_loop`) as a follow-up.
//!
//! This commit is the follow-up. Both loop forms now use the same
//! `body_is_iter_scope_safe` predicate as while loops:
//!
//! - Safe shape: body allocates transient strings (`string.split`,
//!   `string.trim`, `string.substring`, etc.) and never assigns the
//!   resulting pointer to an outer-scope reference. Codegen emits
//!   per-iteration `mem_scope_push`/`mem_scope_pop` so the bump arena
//!   rewinds at iteration end.
//!
//! - Unsafe shape: body assigns the iteration's string allocation to an
//!   outer-scope accumulator (the canonical
//!   `RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED` shape). Codegen omits the
//!   pair so the outer pointer stays valid.
//!
//! This test asserts both halves of the invariant for both iterate forms
//! by inspecting the emitted WASM directly.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

/// Compile a single .cln source and resolve function indexes for the given
/// import names. Mirrors the helper in `tests/test_while_iter_scope.rs`.
fn compile_and_find_imports(source: &str, import_names: &[&str]) -> (Vec<u8>, Vec<Option<u32>>) {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("repro.cln");
    fs::write(&main, source).expect("write repro.cln");

    let (wasm, _build_state) = compile_multi_file_with_memory_tier(
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

    let mut next_func_idx: u32 = 0;
    let mut indexes = vec![None; import_names.len()];

    for payload in WasmParser::new(0).parse_all(&wasm) {
        let Ok(Payload::ImportSection(reader)) = payload else {
            continue;
        };
        for import in reader {
            let import = import.expect("read import");
            if let wasmparser::TypeRef::Func(_) = import.ty {
                for (i, name) in import_names.iter().enumerate() {
                    if &import.name == name {
                        indexes[i] = Some(next_func_idx);
                    }
                }
                next_func_idx += 1;
            }
        }
    }

    (wasm, indexes)
}

/// Count `Call` instructions across every code section that target any of
/// the given function indexes.
fn count_calls_to(wasm: &[u8], targets: &[u32]) -> usize {
    let mut total = 0usize;
    for payload in WasmParser::new(0).parse_all(wasm) {
        let Ok(Payload::CodeSectionEntry(body)) = payload else {
            continue;
        };
        let Ok(mut reader) = body.get_operators_reader() else {
            continue;
        };
        loop {
            match reader.read() {
                Ok(Operator::Call { function_index }) => {
                    if targets.contains(&function_index) {
                        total += 1;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Collection iterate (`iterate v in list`)
// ---------------------------------------------------------------------------

/// Safe shape: iterate body allocates transient `string.trim` / `string.split`
/// strings, never assigns the result to an outer-scope reference, never
/// returns or throws. Mirrors the SSR handler pattern from the bug report.
const SAFE_ITERATE_REPRO: &str = r#"start:
	list<string> items = "alpha , beta ,  gamma".split(",")
	integer matched = 0
	iterate item in items
		string trimmed = item.trim()
		if trimmed == "beta"
			matched = matched + 1
	printl("matched=" + matched.toString())
"#;

/// Unsafe shape: iterate body concatenates each element into an outer
/// `acc` string. Canonical `RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED` shape.
const UNSAFE_ITERATE_REPRO: &str = r#"start:
	list<string> items = "a,b,c".split(",")
	string acc = ""
	iterate item in items
		acc = acc + item
	printl(acc)
"#;

#[test]
fn safe_iterate_loop_emits_per_iteration_mem_scope_marks() {
    let (wasm, idxs) =
        compile_and_find_imports(SAFE_ITERATE_REPRO, &["mem_scope_push", "mem_scope_pop"]);
    let push_idx = idxs[0]
        .expect("compiler must import `mem_scope_push` — listed in the host bridge registry");
    let pop_idx = idxs[1].expect("compiler must import `mem_scope_pop`");

    let push_calls = count_calls_to(&wasm, &[push_idx]);
    let pop_calls = count_calls_to(&wasm, &[pop_idx]);

    assert!(
        push_calls >= 1 && pop_calls >= 1,
        "COMPILER-NO-PER-ITERATION-SCOPE-POP-LEAKS-INTRA-REQUEST-HEAP regression: \
         compiler dropped the per-iteration mem_scope_push/pop pair from an \
         iterate loop whose body has no outer-scope ref escape. \
         mem_scope_push calls observed = {push_calls}, mem_scope_pop calls observed = {pop_calls}. \
         Without these the per-iteration `string.trim`/`string.split` allocations \
         accumulate until the per-request scope is torn down, exhausting the \
         bump arena on long-running handlers."
    );

    // Pair must balance — one pop per push. Off-by-one would unbalance the
    // host's scope stack and corrupt cross-request reclamation.
    assert_eq!(
        push_calls, pop_calls,
        "push/pop count must balance ({push_calls} pushes vs {pop_calls} pops) — \
         an unbalanced pair leaves a stale scope mark on the host's stack."
    );
}

#[test]
fn unsafe_iterate_loop_does_not_emit_per_iteration_mem_scope_marks() {
    let (wasm, idxs) =
        compile_and_find_imports(UNSAFE_ITERATE_REPRO, &["mem_scope_push", "mem_scope_pop"]);

    let push_idx = idxs[0];
    let pop_idx = idxs[1];

    let push_calls = push_idx.map(|i| count_calls_to(&wasm, &[i])).unwrap_or(0);
    let pop_calls = pop_idx.map(|i| count_calls_to(&wasm, &[i])).unwrap_or(0);

    assert_eq!(
        push_calls, 0,
        "RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED regression (iterate): compiler \
         emitted per-iteration mem_scope_push around an iterate loop whose \
         body assigns each iteration's string allocation to an outer-scope \
         `acc` accumulator. The next iteration's allocator overlaps the freed \
         region; `acc` ends up pointing into garbage. mem_scope_push count = {push_calls}."
    );
    assert_eq!(
        pop_calls, 0,
        "RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED regression (iterate): same as \
         above for the matching pop. mem_scope_pop count = {pop_calls}."
    );
}

// ---------------------------------------------------------------------------
// Range iterate (`iterate i in 0 to N`)
// ---------------------------------------------------------------------------

/// Safe shape: range iterate body allocates a transient substring per
/// iteration without escape.
const SAFE_RANGE_REPRO: &str = r#"start:
	string source = "abcdefghij"
	integer matched = 0
	iterate i in 0 to 4000
		string ch = source.substring(i % 10, (i % 10) + 1)
		if ch == "a"
			matched = matched + 1
	printl("matched=" + matched.toString())
"#;

/// Unsafe shape: range iterate body concatenates into an outer accumulator.
const UNSAFE_RANGE_REPRO: &str = r#"start:
	string acc = ""
	iterate i in 0 to 5
		acc = acc + "x"
	printl(acc)
"#;

#[test]
fn safe_range_iterate_emits_per_iteration_mem_scope_marks() {
    let (wasm, idxs) =
        compile_and_find_imports(SAFE_RANGE_REPRO, &["mem_scope_push", "mem_scope_pop"]);
    let push_idx = idxs[0].expect("compiler must import `mem_scope_push`");
    let pop_idx = idxs[1].expect("compiler must import `mem_scope_pop`");

    let push_calls = count_calls_to(&wasm, &[push_idx]);
    let pop_calls = count_calls_to(&wasm, &[pop_idx]);

    assert!(
        push_calls >= 1 && pop_calls >= 1,
        "COMPILER-NO-PER-ITERATION-SCOPE-POP-LEAKS-INTRA-REQUEST-HEAP regression \
         (range iterate): compiler dropped the per-iteration mem_scope_push/pop \
         pair from a range loop whose body has no outer-scope ref escape. \
         mem_scope_push calls = {push_calls}, mem_scope_pop calls = {pop_calls}."
    );
    assert_eq!(
        push_calls, pop_calls,
        "push/pop count must balance ({push_calls} vs {pop_calls})."
    );
}

#[test]
fn unsafe_range_iterate_does_not_emit_per_iteration_mem_scope_marks() {
    let (wasm, idxs) =
        compile_and_find_imports(UNSAFE_RANGE_REPRO, &["mem_scope_push", "mem_scope_pop"]);

    let push_idx = idxs[0];
    let pop_idx = idxs[1];

    let push_calls = push_idx.map(|i| count_calls_to(&wasm, &[i])).unwrap_or(0);
    let pop_calls = pop_idx.map(|i| count_calls_to(&wasm, &[i])).unwrap_or(0);

    assert_eq!(
        push_calls, 0,
        "RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED regression (range iterate): \
         compiler emitted per-iteration mem_scope_push around a range loop \
         whose body assigns into an outer-scope accumulator. push count = {push_calls}."
    );
    assert_eq!(
        pop_calls, 0,
        "RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED regression (range iterate): \
         matching pop. pop count = {pop_calls}."
    );
}
