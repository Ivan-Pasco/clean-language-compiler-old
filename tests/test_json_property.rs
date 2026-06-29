//! Property-based tests for JSON stdlib (`json.textToData`, `json.dataToText`,
//! `json.get`).
//!
//! These tests drive Clean Language source through the real compile → execute
//! pipeline (via `compile_multi_file_with_memory_tier` + `wasmtime_runner`).
//! End-to-end is required because the JSON parser/serialiser live entirely in
//! generated WASM — there is no Rust-side `json::parse(&str)` entry point —
//! and the historical JSON bugs (parse-cache leak, dotpath traversal, Any
//! unbox-on-return) only surface through the full pipeline.
//!
//! Each test is heavy (one compile + one wasmtime run per generated case), so
//! the suite is gated behind `CLEAN_RUN_JSON_PROPERTY_TESTS=1`. Default
//! `cargo test` skips them; the dedicated CI coverage job and the nightly
//! corpus job set the env var.

#![cfg(test)]

use std::path::PathBuf;
use std::process::Command;

use clean_language_compiler::{compile_multi_file_with_memory_tier, MemoryTier};
use proptest::prelude::*;

fn enabled() -> bool {
    matches!(
        std::env::var("CLEAN_RUN_JSON_PROPERTY_TESTS").as_deref(),
        Ok("1") | Ok("true")
    )
}

fn compile_source_to_wasm(source: &str, label: &str) -> PathBuf {
    let tmp_dir = std::env::temp_dir().join("clean_json_property");
    std::fs::create_dir_all(&tmp_dir).expect("create tmp dir");
    let src_path = tmp_dir.join(format!("{label}.cln"));
    let wasm_path = tmp_dir.join(format!("{label}.wasm"));
    std::fs::write(&src_path, source).expect("write source");

    let search_paths = vec![tmp_dir.clone()];
    let (wasm, _state) = compile_multi_file_with_memory_tier(
        &src_path,
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
        panic!("generated JSON source must compile cleanly:\n---\n{source}\n---");
    });

    std::fs::write(&wasm_path, wasm).expect("write wasm");
    wasm_path
}

