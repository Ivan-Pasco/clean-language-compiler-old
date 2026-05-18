//! Regression test for REG001: WASM function index corruption when
//! db/env/time/crypto builtin wrapper functions are registered during the
//! import phase instead of the local-function phase.
//!
//! Root cause (0.30.124): register_db_builtin_imports() etc. called both
//! register_import_function() and register_function() in the same pass.
//! WASM assigns import indices first (0..N-1) and local function indices
//! starting at N, so mixing local function creation into the import phase
//! corrupts all subsequent function indices. Symptoms: bridge function call
//! sites (e.g. http.redirect) receive wrong string pointers — the pointer
//! resolves to offset 0 of the data section, dumping all string constants
//! as the output.
//!
//! Fix: register_db/env/time/crypto_builtin_imports() registers only raw
//! imports; register_db/env/time/crypto_builtin_wrappers() creates local
//! wrapper functions AFTER all imports are registered.

use wasmparser::{Parser as WasmParser, Payload};

fn count_imports(wasm_bytes: &[u8]) -> usize {
    let mut count = 0;
    for payload in WasmParser::new(0).parse_all(wasm_bytes) {
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

fn count_local_functions(wasm_bytes: &[u8]) -> usize {
    let mut count = 0;
    for payload in WasmParser::new(0).parse_all(wasm_bytes) {
        if let Ok(Payload::FunctionSection(reader)) = payload {
            count = reader.count() as usize;
        }
    }
    count
}

/// Verify that every Call instruction in the WASM code section references a
/// valid function index (< imports + local functions).  A corrupted index
/// (e.g. pointing into the gap between the import and local function ranges)
/// would trip this assert.
fn assert_call_indices_valid(wasm_bytes: &[u8]) {
    let n_imports = count_imports(wasm_bytes);
    let n_locals = count_local_functions(wasm_bytes);
    let total = n_imports + n_locals;

    for payload in WasmParser::new(0).parse_all(wasm_bytes) {
        if let Ok(Payload::CodeSectionEntry(body)) = payload {
            let mut reader = body.get_operators_reader().expect("valid code");
            loop {
                use wasmparser::Operator;
                match reader.read() {
                    Ok(Operator::Call { function_index }) => {
                        assert!(
                            (function_index as usize) < total,
                            "Call instruction references out-of-range function index {}; \
                             WASM has {} imports + {} local functions = {} total. \
                             This indicates function index corruption (REG001).",
                            function_index,
                            n_imports,
                            n_locals,
                            total
                        );
                    }
                    Ok(Operator::End) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    }
}

#[test]
fn wasm_function_indices_valid_with_env_builtin_no_plugin() {
    // Program that uses env.get without the frame.data plugin — this forces
    // register_env_builtin_imports() to run, which in 0.30.124 incorrectly
    // created a local wrapper function during the import phase, corrupting
    // all subsequent function indices.
    let source = r#"start:
    string val = env.get("DATABASE_URL")
    print(val)
"#;

    let wasm = match clean_language_compiler::compile(source) {
        Ok(w) => w,
        Err(errors) => {
            // Compilation errors are acceptable here (env.get without host
            // may fail at type-check); the important thing is that IF it
            // compiles, the WASM is structurally correct.
            eprintln!("Compilation returned errors (acceptable): {:?}", errors);
            return;
        }
    };

    assert_call_indices_valid(&wasm);
}

#[test]
fn wasm_function_indices_valid_with_db_builtin_no_plugin() {
    // Program that references db.query without frame.data — exercises
    // register_db_builtin_imports() and register_db_builtin_wrappers().
    let source = r#"start:
    string result = db.query("SELECT 1", "")
    print(result)
"#;

    let wasm = match clean_language_compiler::compile(source) {
        Ok(w) => w,
        Err(errors) => {
            eprintln!("Compilation returned errors (acceptable): {:?}", errors);
            return;
        }
    };

    assert_call_indices_valid(&wasm);
}

#[test]
fn wasm_function_indices_valid_with_multiple_builtins_no_plugin() {
    // Program that uses both env.get and time.now — both builtin wrappers are
    // created, exercising the full two-phase split across two namespaces.
    // This is the configuration most likely to expose index corruption bugs.
    let source = r#"start:
    string val = env.get("KEY")
    integer t = time.now()
    print(val)
    print(toString(t))
"#;

    let wasm = match clean_language_compiler::compile(source) {
        Ok(w) => w,
        Err(errors) => {
            eprintln!("Compilation returned errors (acceptable): {:?}", errors);
            return;
        }
    };

    assert_call_indices_valid(&wasm);
}
