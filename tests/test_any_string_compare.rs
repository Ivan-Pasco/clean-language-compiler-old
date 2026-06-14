//! Regression test for ANY_STRING_COMPARE_USES_POINTER
//! (fingerprint `965e0b395e8154202415496c89613c9269e34771a034c94899adbe5c8561899e`,
//! reported against compiler 0.30.276).
//!
//! Background: when a binary comparison operator (`==` or `!=`) has one
//! operand of static type `any` (e.g. the result of field access on an
//! `any`-typed receiver: `data.kind`) and the other operand is a string
//! literal, the codegen dispatched to `string_compare` with the boxed
//! any-struct pointer as the first argument. The first 4 bytes of that
//! struct are the type tag (4 for string), and `string_compare` interprets
//! them as the string's length — so the comparison silently never matches
//! and the `!=` branch silently always matches.
//!
//! Direct sibling of ANY_INT_COMPARE_USES_POINTER. The fix in 0.30.278
//! extended `emit_unbox_any` for Integer/Number/Boolean targets but
//! excluded String. This fix adds `emit_any_to_string` in the MIR builder
//! and routes the String case through `MirOperation::AnyToString`, which
//! the codegen lowers to an inline runtime tag-dispatch block before the
//! BinaryOp.
//!
//! This test compiles a minimal repro and walks the emitted WASM. For
//! each `string_compare` call site, it checks that a tag-load
//! (`I32Load { offset: 0 }`) appears somewhere between the previous
//! `string_compare` (or function start) and this one — that's the
//! fingerprint of the AnyToString dispatch block. The pre-fix bug shape
//! is a `string_compare` reached without any intervening tag-load.
//!
//! The repro has three any-vs-string comparisons (`== "user"`,
//! `== "admin"`, `!= "user"`). After the fix all three must be
//! tag-dispatched.

use std::fs;
use tempfile::TempDir;
use wasmparser::{Operator, Parser as WasmParser, Payload};

use clean_language_compiler::MemoryTier;

const REPRO: &str = r#"start:
	any data = get_data()
	if data.kind == "user"
		print("BRANCH_USER\n")
	if data.kind == "admin"
		print("BRANCH_ADMIN\n")
	if data.kind != "user"
		print("BRANCH_NOT_USER\n")

functions:
	any get_data()
		return "{\"kind\": \"user\"}"
"#;

#[test]
fn any_field_string_compare_dispatches_on_tag_before_string_compare() {
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
        panic!("repro compile must succeed");
    });

    // Resolve the function index of `string_compare`. It can live either in
    // the import section (host bridge) or as a regular module function with
    // a name in the name section.
    let mut string_compare_idx: Option<u32> = None;
    let mut imported_func_count: u32 = 0;

    for payload in WasmParser::new(0).parse_all(&wasm) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let Ok(import) = import else { continue };
                if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                    if string_compare_idx.is_none()
                        && (import.name == "string_compare" || import.name == "_string_compare")
                    {
                        string_compare_idx = Some(imported_func_count);
                    }
                    imported_func_count += 1;
                }
            }
        }
    }

    if string_compare_idx.is_none() {
        // Fall back to the name section for module-internal functions.
        for payload in WasmParser::new(0).parse_all(&wasm) {
            if let Ok(Payload::CustomSection(reader)) = payload {
                if reader.name() == "name" {
                    use wasmparser::{Name, NameSectionReader};
                    let name_reader = NameSectionReader::new(reader.data(), reader.data_offset());
                    for subsection in name_reader {
                        let Ok(Name::Function(fname_reader)) = subsection else {
                            continue;
                        };
                        for naming in fname_reader {
                            let Ok(n) = naming else { continue };
                            if n.name == "string_compare" || n.name == "_string_compare" {
                                string_compare_idx = Some(n.index);
                                break;
                            }
                        }
                        if string_compare_idx.is_some() {
                            break;
                        }
                    }
                }
            }
        }
    }

    assert!(
        string_compare_idx.is_some(),
        "test setup error: could not locate `string_compare` in the WASM \
         imports or name section. The codegen has likely renamed it; \
         update this test to match."
    );
    let string_compare_idx = string_compare_idx.unwrap();

    // Walk every code section body. For each `Call(string_compare)`, check
    // whether a tag-load (`I32Load { offset: 0 }`) occurred earlier in the
    // body since the previous `Call(string_compare)`. The tag-load is the
    // signature of the AnyToString dispatch block emitted by the fix.
    //
    // Note: any_to_string also reads the value at offset 4 with later
    // I32Loads, but the leading "read tag at offset 0" is the cheapest and
    // most stable shape to match — it appears once per AnyToString call,
    // regardless of which branch of the tag-dispatch the codegen ends up
    // taking.
    let mut compares_with_tag_dispatch = 0usize;
    let mut bare_string_compares = 0usize;

    for payload in WasmParser::new(0).parse_all(&wasm) {
        let Ok(Payload::CodeSectionEntry(body)) = payload else {
            continue;
        };
        let Ok(mut reader) = body.get_operators_reader() else {
            continue;
        };

        let mut saw_tag_load_since_last_compare = false;

        loop {
            match reader.read() {
                Ok(Operator::I32Load { memarg }) => {
                    if memarg.offset == 0 {
                        saw_tag_load_since_last_compare = true;
                    }
                }
                Ok(Operator::Call { function_index }) => {
                    if function_index == string_compare_idx {
                        if saw_tag_load_since_last_compare {
                            compares_with_tag_dispatch += 1;
                        } else {
                            bare_string_compares += 1;
                        }
                        saw_tag_load_since_last_compare = false;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }

    assert!(
        compares_with_tag_dispatch >= 3,
        "expected at least 3 string_compare calls preceded by an \
         AnyToString tag-load dispatch (one per any-vs-string comparison \
         in the repro: `data.kind == \"user\"`, `data.kind == \"admin\"`, \
         `data.kind != \"user\"`), but found only {}. \
         bare-string-compare count: {}. This is the \
         ANY_STRING_COMPARE_USES_POINTER regression — the codegen is \
         passing the boxed any-pointer to string_compare instead of \
         converting it to a string first.",
        compares_with_tag_dispatch,
        bare_string_compares
    );
}