fn run_wasm(wasm_path: &PathBuf) -> String {
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        PathBuf::from(format!("{}/release/wasmtime_runner", target_dir)),
        PathBuf::from(format!("{}/debug/wasmtime_runner", target_dir)),
        manifest_dir.join(format!("{}/release/wasmtime_runner", target_dir)),
        manifest_dir.join(format!("{}/debug/wasmtime_runner", target_dir)),
    ];
    let runner = candidates
        .iter()
        .find(|p| p.exists())
        .expect("wasmtime_runner must be built — `cargo build --bin wasmtime_runner`");

    let output = Command::new(runner)
        .arg(wasm_path)
        .output()
        .expect("invoke wasmtime_runner");

    assert!(
        output.status.success(),
        "wasmtime_runner exited non-zero ({}):\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Pull the actual program stdout from the wasmtime_runner wrapped output.
///
/// `wasmtime_runner` decorates the program's output with banner lines:
/// ```text
/// 🚀 Loading WebAssembly file: …
/// 📦 File size: … bytes
/// ✅ WebAssembly module loaded successfully
/// 📋 Exported functions: […]
/// 🎯 Executing start function...
/// --- Output ---
/// <program stdout here>--- End Output ---
/// ✅ Execution completed successfully!
/// ```
/// Asserting against the full output catches the wrapper banners and breaks
/// trivially. Extract the substring between the `--- Output ---\n` opener
/// and `--- End Output ---` closer; if either marker is missing, fall back
/// to the raw output so the assertion's failure message stays informative.
fn extract_program_output(raw: &str) -> String {
    let Some(start) = raw.find("--- Output ---\n") else {
        return raw.to_string();
    };
    let body_start = start + "--- Output ---\n".len();
    let Some(end_rel) = raw[body_start..].find("--- End Output ---") else {
        return raw[body_start..].to_string();
    };
    raw[body_start..body_start + end_rel].to_string()
}

/// Escape an arbitrary string for safe embedding inside a Clean Language double-quoted
/// string literal. Mirrors the minimum subset of RFC 8259 the generated JSON literals
/// need.
fn escape_for_cln_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// JSON-safe strings that survive a round-trip through both Clean's string
/// literal escaper and the JSON encoder. Restricted to printable ASCII (minus
/// quote/backslash) to keep failures attributable to JSON semantics rather
/// than lexer / encoding quirks.
fn safe_string_strategy() -> impl Strategy<Value = String> {
    "[A-Za-z0-9 _\\-]{0,20}".prop_map(String::from)
}

fn safe_key_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,8}".prop_map(String::from)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,                  // each case = one compile + one wasmtime run
        max_shrink_iters: 8,
        ..ProptestConfig::default()
    })]

    /// Property: `json.get(j, "k")` must return the same value no matter how
    /// many times it is called against the same `j`. Drives the parse-cache
    /// path that leaked memory pre-0.30.369 (CMP-SSR-MALLOC-OOM-CONDITIONAL-HELPER).
    /// 1000 iterations on a non-trivial JSON exercises the cache without
    /// timing out CI.
    #[test]
    fn prop_json_get_is_idempotent(values in proptest::collection::vec(safe_string_strategy(), 1..6)) {
        if !enabled() {
            return Ok(());
        }

        // Build a JSON array of {"k": "<value>"} objects from the strategy.
        let mut json_items = Vec::with_capacity(values.len());
        for v in &values {
            json_items.push(format!("{{\\\"k\\\":\\\"{}\\\"}}", escape_for_cln_literal(v)));
        }
        let json_literal = format!("[{}]", json_items.join(","));
        let expected = values[0].clone();

        let source = format!(
r#"start:
	string j = "{json_literal}"
	integer i = 0
	string last = ""
	while i < 1000
		last = json.get(j, "0.k")
		i = i + 1
	print(last) +
"#
        );

        let label = format!("idempotent_{}", values.len());
        let wasm = compile_source_to_wasm(&source, &label);
        let raw = run_wasm(&wasm);
        let out = extract_program_output(&raw);

        // Cannot use `trim_end()` here — the strategy generates strings that
        // may end in whitespace, and trimming the program output before the
        // ends_with check would falsely reject them. The print() + form
        // appends one trailing '\n' that we strip explicitly.
        let trimmed = out.strip_suffix('\n').unwrap_or(&out);
        prop_assert!(
            trimmed.ends_with(&expected),
            "json.get must be idempotent: expected trailing {expected:?}, got {out:?}"
        );
    }

    /// Property: `json.get` against valid array indices returns the value at
    /// that index. Guards against the regression where indices > 0 on object
    /// arrays returned the first element (5325ab5c).
    #[test]
    fn prop_json_get_index_picks_correct_element(values in proptest::collection::vec(safe_string_strategy(), 2..6)) {
        if !enabled() {
            return Ok(());
        }

        let mut json_items = Vec::with_capacity(values.len());
        for v in &values {
            json_items.push(format!("{{\\\"k\\\":\\\"{}\\\"}}", escape_for_cln_literal(v)));
        }
        let json_literal = format!("[{}]", json_items.join(","));

        let last_index = values.len() - 1;
        let expected = values[last_index].clone();

        let source = format!(
r#"start:
	string j = "{json_literal}"
	string v = json.get(j, "{last_index}.k")
	print(v) +
"#
        );

        let label = format!("index_{}", values.len());
        let wasm = compile_source_to_wasm(&source, &label);
        let raw = run_wasm(&wasm);
        let out = extract_program_output(&raw);

        // Same trailing-whitespace concern as prop_json_get_is_idempotent —
        // strip only the single trailing newline emitted by `print(…) +`.
        let trimmed = out.strip_suffix('\n').unwrap_or(&out);
        prop_assert!(
            trimmed.ends_with(&expected),
            "json.get index {last_index} must select that element: expected {expected:?}, got {out:?}"
        );
    }

    /// Property: `json.dataToText(json.textToData(s))` round-trips simple object
    /// values back to a string containing every original value. The round-trip
    /// is the canonical guard for the Any unbox-on-return path that broke
    /// twice in 2026.
    #[test]
    fn prop_json_round_trip_preserves_values(
        key in safe_key_strategy(),
        value in safe_string_strategy(),
    ) {
        if !enabled() {
            return Ok(());
        }

        let key_esc = escape_for_cln_literal(&key);
        let val_esc = escape_for_cln_literal(&value);

        let source = format!(
r#"start:
	string j = "{{\"{key_esc}\":\"{val_esc}\"}}"
	any parsed = json.textToData(j)
	string back = json.dataToText(parsed)
	print(back) +
"#
        );

        let label = format!("roundtrip_{}_{}", key.len(), value.len());
        let wasm = compile_source_to_wasm(&source, &label);
        let raw = run_wasm(&wasm);
        let out = extract_program_output(&raw);

        prop_assert!(
            out.contains(&key) && out.contains(&value),
            "round-trip must preserve key {key:?} and value {value:?}; got {out:?}"
        );
    }
}
