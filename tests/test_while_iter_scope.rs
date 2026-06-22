//! Regression test for FRAME-UI-ASSEMBLE-PAGE-COMPANION-NO-ROUTES-MOUNTED
//! (dashboard fingerprint `e8ce04051112`, reported against 0.30.331+).
//!
//! Background: `8c25d971` removed per-iteration `mem_scope_push`/`mem_scope_pop`
//! from every loop form to fix `RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED` (an
//! outer accumulator like `errs = errs + ...` was pointing into the
//! per-iter region after the pop, and the next iteration's allocator was
//! overwriting it). That removal was correct for the unsafe shape but
//! over-corrected: while loops whose body allocates many short-lived
//! strings without escaping any of them to outer scope now leak every
//! allocation until the host's per-request `scope_pop`. frame.ui's
//! `find_unescaped_quote` scans large JSON bodies with a `substring`
//! allocation per iteration of its inner `while`; under the post-0.30.331
//! codegen the cumulative allocations exhaust the plugin's bump arena and
//! `assemble()` traps in `string.substring` partway through the page.
//!
//! Fix: `body_is_iter_scope_safe` (in `src/mir/mir_builder/helpers.rs`)
//! decides at MIR-build time whether a loop body can leak pointers across
//! a `mem_scope_pop`. When it cannot — no outer-scope arena-typed
//! assignment, no `return` — the while loop builder restores per-iteration
//! `mem_scope_push`/`pop` (and `Break`/`Continue` emit the matching pop).
//! When it can, the loop falls back to the post-`8c25d971` no-scope
//! behavior so `RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED` stays fixed.
//!
//! This test asserts both halves of the invariant by inspecting the
//! emitted WASM directly:
//!
//! 1. A while loop with transient `substring` allocations and no outer
//!    ref escape MUST contain `mem_scope_push` and `mem_scope_pop` calls
//!    inside the function body. Without them the cumulative allocations
//!    grow unboundedly and the original symptom reappears.
//!
//! 2. A while loop that assigns the iteration's allocation to an outer
//!    string variable (the canonical RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED
//!    shape, translated to `while` form) MUST NOT contain those calls
//!    inside its body — emitting them would re-introduce the original
//!    use-after-free regression.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

/// Compile a single .cln source and resolve the function indexes for any
/// requested import names so the test can look at calls by *which function*
/// rather than by raw index.
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

    // Walk the import section to map each requested name → its function index.
    // Imports occupy the first N function indexes (where N is the import count
    // restricted to function imports), in the order they appear in the section.
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

/// Count how many `Call` instructions across every code section target
/// any of the given function indexes.
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

/// Safe shape: while loop body allocates `substring` per iteration and
/// never assigns it to an outer-scope reference. Expectation: codegen
/// emits per-iteration `mem_scope_push`/`mem_scope_pop` around the body.
const SAFE_REPRO: &str = r#"start:
	integer i = 0
	integer matched = 0
	string source = "abcdefghij"
	while i < 4000
		string ch = source.substring(i % 10, (i % 10) + 1)
		if ch == "a"
			matched = matched + 1
		i = i + 1
	printl("matched=" + matched.toString())
"#;

/// Unsafe shape: while loop body assigns the iteration's string allocation
/// to an outer-scope string accumulator. Expectation: codegen does NOT
/// emit per-iteration `mem_scope_push`/`mem_scope_pop` (would corrupt
/// `acc` after the pop — the RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED bug).
const UNSAFE_REPRO: &str = r#"start:
	string acc = ""
	integer i = 0
	while i < 5
		acc = acc + "x"
		i = i + 1
	printl(acc)
"#;

#[test]
fn safe_while_loop_emits_per_iteration_mem_scope_marks() {
    let (wasm, idxs) = compile_and_find_imports(SAFE_REPRO, &["mem_scope_push", "mem_scope_pop"]);
    let push_idx = idxs[0].expect(
        "compiler must import `mem_scope_push` — it's listed in the host bridge \
         and registered by every plugin/standalone target",
    );
    let pop_idx = idxs[1].expect("compiler must import `mem_scope_pop`");

    let push_calls = count_calls_to(&wasm, &[push_idx]);
    let pop_calls = count_calls_to(&wasm, &[pop_idx]);

    assert!(
        push_calls >= 1 && pop_calls >= 1,
        "FRAME-UI-ASSEMBLE-PAGE-COMPANION-NO-ROUTES-MOUNTED regression: \
         compiler dropped the per-iteration mem_scope_push/pop pair from a \
         while loop whose body has no outer-scope ref escape. \
         mem_scope_push calls observed = {push_calls}, mem_scope_pop calls observed = {pop_calls}. \
         Without these the inner-loop substring allocations from \
         frame.ui's find_unescaped_quote accumulate until the plugin's \
         bump arena traps mid-assemble."
    );

    // The pair MUST balance (one pop per push). Off-by-one would unbalance
    // the host's scope mark stack and corrupt cross-request reclamation.
    assert_eq!(
        push_calls, pop_calls,
        "push/pop count must balance ({push_calls} pushes vs {pop_calls} pops) — \
         an unbalanced pair leaves a stale scope mark on the host's stack."
    );
}

#[test]
fn unsafe_while_loop_does_not_emit_per_iteration_mem_scope_marks() {
    let (wasm, idxs) = compile_and_find_imports(UNSAFE_REPRO, &["mem_scope_push", "mem_scope_pop"]);

    // If the imports are absent entirely the test trivially passes the
    // "no calls" assertion. The safe-shape test above is the strong
    // guarantee that they're available when needed.
    let push_idx = idxs[0];
    let pop_idx = idxs[1];

    let push_calls = push_idx.map(|i| count_calls_to(&wasm, &[i])).unwrap_or(0);
    let pop_calls = pop_idx.map(|i| count_calls_to(&wasm, &[i])).unwrap_or(0);

    assert_eq!(
        push_calls, 0,
        "RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED regression: compiler emitted \
         per-iteration mem_scope_push around a while loop whose body assigns \
         an iteration-allocated string to an outer-scope `acc` variable. The \
         next iteration's allocator overlaps the freed region; `acc` ends up \
         pointing into garbage. mem_scope_push count observed = {push_calls}."
    );
    assert_eq!(
        pop_calls, 0,
        "RUNTIME-CONSECUTIVE-IF-ITERATE-DROPPED regression: same as above for \
         the matching pop. mem_scope_pop count observed = {pop_calls}."
    );
}
