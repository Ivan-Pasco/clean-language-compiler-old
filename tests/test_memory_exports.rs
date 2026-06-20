//! Regression tests for COMPILER-NO-FREE-EXPORT-LEAKS-WASM-MEMORY
//! (errors.cleanlanguage.dev fingerprint 3409ae36e297).
//!
//! Before this fix, the compiler emitted a `malloc` export (bump allocator)
//! but no `free`, no `scope_push`, no `scope_pop`. Host bridges that guarded
//! reclamation with `if (state.exports.free)` silently no-op'd, so any
//! handler accumulating intermediate strings exhausted the 128 MB linear
//! memory cap even though final output was tiny.
//!
//! The compiler now exports four memory-management functions:
//!   - `malloc(size)  -> ptr`  : bump allocate
//!   - `free(ptr)`              : no-op (bump allocator can't reclaim per-block)
//!   - `scope_push() -> mark`   : save current __heap_ptr
//!   - `scope_pop(mark)`        : reset __heap_ptr to saved mark
//!   - `__heap_ptr` (global)    : direct read/write of the bump pointer

use wasmparser::ExternalKind;
use wasmparser::Parser as WasmParser;
use wasmparser::Payload;

fn compile_source(source: &str) -> Vec<u8> {
    clean_language_compiler::compile(source).expect("compilation should succeed")
}

fn list_function_exports(wasm_bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in WasmParser::new(0).parse_all(wasm_bytes) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader {
                let export = export.expect("valid export");
                if matches!(export.kind, ExternalKind::Func) {
                    names.push(export.name.to_string());
                }
            }
        }
    }
    names
}

fn list_global_exports(wasm_bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in WasmParser::new(0).parse_all(wasm_bytes) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader {
                let export = export.expect("valid export");
                if matches!(export.kind, ExternalKind::Global) {
                    names.push(export.name.to_string());
                }
            }
        }
    }
    names
}

#[test]
fn compiled_module_exports_free() {
    let wasm = compile_source("start:\n\tprint(\"hi\")\n");
    let exports = list_function_exports(&wasm);
    assert!(
        exports.iter().any(|n| n == "free"),
        "compiler must export `free` so host bridges' `if (state.exports.free)` \
         guard passes (COMPILER-NO-FREE-EXPORT-LEAKS-WASM-MEMORY). \
         Got function exports: {:?}",
        exports
    );
}

#[test]
fn compiled_module_exports_malloc_companion_pair() {
    // The malloc/free pair must both exist — exporting one without the other
    // creates the lopsided memory contract that caused the original bug.
    let wasm = compile_source("start:\n\tprint(\"hi\")\n");
    let exports = list_function_exports(&wasm);
    let has_malloc = exports.iter().any(|n| n == "malloc");
    let has_free = exports.iter().any(|n| n == "free");
    assert!(
        has_malloc && has_free,
        "compiler must export both malloc and free as a paired contract. \
         malloc={}, free={}",
        has_malloc,
        has_free
    );
}

#[test]
fn compiled_module_exports_scope_primitives() {
    // Per-request reclaim is achieved by the host wrapping each unit of work:
    //   let mark = scope_push();  // ... work ...  scope_pop(mark);
    // Both must be exported as a pair.
    let wasm = compile_source("start:\n\tprint(\"hi\")\n");
    let exports = list_function_exports(&wasm);
    let has_push = exports.iter().any(|n| n == "scope_push");
    let has_pop = exports.iter().any(|n| n == "scope_pop");
    assert!(
        has_push && has_pop,
        "compiler must export scope_push and scope_pop as a paired \
         region-based reclaim API. scope_push={}, scope_pop={}",
        has_push,
        has_pop
    );
}

#[test]
fn compiled_module_exports_heap_ptr_global() {
    // The host needs to read __heap_ptr (e.g. to sync NEXT_ALLOCATION_OFFSET).
    // This has always been exported — guarding so a refactor doesn't drop it.
    let wasm = compile_source("start:\n\tprint(\"hi\")\n");
    let globals = list_global_exports(&wasm);
    assert!(
        globals.iter().any(|n| n == "__heap_ptr"),
        "compiler must export __heap_ptr global so the host can sync the \
         bump pointer with its own NEXT_ALLOCATION_OFFSET. Got: {:?}",
        globals
    );
}
