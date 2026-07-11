//! Dashboard-shape canary — the CI smoke test the follow-up ticket
//! `54887260abf6` asked for:
//!
//! > "add a smoke test in their CI that literally builds this dashboard's
//! >  main.cln and curls the URLs — that would have caught the 0.33.44
//! >  regression before it shipped."
//!
//! We can't spin up MySQL + clean-server + the real dashboard in a
//! standard `cargo test` run. But the WASM the compiler emits is the
//! delivery contract for both regression classes:
//!
//!  * **Fingerprint #3957fce40e09 (originally 0.33.41-43):** `json.get`
//!    calls whose first arg comes from `db.query` were crashing at runtime
//!    with "String read out of bounds". Whatever fixes this must NOT
//!    reintroduce spurious boxing before the `expand_strings` wrapper.
//!  * **Fingerprint #54887260abf6 (0.33.44):** the previous attempt at
//!    fixing #3957fce4 added call-site BoxAny for `json.get`'s first arg.
//!    Because `_json_get` in frame.server plugin.toml is
//!    `expand_strings=true`, the wrapper read the box's tag byte (4) as
//!    the string length and forwarded `(box+4, 4)` to the host. Nine of
//!    ten URL patterns regressed.
//!
//! Both classes leave a visible fingerprint in the compiled WAT. The
//! assertions below inspect the dashboard-shape canary's emitted WASM
//! and refuse to let either shape ship.
//!
//! ## What the canary covers
//!
//! `tests/cln/canaries/dashboard_shape.cln` mirrors the shape of
//! `clean-errors/app/server/pages/tasks.cln::tasks_list_page` — sequential
//! string concats, two `db.query` calls interleaved with them, and
//! `json.get` calls both before and inside a `while` loop with a
//! `rows_html` accumulator. That is the exact combination that
//! regressed on 0.33.41-44.
//!
//! ## Why WAT inspection instead of runtime execution
//!
//! Runtime execution needs the frame.server host bridges (`_db_query`,
//! `_http_respond`, MySQL connectivity, etc.) — none of which are
//! available in the compiler's CI runner. Building an in-repo mock host
//! would double the CI runtime and duplicate contract surface. WAT
//! inspection catches the shape at a lower cost with more targeted
//! assertions.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Try to locate a cln binary. Returns `None` if none exists — the tests
/// call `.expect_or_skip(...)` on this so `cargo test` in a fresh clone
/// gracefully reports "skipped" instead of panicking with cryptic
/// output. Mirrors the pattern in `test_canary_registry_coverage.rs`.
fn cln_binary_opt() -> Option<PathBuf> {
    let manifest_dir = repo_root();
    let candidate_release = manifest_dir.join("target").join("release").join("cln");
    if candidate_release.exists() {
        return Some(candidate_release);
    }
    let candidate_debug = manifest_dir.join("target").join("debug").join("cln");
    if candidate_debug.exists() {
        return Some(candidate_debug);
    }
    None
}

fn cln_binary() -> PathBuf {
    cln_binary_opt().unwrap_or_else(|| {
        panic!(
            "cln binary not found at target/release/cln or target/debug/cln — \
             run `cargo build --release --bin cln` before this test"
        )
    })
}

