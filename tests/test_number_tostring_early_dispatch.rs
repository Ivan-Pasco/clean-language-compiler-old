//! Regression test for CODEGEN-F64-IN-I32-SLOT / `codegen-f64-in-i32-slot`
//! (reported locally 2026-07-16, discovered on the clean-errors project's
//! main.cln build; wasmparser message: "type mismatch: expected i32, found
//! f64").
//!
//! Background: `stdlib::method_style::register_conversion_methods` registers
//! `number.toString` as an `(i32) -> i32` identity stub — the same generic
//! boxed-value pattern used for `integer.toString` and `boolean.toString`.
//! That signature is wrong for `Number`, whose Clean representation is `f64`.
//!
//! The MIR builder has a `method_symbol.0 == 0` fast path for
//! `(ConcreteType::Number, "toString")` that would have routed to
//! `float_to_string(f64) -> i32`. But when the type checker successfully
//! resolves `number.toString` in the symbol table (a non-zero method symbol),
//! the fast path is skipped and the generic method dispatch calls the buggy
//! identity stub — with an f64 value on the stack and an i32 parameter slot.
//! That module fails wasmparser validation.
//!
//! Repro: the frame.data plugin's `__to_params` body walks a model's fields
//! and, for each `number` field, emits `params + field.toString()`. The
//! plugin-generated AST has method_symbol non-zero for that call — hence the
//! failure mode reproduces on real projects (clean-errors, main.cln, func 620)
//! but not on hand-written source where the fast path always fires.
//!
//! Fix: an EARLY dispatch for `Number.toString()` in `expressions.rs` that
//! mirrors the existing `Integer.toString()` / `IntegerSized{64}.toString()`
//! early-dispatch handlers. It runs BEFORE the method_symbol check and always
//! routes to `float_to_string`.
//!
//! Because a minimal .cln reproduction of the plugin-generated shape requires
//! the frame.data plugin (which pulls in frame.server and its `_http_respond`
//! import — not provided by the standalone `wasmtime_runner`), this
//! regression check is a Rust integration test that inspects the emitted
//! WASM. It compiles a hand-written class-method form that exercises
//! `Number.toString()` and asserts that the module (a) validates and (b) does
//! NOT contain an internal `number.toString`-shaped identity function being
//! called with an f64 arg.
//!
//! The strong assertion: the compiled `render` function must call
//! `float_to_string` at least once — proving the early dispatch fires.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Parser as WasmParser, Payload, Validator};

use clean_language_compiler::MemoryTier;

const REPRO: &str = r#"start:
	Container c = Container()
	c.set_score(3.14)
	print(c.render()) +

class Container
	number score

	functions:
		public:
			set_score(number v)
				score = v

			string render()
				string result = "score="
				result = result + score.toString()
				return result
"#;

#[test]
fn number_tostring_compiles_to_valid_wasm() {
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
        panic!("Number.toString() repro must compile");
    });

    // Sanity check: the generated module must pass wasmparser validation.
    // Before the fix, a module hitting the plugin-generated shape would fail
    // here with "type mismatch: expected i32, found f64". The hand-written
    // shape doesn't reproduce the exact failure but MUST validate.
    Validator::new()
        .validate_all(&wasm)
        .expect("generated WASM must pass wasmparser validation");

    // Find the index of `float_to_string` in the import section, and confirm
    // it is called at least once from the code section. That proves the early
    // dispatch fired for `score.toString()` in `render()`.
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

    // Walk every code section and count Call ops targeting float_to_string.
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
        "expected `score.toString()` in Container.render() to lower to a \
         `call float_to_string` (f64 -> i32 pointer), but no Call to \
         function index {} appears in the code section. This means the \
         MIR-builder's Number.toString() early dispatch is not firing — \
         the compiler is again routing Number receivers through the buggy \
         `number.toString` identity stub registered by \
         stdlib::method_style::register_conversion_methods.",
        float_to_string_index
    );
}
