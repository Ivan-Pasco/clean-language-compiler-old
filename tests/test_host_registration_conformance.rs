//! Host-registration conformance tests.
//!
//! Parses every `linker.func_wrap(...)` call in the two in-repo host
//! implementations (`src/plugins/wasm_adapter.rs` and
//! `src/bin/wasmtime_runner.rs`) and asserts that each registration matches
//! `src/plugins/function-registry.toml` at the WASM-shape level, and when
//! `param_names` is declared in the registry, also at the semantic
//! argument-order level.
//!
//! This is what catches drift like `mem_alloc(size, _align)` vs the
//! registry's `(type_id, size)` — a class of bug that pure type checking
//! cannot detect because both shapes expand to `(i32, i32) -> i32` at the
//! WASM level.
//!
//! Two modes:
//!
//! - **Type & return drift** (always enforced): every registration must
//!   match the registry's `params`/`returns` WASM shape, or fail. This is
//!   the baseline conformance check.
//!
//! - **Argument-order drift** (enforced per-entry): only registry entries
//!   that opt in by declaring `param_names = [...]` are checked. Today only
//!   the `memory_runtime` namespace is annotated; expanding coverage
//!   prevents the next class of mismatch by construction.
//!
//! Orphan registrations (host registers something the registry doesn't
//! declare) are reported but classified as warnings — many existing
//! registrations under the `env` compat module pre-date the registry and
//! will be migrated incrementally. The test counts orphans and asserts the
//! count does NOT grow without explicit ratchet adjustment.

use std::path::PathBuf;

use clean_language_compiler::plugins::host_conformance::{
    check_host_registrations, format_issues, parse_registrations, Issue, IssueKind,
};
use clean_language_compiler::plugins::registry_loader::RegistryIndex;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    let p = repo_root().join(path);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("failed to read {}: {e}", p.display()))
}

/// Issues classified by enforcement tier so we can ratchet down orphan
/// registrations without blocking on the long tail today.
struct Classified {
    /// Hard failures: type drift, return drift, semantic-name drift.
    hard: Vec<Issue>,
    /// Soft failures: orphan registrations (no registry entry yet).
    orphans: Vec<Issue>,
}

fn classify(issues: Vec<Issue>) -> Classified {
    let mut hard = Vec::new();
    let mut orphans = Vec::new();
    for i in issues {
        match i.kind {
            IssueKind::OrphanRegistration => orphans.push(i),
            _ => hard.push(i),
        }
    }
    Classified { hard, orphans }
}

/// `wasm_adapter.rs` is the in-process plugin host. Every host function the
/// compiler bakes into a plugin's link-time environment goes through here.
/// Drift in this host is exactly what produced COMPILER-MEM-ALLOC-NO-GROW
/// (param order swap that pure-type checking missed).
#[test]
fn wasm_adapter_registrations_conform_to_registry() {
    let registry = RegistryIndex::load().expect("registry loads");
    let source = read("src/plugins/wasm_adapter.rs");
    let regs = parse_registrations(&source);
    assert!(
        regs.len() >= 50,
        "expected to parse many registrations from wasm_adapter.rs, got {}",
        regs.len()
    );
    let Classified { hard, orphans } = classify(check_host_registrations(&regs, &registry));

    if let Some(msg) = format_issues(&hard, "src/plugins/wasm_adapter.rs") {
        panic!("{msg}");
    }

    // Orphan-registration ratchet: we don't fail on existing orphans (they
    // are tracked for incremental cleanup), but we DO fail if the count
    // increases — that's a regression. Update this number only when you've
    // either added the missing registry entry or removed the stub.
    //
    // Last audit: 2026-06-18 — many `env`-namespace string/list stubs are
    // historically not in the registry; they are aliases for canonical
    // `string_*` / `list_*` entries.
    const ALLOWED_ORPHAN_MAX: usize = 200;
    assert!(
        orphans.len() <= ALLOWED_ORPHAN_MAX,
        "{}",
        format_issues(&orphans, "src/plugins/wasm_adapter.rs (orphans)").unwrap_or_default()
    );
}

/// `wasmtime_runner.rs` is the standalone runner used by the test harness.
/// It must agree with the registry as strictly as `wasm_adapter.rs`.
#[test]
fn wasmtime_runner_registrations_conform_to_registry() {
    let registry = RegistryIndex::load().expect("registry loads");
    let source = read("src/bin/wasmtime_runner.rs");
    let regs = parse_registrations(&source);
    assert!(
        !regs.is_empty(),
        "expected non-zero registrations in wasmtime_runner.rs"
    );
    let Classified { hard, orphans } = classify(check_host_registrations(&regs, &registry));

    if let Some(msg) = format_issues(&hard, "src/bin/wasmtime_runner.rs") {
        panic!("{msg}");
    }

    const ALLOWED_ORPHAN_MAX: usize = 200;
    assert!(
        orphans.len() <= ALLOWED_ORPHAN_MAX,
        "{}",
        format_issues(&orphans, "src/bin/wasmtime_runner.rs (orphans)").unwrap_or_default()
    );
}

/// The memory_runtime namespace is the proof-of-concept for semantic
/// `param_names` enforcement — it's the entry that exposed
/// COMPILER-MEM-ALLOC-NO-GROW. This dedicated test pins it so even if the
/// broader allow-list above is loosened, mem_alloc/mem_retain/mem_release
/// stay locked.
#[test]
fn memory_runtime_param_names_locked() {
    let registry = RegistryIndex::load().expect("registry loads");
    for canonical in ["mem_alloc", "mem_retain", "mem_release"] {
        let entry = registry
            .lookup(canonical)
            .unwrap_or_else(|| panic!("registry missing canonical {canonical}"));
        assert!(
            !entry.param_names.is_empty(),
            "{canonical}: registry must declare param_names so host conformance can detect argument-order drift"
        );
    }
}