/// Compile the canary with `--plugins` (frame.data + frame.server) and
/// return the raw WASM bytes.
fn compile_dashboard_canary() -> Vec<u8> {
    let root = repo_root();
    let canary = root
        .join("tests")
        .join("cln")
        .join("canaries")
        .join("dashboard_shape.cln");
    let out = root
        .join("tests")
        .join("output")
        .join("dashboard_shape.wasm");
    let _ = std::fs::create_dir_all(out.parent().unwrap());

    let status = Command::new(cln_binary())
        .arg("compile")
        .arg("--plugins")
        .arg(&canary)
        .arg("--output")
        .arg(&out)
        .output()
        .expect("failed to invoke cln");

    assert!(
        status.status.success(),
        "cln compile failed for dashboard_shape.cln:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );

    std::fs::read(&out).expect("compiled WASM must exist")
}

/// Convert the WASM to WAT via the in-crate `wasm2wat` binary. Keeps the
/// test self-contained (no `wabt` or external dep required in CI).
fn wasm_to_wat(bytes: &[u8]) -> String {
    let tmp_wasm = std::env::temp_dir().join("dashboard_shape_canary.wasm");
    let tmp_wat = std::env::temp_dir().join("dashboard_shape_canary.wat");
    std::fs::write(&tmp_wasm, bytes).unwrap();

    // Invoke the crate's own wasm2wat binary. Falls back to the system
    // `wasm2wat` if the in-crate binary isn't present (e.g. running under
    // `cargo test` without the wasm2wat bin dep resolved).
    let manifest_dir = repo_root();
    let candidates = [
        manifest_dir.join("target/release/wasm2wat"),
        manifest_dir.join("target/debug/wasm2wat"),
    ];
    let bin = candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .unwrap_or_else(|| PathBuf::from("wasm2wat"));

    let status = Command::new(&bin)
        .arg(&tmp_wasm)
        .arg(&tmp_wat)
        .output()
        .expect("failed to invoke wasm2wat");

    assert!(
        status.status.success(),
        "wasm2wat failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );

    std::fs::read_to_string(&tmp_wat).unwrap()
}

/// Extract the numeric WASM function index for an export by name.
/// Returns `None` if the export is missing (which is itself a signal
/// worth surfacing — the canary would fail loudly upstream).
fn export_index(wat: &str, export_name: &str) -> Option<u32> {
    let needle = format!("(export \"{}\" (func ", export_name);
    let line = wat.lines().find(|l| l.contains(&needle))?;
    let start = line.find("(func ")? + "(func ".len();
    let rest = &line[start..];
    let end = rest.find(')')?;
    rest[..end].trim().parse().ok()
}

/// Slice out the body of a specific function (identified by its WASM
/// function index) from the WAT.
///
/// The wasm2wat output has two distinct `(func ...)` forms:
/// - `  (func (type N))` inside the function section — these are the
///   forward declarations that pin each function's type index. They are
///   NOT bodies and must be skipped.
/// - `  (func` followed by `(local ...)` lines and then the actual
///   instruction stream — these ARE the bodies.
///
/// This function walks the WAT line-by-line, identifies the Nth
/// body-form `(func` (where N = defined_index = func_idx - import_count),
/// and returns everything up to the matching closing `  )` line at the
/// same indentation.
fn function_body_by_index(wat: &str, func_idx: u32) -> String {
    let import_count = wat.matches("  (import ").count();
    let defined_index = (func_idx as usize).saturating_sub(import_count);

    let lines: Vec<&str> = wat.lines().collect();
    let mut body_starts: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        // A body starts with exactly `  (func` (no extra tokens). The
        // function-section declarations `  (func (type N))` are on a
        // single line with the trailing `))` — they do not start a body.
        if line.trim_end() == "  (func" {
            body_starts.push(i);
        }
    }

    let start = body_starts.get(defined_index).copied().unwrap_or_else(|| {
        panic!(
            "function body #{} not found in WAT (defined_index={}, imports={}, total_body_starts={})",
            func_idx,
            defined_index,
            import_count,
            body_starts.len()
        )
    });
    let end = body_starts
        .get(defined_index + 1)
        .copied()
        .unwrap_or(lines.len());

    lines[start..end].join("\n")
}

/// Count occurrences of `call N` (as a whole word) in a function body.
fn count_calls(body: &str, target: u32) -> usize {
    let needle = format!("call {}\n", target);
    let needle2 = format!("call {}\r\n", target);
    body.matches(&needle).count() + body.matches(&needle2).count()
}

/// Check whether a body contains the "BoxAny 12-byte" allocation pattern.
///
/// The pattern emitted by `emit_box_value` (see
/// `codegen/mir_codegen/instructions.rs`) is:
///
///     i32.const 0     ;; type_id
///     i32.const 12    ;; size (tag + i32 payload + i32 payload)
///     call <mem_alloc>
///
/// We look for the `i32.const 12` immediately preceded by `i32.const 0`
/// and followed by `call <mem_alloc>`. wasm2wat normalizes the newlines
/// so a simple substring search over the compact form is reliable.
fn count_box_any_pattern(body: &str, mem_alloc_idx: u32) -> usize {
    // wasm2wat inserts newlines between each instruction; the pattern
    // is three consecutive lines. We normalize whitespace and search.
    let compact: String = body.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let pattern = format!("i32.const 0\ni32.const 12\ncall {}", mem_alloc_idx);
    compact.matches(&pattern).count()
}

#[test]
fn dashboard_shape_canary_compiles_with_plugins() {
    if cln_binary_opt().is_none() {
        eprintln!(
            "skipping: cln binary missing — build with `cargo build --release --bin cln` \
             before running the dashboard-shape canary tests"
        );
        return;
    }
    // First contract: the canary must compile. If this fails, the
    // canary itself is broken and the shape assertions below are
    // meaningless.
    let wasm = compile_dashboard_canary();
    assert!(!wasm.is_empty());
    assert!(wasm.starts_with(&[0x00, 0x61, 0x73, 0x6d]), "WASM magic");
}

#[test]
fn dashboard_page_does_not_box_expand_strings_wrapper_args() {
    if cln_binary_opt().is_none() {
        eprintln!("skipping: cln binary missing");
        return;
    }
    // Fingerprint #54887260abf6 — 0.33.44 regression class.
    //
    // `json.get` and `db.query` resolve to plugin `expand_strings=true`
    // wrappers that expect raw length-prefixed Clean strings. A regression
    // that reintroduces spurious BoxAny before those calls will produce a
    // 12-byte malloc followed immediately by tag=4 / ptr / 0 stores and
    // then the wrapper call. If the number of `mem_alloc(12)` allocations
    // inside `dashboard_page` is anywhere close to the number of
    // (json.get + db.query) calls, the regression is back.
    //
    // In the working 0.33.45+ codegen, `dashboard_page` has ZERO 12-byte
    // BoxAny mallocs — arguments flow straight to the wrapper as raw
    // string pointers.
    let wasm = compile_dashboard_canary();
    let wat = wasm_to_wat(&wasm);

    let dashboard_idx =
        export_index(&wat, "dashboard_page").expect("dashboard_page must be exported");
    let mem_alloc_idx = export_index(&wat, "mem_alloc").expect("mem_alloc must be exported");

    let body = function_body_by_index(&wat, dashboard_idx);
    let box_count = count_box_any_pattern(&body, mem_alloc_idx);

    assert_eq!(
        box_count, 0,
        "dashboard_page emitted {} BoxAny(12) allocations. \
         Any non-zero count means a codegen change re-introduced call-site \
         boxing before an expand_strings=true bridge wrapper — the same \
         class of failure that shipped as 0.33.44 fingerprint #54887260abf6 \
         and regressed 9 of 10 dashboard URLs. Consult \
         `resolves_to_expand_strings_wrapper` in \
         `src/codegen/mir_codegen/utilities.rs` and inspect the debug_assert! \
         guard in `instructions.rs::generate_call_instruction` — one of them \
         should have caught this before the test.",
        box_count
    );
}

#[test]
fn dashboard_page_calls_json_get_and_db_query_via_plugin_wrappers() {
    if cln_binary_opt().is_none() {
        eprintln!("skipping: cln binary missing");
        return;
    }
    // Positive-shape assertion: the canary IS supposed to route through
    // the plugin wrappers, so the emitted WAT must contain calls to
    // BOTH json.get and db.query. If either export is missing, the
    // plugin bridge registration broke — that would silently make the
    // "no boxing" assertion above pass by producing an empty function.
    let wasm = compile_dashboard_canary();
    let wat = wasm_to_wat(&wasm);

    let dashboard_idx =
        export_index(&wat, "dashboard_page").expect("dashboard_page must be exported");
    let json_get_idx = export_index(&wat, "json.get")
        .expect("json.get must be exported (plugin bridge wrapper registration failed?)");
    let db_query_idx = export_index(&wat, "db.query")
        .expect("db.query must be exported (plugin bridge wrapper registration failed?)");

    let body = function_body_by_index(&wat, dashboard_idx);
    let json_get_calls = count_calls(&body, json_get_idx);
    let db_query_calls = count_calls(&body, db_query_idx);

    // dashboard_page.cln has 4 json.get calls (line 843's + 3 inside the loop) and 2 db.query calls.
    // Exact counts depend on codegen — allow ≥ minimums so the test survives
    // benign codegen changes but fails loudly if the plugin bridges vanish.
    assert!(
        json_get_calls >= 4,
        "dashboard_page must call json.get at least 4 times (found {}). \
         Fewer calls means the compiler stripped some or re-routed them \
         to a different function — either the fix for #3957fce4 landed \
         and silently changed the ABI, or the plugin bridge wiring broke.",
        json_get_calls
    );
    assert!(
        db_query_calls >= 2,
        "dashboard_page must call db.query at least 2 times (found {}). \
         Fewer calls means the codegen dropped db.query calls or re-wired \
         them elsewhere.",
        db_query_calls
    );
}
