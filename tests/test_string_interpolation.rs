//! Regression test for SEM-INTERP-UNDEFINED (dashboard fp 224922a73397).
//!
//! Before the fix, `Expression::StringInterpolation` had no match arm in
//! `HirBuilder::build_expression` — every interpolated literal hit the
//! catch-all `_ => HirExpression::Literal { value: Value::Void }` and
//! silently became an empty string at runtime, with no compile-time
//! diagnostic, even when the interpolated identifier was undefined.
//!
//! The fix lowers `"hello {x}"` to a left-folded concatenation
//! `"hello " + x.toString()`, building each interpolated subexpression
//! through `build_expression` like any other use. The resolver then
//! visits `x` and emits SEM006/SEM007 if it can't bind, matching how
//! `print(undefined_x)` already behaves.
//!
//! This test pins both halves of the fix:
//!   1. A defined identifier interpolates to its `toString()` form
//!      (positive path — companion fixture
//!      `tests/cln/strings/interpolation.cln`).
//!   2. An undefined identifier produces a compile error at semantic
//!      analysis instead of a silent empty string at runtime.

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use std::path::PathBuf;
use std::process::Command;

fn compile_to_wasm(source_path: &str, out_path: &str) {
    let source = PathBuf::from(source_path);
    let search_paths = vec![source.parent().expect("source has parent").to_path_buf()];

    let (wasm, _build_state) = compile_multi_file_with_memory_tier(
        &source,
        search_paths,
        2,
        None,
        MemoryTier::Standard,
        false,
    )
    .unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!("regression source {source_path} must compile cleanly");
    });

    std::fs::write(out_path, wasm).expect("write wasm");
}

fn run_via_wasmtime_runner(wasm_path: &str) -> String {
    // Same release/debug fallback pattern as the other regression tests.
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        PathBuf::from(format!("{}/release/wasmtime_runner", target_dir)),
        PathBuf::from(format!("{}/debug/wasmtime_runner", target_dir)),
        manifest_dir.join(format!("{}/release/wasmtime_runner", target_dir)),
        manifest_dir.join(format!("{}/debug/wasmtime_runner", target_dir)),
    ];
    let runner = candidates.iter().find(|p| p.exists()).unwrap_or_else(|| {
        panic!(
            "wasmtime_runner not built — `cargo build --bin wasmtime_runner` (debug) or \
             `cargo build --release --bin wasmtime_runner`. Looked in: {candidates:?}"
        )
    });

    let output = Command::new(runner)
        .arg(wasm_path)
        .output()
        .expect("invoke wasmtime_runner");

    assert!(
        output.status.success(),
        "wasmtime_runner exited non-zero ({}):\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn string_interpolation_concatenates_via_to_string() {
    // Positive path: every interpolated identifier resolves and the
    // surrounding text is preserved verbatim.
    let out = std::env::temp_dir().join("string_interpolation_regression.wasm");
    let out_str = out.to_string_lossy().to_string();
    compile_to_wasm("tests/cln/strings/interpolation.cln", &out_str);
    let output = run_via_wasmtime_runner(&out_str);

    // Before the fix, every line containing `{...}` would emit as a
    // bare empty string and `printl` would print a single blank line —
    // length 0, no interpolation, no error. The assertions below pin
    // both the literal text AND the interpolated values to catch
    // either type of regression.
    for expected in [
        "hello world",
        "answer is 42",
        "ratio is 3.14",
        "trailing world",
        "between A and B",
        "no interps here",
    ] {
        assert!(
            output.contains(expected),
            "SEM-INTERP-UNDEFINED regression: expected line `{expected}` missing from output. \
             If the interpolation silently became empty again, you'll see surrounding text \
             with the {{name}} placeholder dropped. Output was:\n{output}"
        );
    }
}

#[test]
fn string_interpolation_undefined_identifier_is_a_compile_error() {
    // Negative path: SEM-INTERP-UNDEFINED's specific symptom — an
    // undefined identifier inside `{...}` must surface at compile time,
    // not become an empty string at runtime.
    let tmp = std::env::temp_dir().join("interp_undefined_regression.cln");
    std::fs::write(
        &tmp,
        "start:\n\tstring s = \"hello {undefined_xyz}\"\n\tprintl(s)\n",
    )
    .expect("write tmp source");

    let search_paths = vec![tmp.parent().expect("tmp has parent").to_path_buf()];
    let result = compile_multi_file_with_memory_tier(
        &tmp,
        search_paths,
        2,
        None,
        MemoryTier::Standard,
        false,
    );

    match result {
        Ok(_) => panic!(
            "SEM-INTERP-UNDEFINED regression: compile succeeded for a string literal \
             containing an interpolation of an undefined identifier (`undefined_xyz`). \
             The expected behaviour is a SEM006/SEM007 at compile time. Before the fix, \
             the catch-all `_ => Void` in HirBuilder::build_expression swallowed the \
             interpolation entirely and the runtime emitted an empty string."
        ),
        Err(errors) => {
            let combined: String = errors.iter().map(|e| format!("{e}")).collect();
            assert!(
                combined.contains("undefined_xyz"),
                "Expected the compile error to name the undefined identifier `undefined_xyz`; \
                 got:\n{combined}"
            );
            // The error should be of the "undefined identifier" family. The
            // CLI surfaces it as `error[SEM007]:` but the direct API returns
            // the simpler `Validation error: Undefined variable '<name>'`
            // shape. Accept either — the diagnostic exists and names the
            // unresolved identifier, which is what matters semantically.
            assert!(
                combined.contains("Undefined variable")
                    || combined.contains("not found")
                    || combined.contains("SEM006")
                    || combined.contains("SEM007"),
                "Expected an undefined-identifier diagnostic for an unbound \
                 interpolation target; got:\n{combined}"
            );
        }
    }

    let _ = std::fs::remove_file(&tmp);
}
