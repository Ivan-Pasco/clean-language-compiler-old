//! The linear-memory layout documented at `codegen::DATA_SECTION_START`:
//! string literals from 4 KB, packed into one data segment, then the 64 KB
//! transient arena pool, then the heap, whose start is the initial
//! `__heap_ptr`. Each literal used to get its own data segment plus 16 bytes
//! of padding nothing read.
//!
//! Also guards the removal of `string_builder_reclaim` / `heap_ptr_snapshot`,
//! two helpers nothing has called since the 0.30.375 reclaim rollback that
//! were still compiled into, and exported from, every module.

use wasmparser::{DataKind, Operator, Parser, Payload};

const STRING_POOL_START: u32 = 4096;
const TRANSIENT_POOL_SIZE: u32 = 65_536;

fn const_i32(expr: &wasmparser::ConstExpr) -> i32 {
    let mut reader = expr.get_operators_reader();
    match reader.read().expect("const expr operator") {
        Operator::I32Const { value } => value,
        other => panic!("expected i32.const, got {other:?}"),
    }
}

/// (first global's initial value, data segments as (offset, length), exports)
fn layout(wasm: &[u8]) -> (i32, Vec<(u32, u32)>, Vec<String>) {
    let mut heap_start = None;
    let mut segments = Vec::new();
    let mut exports = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.expect("valid module") {
            Payload::GlobalSection(reader) => {
                for global in reader {
                    let global = global.expect("global");
                    heap_start.get_or_insert_with(|| const_i32(&global.init_expr));
                }
            }
            Payload::DataSection(reader) => {
                for data in reader {
                    let data = data.expect("data segment");
                    if let DataKind::Active { offset_expr, .. } = data.kind {
                        segments.push((const_i32(&offset_expr) as u32, data.data.len() as u32));
                    }
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    exports.push(export.expect("export").name.to_string());
                }
            }
            _ => {}
        }
    }
    (heap_start.expect("__heap_ptr global"), segments, exports)
}

#[test]
fn heap_starts_after_strings_and_transient_pool() {
    let source = "start:\n\tprint(\"hello\")\n\tprint(\"layout\")\n";
    let wasm = clean_language_compiler::compile(source).expect("compiles");
    let (heap_start, segments, _) = layout(&wasm);

    let first = segments
        .iter()
        .map(|(offset, _)| *offset)
        .min()
        .expect("data");
    assert_eq!(first, STRING_POOL_START, "string literals start at 4 KB");
    let strings_end = segments
        .iter()
        .map(|(offset, len)| offset + len)
        .max()
        .expect("data");
    let pool_base = (strings_end + 7) & !7;
    assert_eq!(
        heap_start as u32,
        pool_base + TRANSIENT_POOL_SIZE,
        "the heap starts right after the strings ({strings_end}) and the transient pool"
    );
    assert_eq!(segments.len(), 1, "string literals form one data segment");
}

#[test]
fn removed_reclaim_helpers_are_gone() {
    let wasm = clean_language_compiler::compile("start:\n\tprint(\"hi\")\n").expect("compiles");
    let (_, _, exports) = layout(&wasm);
    for name in [
        "string_builder_reclaim",
        "__string_builder_reclaim",
        "heap_ptr_snapshot",
        "__heap_ptr_snapshot",
    ] {
        assert!(
            !exports.iter().any(|e| e == name),
            "{name} is still exported"
        );
    }

    let calls_removed_builtin = "start:\n\tinteger mark = heap_ptr_snapshot()\n\tprint(mark)\n";
    assert!(
        clean_language_compiler::compile(calls_removed_builtin).is_err(),
        "heap_ptr_snapshot is no longer a builtin"
    );
}
