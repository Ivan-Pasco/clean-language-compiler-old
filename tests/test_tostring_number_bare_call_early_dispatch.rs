//! Regression test for COM001 fingerprint `f26672a3b9a11308` — bare
//! `toString(x)` where `x` is a `number` (f64) produced invalid WASM.
//!
//! Companion to `test_number_tostring_early_dispatch` (which covers the
//! method-call form `x.toString()`). Both share the same underlying cause:
//! the compiler assumed the callee's parameter slot was i32 while pushing an
//! f64 value onto the stack. This test covers the FunctionCall path where
//! `toString` resolves to the bare `(Integer) -> String` symbol in the
//! resolver and the argument happens to be Number.
//!
//! Fix: `mir_builder/expressions.rs` — an early dispatch in the FunctionCall
//! handler routes `toString(number)` to `float_to_string` (the correct
//! `(f64) -> i32` host import) before the generic function-call codegen has
//! a chance to emit an i32-shaped call.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Parser as WasmParser, Payload, Validator};

use clean_language_compiler::MemoryTier;

const REPRO: &str = r#"start:
	number x = 3.14
	string s = toString(x)
	print(s) +
"#;

#[test]
fn bare_tostring_on_number_compiles_to_valid_wasm() {
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
        panic!("bare toString(number) repro must compile");
    });

    Validator::new()
        .validate_all(&wasm)
        .expect("generated WASM must pass wasmparser validation");

    // Find the `float_to_string` import index and confirm it is called.
    let mut float_to_string_index: Option<u32> = None;
    let mut import_func_count: u32 = 0;

    for payload in WasmParser::new(0).parse_all(&wasm) {
        let Ok(payload) = payload else { continue };
        if let Payload::ImportSection(reader) = payload {
            for import in reader {
                let Ok(import) = import else { continue };
                if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                    if import.name == "float_to_string" {
                        float_to_string_index = Some(import_func_count);
                    }
                    import_func_count += 1;
                }
            }
        }
    }

    let float_to_string_index =
        float_to_string_index.expect("float_to_string must be imported by the module");

    let mut float_to_string_calls: usize = 0;
    for payload in WasmParser::new(0).parse_all(&wasm) {
        let Ok(Payload::CodeSectionEntry(body)) = payload else {
            continue;
        };
        let Ok(mut reader) = body.get_operators_reader() else {
            continue;
        };
        while let Ok(op) = reader.read() {
            if let wasmparser::Operator::Call { function_index } = op {
                if function_index == float_to_string_index {
                    float_to_string_calls += 1;
                }
            }
        }
    }

    assert!(
        float_to_string_calls >= 1,
        "expected bare `toString(number)` to lower to a `call float_to_string`, \
         but no Call to function index {} appears in the module. This means \
         the FunctionCall early dispatch for `toString(number)` is not firing \
         and the compiler is again generating an f64-into-i32 slot mismatch.",
        float_to_string_index
    );
}
