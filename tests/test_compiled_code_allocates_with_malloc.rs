//! Regression test for #40: compiled code allocates through the module's
//! own allocator.
//!
//! MEMORY_MODEL.md "Allocation": compiled code calls `__malloc` (exported as
//! `malloc`). Boxing into `any` (BoxAny), class instances (Alloca), object
//! literals, the class JSON trees and the validator functions still called
//! the host's `memory_runtime.mem_alloc` import. Two allocators bumping one
//! heap stay apart only while every host keeps `__heap_ptr` in sync on every
//! call (the frame.ui browser loader did not until 3.0.16), and the
//! validators passed `mem_alloc(type_id, size)` its arguments swapped.
//!
//! This test compiles a program that uses every one of those paths and
//! checks that no function body calls the `mem_alloc` import, while the
//! allocations are still made (calls to `malloc`). The import itself stays:
//! EXECUTION_LAYERS.md lists `memory_runtime.*` among the always-emitted
//! imports.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::MemoryTier;

const PROGRAM: &str = r#"start:
	Point p = Point(1, 2)
	list<Point> points = [p, (Point(3, 4))]
	any boxed = makeAny()
	any obj = {"name": "box", "size": 3}
	string s = "{\"x\": 1}"
	any v = json.get(s, "x")
	integer? maybe = 5
	integer rules = validator.create()
	print(json.encode(p))
	print(json.encode(points))
	print(json.encode(obj))
	print(boxed)
	print(v)
	print(maybe default 0)
	print(validator.isOk(validator.ok(1)))

class Point
	public:
		integer x
		integer y

	constructor(integer xParam, integer yParam)
		x = xParam
		y = yParam

functions:
	any makeAny()
		return "hello"
"#;

#[test]
fn compiled_code_never_calls_host_mem_alloc() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("main.cln");
    fs::write(&main, PROGRAM).expect("write main.cln");

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
        panic!("program must compile");
    });

    let mut mem_alloc_idx: Option<u32> = None;
    let mut malloc_idx: Option<u32> = None;
    let mut imported_func_count: u32 = 0;
    for payload in WasmParser::new(0).parse_all(&wasm) {
        match payload {
            Ok(Payload::ImportSection(reader)) => {
                for import in reader.into_iter().flatten() {
                    if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                        if import.module == "memory_runtime" && import.name == "mem_alloc" {
                            mem_alloc_idx = Some(imported_func_count);
                        }
                        imported_func_count += 1;
                    }
                }
            }
            Ok(Payload::ExportSection(reader)) => {
                for export in reader.into_iter().flatten() {
                    if export.name == "malloc"
                        && matches!(export.kind, wasmparser::ExternalKind::Func)
                    {
                        malloc_idx = Some(export.index);
                    }
                }
            }
            _ => {}
        }
    }
    let mem_alloc_idx =
        mem_alloc_idx.expect("memory_runtime.mem_alloc stays imported (EXECUTION_LAYERS.md)");
    let malloc_idx = malloc_idx.expect("the module exports its allocator as `malloc`");

    let mut body_index = imported_func_count;
    let mut mem_alloc_callers = Vec::new();
    let mut malloc_calls = 0usize;
    for payload in WasmParser::new(0).parse_all(&wasm) {
        let Ok(Payload::CodeSectionEntry(body)) = payload else {
            continue;
        };
        let reader = body.get_operators_reader().expect("operators");
        for op in reader {
            match op.expect("operator") {
                Operator::Call { function_index } if function_index == mem_alloc_idx => {
                    mem_alloc_callers.push(body_index);
                }
                Operator::Call { function_index } if function_index == malloc_idx => {
                    malloc_calls += 1;
                }
                _ => {}
            }
        }
        body_index += 1;
    }

    assert!(
        mem_alloc_callers.is_empty(),
        "functions {mem_alloc_callers:?} call the host's memory_runtime.mem_alloc; \
         compiled code must allocate with `__malloc` (MEMORY_MODEL.md \"Allocation\")"
    );
    assert!(
        malloc_calls > 0,
        "the program allocates, so the module must call `__malloc`"
    );
}
