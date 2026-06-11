//! Regression test for the `expand_component` codegen truncation that ships
//! plugins (frame.ui 2.12.0) with a trap-on-call `expand_component` whose WASM
//! body ends with `unreachable` instead of `return`.
//!
//! Root cause is in `src/codegen/mir_codegen/blocks.rs`,
//! `generate_structured_blocks`'s Branch arm — the continuation-selection
//! block (lines 902-955). When the outer `if` has no else clause and its
//! then-branch contains a nested `if/else`, `find_eventual_continuation`
//! returns the *inner* merge block (already inlined while emitting the
//! then-branch). The outer call short-circuits on the "already generated"
//! guard, the actual false_block (containing every post-if statement) is
//! never visited, and the non-void safety net at lines 244-256 appends
//! `Unreachable`. See `/tmp/truncation-bisection-findings.md` for the
//! full bisection.
//!
//! This test is RED on 0.30.263 and should turn green when the codegen fix
//! lands. The companion `.cln` source lives at
//! tests/cln/codegen/expand_component_truncation_regression.cln.

use std::fs;
use wasmparser::{Operator, Parser as WasmParser, Payload};

const REGRESSION_SOURCE: &str = "tests/cln/codegen/expand_component_truncation_regression.cln";

/// Count the number of function imports — body indices in the code section
/// are offset by this, because WASM assigns import indices first.
fn count_func_imports(wasm: &[u8]) -> usize {
    let mut count = 0;
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let import = import.expect("valid import");
                if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Find the global function index for an exported function name.
fn find_exported_func_index(wasm: &[u8], name: &str) -> Option<u32> {
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader {
                let export = export.expect("valid export");
                if export.name == name && matches!(export.kind, wasmparser::ExternalKind::Func) {
                    return Some(export.index);
                }
            }
        }
    }
    None
}

/// Walk operators in a function body and check whether `Operator::Return`
/// is ever observed before the function's terminating End. Returns
/// `(has_return, ends_with_unreachable)`. The pair distinguishes the
/// truncation symptom (ends with `unreachable end`, no `return` anywhere)
/// from a fixed function (contains at least one `return`).
fn analyze_function_body(body: &wasmparser::FunctionBody<'_>) -> (bool, bool) {
    let mut reader = body
        .get_operators_reader()
        .expect("function body has operators");
    // Walk operators, ignoring nested block/loop/if/else structures —
    // wasmparser yields them at the same level. We only care if any
    // `Operator::Return` appears before the function's final `End`.
    //
    // A function body always ends with a final `End` operator that closes
    // the implicit function block. Track depth to identify that final End.
    let mut depth: i32 = 0;
    let mut saw_return = false;
    let mut last_meaningful: Option<Operator<'_>> = None;
    while let Ok(op) = reader.read() {
        match &op {
            Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                depth += 1;
            }
            Operator::Else => {
                // `else` doesn't change depth — it's an alternative branch
                // inside the same if/end pair.
            }
            Operator::End => {
                if depth == 0 {
                    // Function's terminating End — stop walking.
                    break;
                }
                depth -= 1;
            }
            Operator::Return => {
                saw_return = true;
            }
            _ => {}
        }
        // Capture the most recent operator at depth 0 so we can tell whether
        // the function falls through to `unreachable` (truncation symptom).
        if depth == 0 {
            last_meaningful = Some(op.clone());
        }
    }
    let ends_with_unreachable = matches!(last_meaningful, Some(Operator::Unreachable));
    (saw_return, ends_with_unreachable)
}

/// Locate the FunctionBody for the exported `expand_block` and analyze it.
fn analyze_exported_function(wasm: &[u8], name: &str) -> (bool, bool) {
    let func_index =
        find_exported_func_index(wasm, name).unwrap_or_else(|| panic!("export {name:?} missing"));
    let n_imports = count_func_imports(wasm);
    assert!(
        (func_index as usize) >= n_imports,
        "exported {name:?} has func_index {func_index} but module has only {n_imports} imports — \
         the export points into the import range, which is impossible for a body-bearing function"
    );
    let body_index = (func_index as usize) - n_imports;

    let mut current_body: usize = 0;
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::CodeSectionEntry(body)) = payload {
            if current_body == body_index {
                return analyze_function_body(&body);
            }
            current_body += 1;
        }
    }
    panic!("code section body index {body_index} not found for export {name:?}");
}

fn compile_regression_source() -> Vec<u8> {
    let source = fs::read_to_string(REGRESSION_SOURCE).unwrap_or_else(|e| {
        panic!("regression source {REGRESSION_SOURCE} unreadable: {e}");
    });
    clean_language_compiler::compile_for_plugin(&source, REGRESSION_SOURCE).unwrap_or_else(
        |errors| {
            for e in &errors {
                eprintln!("compile error: {e}");
            }
            panic!(
                "regression source failed to compile — {} errors",
                errors.len()
            );
        },
    )
}

/// Primary regression assertion: `expand_block`'s WASM body must contain at
/// least one `Return` instruction. This is the contract the codegen fix has
/// to satisfy. The function returns `string`, so it MUST reach a `return`
/// statement on every path; if codegen drops the post-`if` statements, no
/// `Return` opcode is emitted and the function falls through to the safety
/// net `Unreachable` — the exact symptom of the truncation bug.
#[test]
fn expand_block_body_contains_return() {
    let wasm = compile_regression_source();
    let (saw_return, ends_with_unreachable) = analyze_exported_function(&wasm, "expand_block");

    assert!(
        saw_return,
        "BUG: expand_block's WASM body contains no `return` instruction — the \
         post-`if attrs != \"\"` statements (including `return result`) were dropped \
         by codegen. Trigger shape: outer `if` with no else + nested `if/else` + \
         statements after. See generate_structured_blocks in \
         src/codegen/mir_codegen/blocks.rs (continuation-selection at lines 902-955). \
         body_ends_with_unreachable={ends_with_unreachable}"
    );
}

/// Secondary assertion: the function body must NOT end in fallthrough
/// `unreachable`. A non-void function that ends with `unreachable` will trap
/// at the call site — this is the exact runtime symptom reported in
/// foundation/management/cross-component-prompts/compiler-frame-ui-expand-component-codegen-truncation.md.
#[test]
fn expand_block_body_does_not_end_with_unreachable() {
    let wasm = compile_regression_source();
    let (_saw_return, ends_with_unreachable) = analyze_exported_function(&wasm, "expand_block");

    assert!(
        !ends_with_unreachable,
        "BUG: expand_block's WASM body ends with `unreachable end` — codegen \
         emitted no `return` so the safety net at \
         src/codegen/mir_codegen/blocks.rs:244-256 appended Unreachable to keep \
         the non-void function well-typed. Every call traps at runtime."
    );
}
