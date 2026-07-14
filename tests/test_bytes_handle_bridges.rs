//! Bytes-handle bridge contract tests.
//!
//! Guards the compiler surface for `req.body_bytes()` and `fs.write_bytes()`
//! introduced to unblock the errors dashboard's tarball-upload endpoint.
//! Both bridges use the opaque-handle convention documented in
//! `foundation/spec/type-system.md` §9b — until a `bytes` primitive lands,
//! the handle is typed as `integer` at the language level and holds a
//! pointer to a `[4-byte LE length][bytes]` buffer.
//!
//! The tests here verify:
//!   1. `fs.write_bytes(path, handle)` type-checks with `(string, integer)
//!      -> integer` and compiles to the canonical `env._fs_write_bytes`
//!      WASM import.
//!   2. The compiler recognizes both dot-notation aliases (`fs.write_bytes`,
//!      `req.body_bytes`) — no SEM007 "undefined variable" for the `fs`
//!      namespace, no dual emission of dot-notation imports.
//!   3. `req.body_bytes()` is subject to the same CONC002 request-context
//!      rule as `req.body` — it's not a bug that it fails outside a
//!      handler; it's the language contract from `req_body`'s validation
//!      applying to the new sibling production.
//!
//! Note: an end-to-end tarball-upload test that exercises `req.body_bytes()`
//! inside a real endpoint handler lives at
//! `tests/cln/plugins/frame-server/bytes_handle_round_trip.cln` — it needs
//! the frame.server plugin loaded, so it runs under the plugin-loading test
//! harness rather than here.

use wasmparser::{Parser as WasmParser, Payload};

fn list_import_names(wasm_bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in WasmParser::new(0).parse_all(wasm_bytes) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                let import = import.expect("valid import");
                names.push(import.name.to_string());
            }
        }
    }
    names
}

#[test]
fn fs_write_bytes_compiles_and_emits_canonical_import() {
    // Using a literal integer as the handle keeps this test out of the
    // req.body_bytes / CONC002 territory so we're isolating the fs.*
    // surface. In production the handle originates from req.body_bytes()
    // inside a handler; the round-trip fixture in
    // tests/cln/plugins/frame-server/ covers that flow.
    let source = r#"start:
	integer h = 0
	integer rc = fs.write_bytes("/tmp/x.bin", h)
	print(rc.toString())
"#;

    let wasm = match clean_language_compiler::compile(source) {
        Ok(w) => w,
        Err(errors) => {
            panic!(
                "fs.write_bytes must compile with (string, integer) -> integer \
                 signature. Errors: {:?}",
                errors
            );
        }
    };

    let imports = list_import_names(&wasm);

    // Canonical name must be present.
    assert!(
        imports.iter().any(|n| n == "_fs_write_bytes"),
        "WASM must import `env._fs_write_bytes` (canonical). Got: {imports:?}",
    );

    // Dot-notation alias must NOT appear as a separate WASM import — that
    // would be dual emission (see tests/test_dual_naming_imports.rs).
    assert!(
        !imports.iter().any(|n| n == "fs.write_bytes"),
        "WASM must NOT import dot-notation `fs.write_bytes` — dual emission \
         is a bug. Got: {imports:?}",
    );
}

#[test]
fn fs_write_bytes_returns_integer_error_code() {
    // fs.write_bytes returns integer per spec/plugins/frame-server.ebnf
    // `fs_write_bytes` (0 = success, non-zero error code) — must
    // type-check against `== 0`.
    let source = r#"start:
	integer h = 0
	integer rc = fs.write_bytes("/tmp/x.bin", h)
	if rc == 0
		print("ok")
	else
		print("err")
"#;

    assert!(
        clean_language_compiler::compile(source).is_ok(),
        "fs.write_bytes must return `integer` so `rc == 0` type-checks. See \
         spec/type-system.md §9b for the error-code convention.",
    );
}

#[test]
fn fs_namespace_is_registered() {
    // Sanity check for the resolver: the `fs` namespace must be a
    // recognized identifier so `fs.write_bytes(...)` resolves without a
    // SEM007 "Undefined variable 'fs'" error. This regressed at least once
    // during development because the namespace was missing from the
    // hardcoded builtin list in `symbol_table.rs`.
    let source = r#"start:
	integer rc = fs.write_bytes("/tmp/x", 0)
	print(rc.toString())
"#;

    let result = clean_language_compiler::compile(source);
    if let Err(errors) = &result {
        for e in errors {
            let msg = format!("{:?}", e);
            assert!(
                !msg.contains("Undefined variable 'fs'"),
                "Compiler must recognize the `fs` namespace. Got error: {msg}",
            );
        }
    }
    assert!(
        result.is_ok(),
        "fs.write_bytes minimal call must compile. Errors: {:?}",
        result.err(),
    );
}

#[test]
fn req_body_bytes_conc002_matches_req_body_behavior() {
    // req.body_bytes() is subject to the same CONC002 request-context rule
    // as req.body — outside a handler both must error. This is not a bug;
    // it's the point of CONC002 (request-context builtins are only valid
    // inside route handlers). The test guards that the new bridge is
    // recognized by the CONC002 validator (not silently allowed everywhere).
    //
    // Once the frame.server end-to-end fixture runs
    // (tests/cln/plugins/frame-server/bytes_handle_round_trip.cln), the
    // in-handler case is covered.
    let source = r#"start:
	integer h = req.body_bytes()
	print(h.toString())
"#;

    let result = clean_language_compiler::compile(source);
    match result {
        Ok(_) => panic!(
            "req.body_bytes() outside a request handler must trigger \
             CONC002, the same rule that governs req.body. If this test \
             passes, either the validator now recognizes it as a \
             non-request-context call (wrong) or it was skipped."
        ),
        Err(errors) => {
            let has_conc002 = errors.iter().any(|e| {
                let s = format!("{:?}", e);
                s.contains("CONC002")
            });
            assert!(
                has_conc002,
                "Expected CONC002 error for req.body_bytes() outside handler. Got: {:?}",
                errors
            );
        }
    }
}
