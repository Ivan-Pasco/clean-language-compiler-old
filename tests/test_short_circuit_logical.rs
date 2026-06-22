//! Spec regression test for short-circuit `and` / `or`.
//!
//! Per `foundation/spec/type-system.md` §11 "Short-Circuit Evaluation of
//! `and` / `or`", the boolean logical operators must NOT evaluate the
//! right-hand operand once the left-hand operand has already decided the
//! result. Before this change, `and` / `or` lowered to bitwise
//! `i32.and` / `i32.or` and both operands always ran; common
//! boundary-guarded loops like
//!
//!     while i < s.length() and s.substring(i, i + 1) == " "
//!
//! invoked `string.substring(s, len, len + 1)` exactly when `i == len`,
//! which traps the WASM substring intrinsic on the out-of-bounds load.
//! frame.ui's `extract_json_field` and `find_unescaped_quote` ride that
//! exact pattern across eight call sites — dashboard fingerprints
//! `738104df15e4`, `02542ef548d3`, `959eea9a2373`, and the framework
//! report I filed at `e2b5be3967a5`
//! (`FRAME-UI-EXTRACT-JSON-FIELD-NON-SHORT-CIRCUIT-AND-OOB`).
//!
//! This test pins the emitted WASM shape: the compiled `and` / `or`
//! must be a single `if (result i32) … else … end` block, NOT a flat
//! `i32.and` / `i32.or`. The `if` keeps the right-hand operand inside
//! the branch that runs only when the left cannot short-circuit, which
//! is what gives short-circuit semantics in the first place.
//!
//! An end-to-end runtime check (the substring boundary repro) lives in
//! `tests/cln/control/loops/while_short_circuit_boundary.cln`; this
//! test pins the codegen shape so the runtime check can't accidentally
//! start passing for the wrong reason (e.g. constant folding the
//! boundary case at MIR time).

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};

fn compile(source: &str) -> Vec<u8> {
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
        panic!("repro must compile");
    });

    wasm
}

fn function_body_ops_by_export<'a>(wasm: &'a [u8], export_name: &str) -> Vec<Operator<'a>> {
    let mut export_target: Option<u32> = None;
    let mut imported_func_count: u32 = 0;
    let mut func_indices: Vec<u32> = Vec::new();
    let mut codes: Vec<wasmparser::FunctionBody<'a>> = Vec::new();

    for payload in WasmParser::new(0).parse_all(wasm) {
        match payload.expect("payload") {
            Payload::ImportSection(reader) => {
                for import in reader {
                    let import = import.expect("import");
                    if let wasmparser::TypeRef::Func(_) = import.ty {
                        imported_func_count += 1;
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for ty in reader {
                    func_indices.push(ty.expect("function type idx"));
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.expect("export");
                    if export.name == export_name {
                        if let wasmparser::ExternalKind::Func = export.kind {
                            export_target = Some(export.index);
                        }
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                codes.push(body);
            }
            _ => {}
        }
    }

    let target = export_target.unwrap_or_else(|| panic!("export {export_name} not found"));
    let local_idx = target
        .checked_sub(imported_func_count)
        .expect("export is an import, not a local function") as usize;
    let body = codes
        .get(local_idx)
        .unwrap_or_else(|| panic!("no code body for export {export_name}"));

    let mut ops = Vec::new();
    let reader = body.get_operators_reader().expect("operators reader");
    for op in reader {
        ops.push(op.expect("op"));
    }
    ops
}

#[test]
fn logical_and_lowers_to_wasm_if_else_block() {
    // Inline `and` in expression position. Has no side-effecting rhs so
    // the codegen shape is the easiest thing to assert on without
    // having to reason about ValueId-to-local mapping.
    let source = "\
start:\n\
\tboolean a = false\n\
\tboolean b = true\n\
\tboolean r = a and b\n\
\tprintln(r.toString())\n\
";
    let wasm = compile(source);
    let ops = function_body_ops_by_export(&wasm, "start");

    let has_if_i32 = ops.iter().any(|op| {
        matches!(
            op,
            Operator::If {
                blockty: wasmparser::BlockType::Type(wasmparser::ValType::I32),
            }
        )
    });
    assert!(
        has_if_i32,
        "`a and b` must lower to a WASM `if (result i32) ... else ... end` \
         block so the right-hand operand is only evaluated when the \
         left-hand cannot short-circuit. Did the MIR `LogicalShortCircuit` \
         get bypassed?"
    );

    let has_i32_and = ops.iter().any(|op| matches!(op, Operator::I32And));
    assert!(
        !has_i32_and,
        "Logical `and` must NOT lower to bitwise `i32.and` — that would \
         evaluate both operands unconditionally and re-introduce the \
         out-of-bounds substring trap fixed by short-circuiting."
    );
}

#[test]
fn logical_or_lowers_to_wasm_if_else_block() {
    let source = "\
start:\n\
\tboolean a = true\n\
\tboolean b = false\n\
\tboolean r = a or b\n\
\tprintln(r.toString())\n\
";
    let wasm = compile(source);
    let ops = function_body_ops_by_export(&wasm, "start");

    let has_if_i32 = ops.iter().any(|op| {
        matches!(
            op,
            Operator::If {
                blockty: wasmparser::BlockType::Type(wasmparser::ValType::I32),
            }
        )
    });
    assert!(
        has_if_i32,
        "`a or b` must lower to a WASM `if (result i32) ... else ... end` \
         block — the same short-circuit shape as `and`, with the rhs \
         living in the else arm instead of the then arm."
    );

    let has_i32_or = ops.iter().any(|op| matches!(op, Operator::I32Or));
    assert!(
        !has_i32_or,
        "Logical `or` must NOT lower to bitwise `i32.or` — that would \
         evaluate the right operand even when the left already decides \
         the result."
    );
}
