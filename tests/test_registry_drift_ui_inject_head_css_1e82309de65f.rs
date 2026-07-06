//! Regression test for dashboard fingerprint 1e82309de65f.
//!
//! Error code: COMPILER-REGISTRY-DRIFT-UI-INJECT-HEAD-CSS.
//!
//! Context: `_ui_inject_head_css` was declared in both the compiler's
//! function-registry.toml and frame.ui's plugin.toml before framework
//! v2.12.96. In v2.12.96 the framework dropped `_ui_inject_head_css` from
//! its plugin.toml, but the compiler-side registry entry lingered — leaving
//! `framework_plugins_match_registry` failing in the "registry declares a
//! function no plugin provides" direction (the inverse of the drift the
//! contract test was originally designed to catch).
//!
//! Fix: the stale registry entry was removed as part of the ongoing registry
//! cleanup ships. Path C Session 3-corrected verified structural closure via
//! `grep -c "_ui_inject_head_css" src/plugins/function-registry.toml` → 0.
//!
//! This anchor test asserts the removal is permanent:
//!   1. `_ui_inject_head_css` MUST NOT be a canonical name in the loaded
//!      registry index.
//!   2. The literal string `_ui_inject_head_css` MUST NOT appear anywhere in
//!      the on-disk `function-registry.toml`, so a resurrection attempt in
//!      the source file is caught even before it is re-parsed into a lookup
//!      entry.
//!
//! If either assertion fails, someone re-added the entry and the whole
//! Session 4-hygiene closure premise is invalidated — reopen the fingerprint.

use std::path::PathBuf;

use clean_language_compiler::plugins::registry_loader::RegistryIndex;

/// The specific canonical name from the drift report. Do NOT parameterize
/// this test — the fingerprint tracks this exact string.
const DRIFTED_NAME: &str = "_ui_inject_head_css";

#[test]
fn ui_inject_head_css_absent_from_registry_index() {
    let idx = RegistryIndex::load().expect("registry must load");
    assert!(
        idx.lookup(DRIFTED_NAME).is_none(),
        "regression: `{DRIFTED_NAME}` reappeared in the registry index — see dashboard fingerprint 1e82309de65f (COMPILER-REGISTRY-DRIFT-UI-INJECT-HEAD-CSS). \
         If this entry is intentionally being reintroduced, coordinate the matching plugin.toml declaration and update this test."
    );
}

#[test]
fn ui_inject_head_css_absent_from_registry_source() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let registry_path = PathBuf::from(manifest_dir)
        .parent()
        .expect("workspace parent must exist")
        .join("foundation")
        .join("platform-architecture")
        .join("function-registry.toml");

    if !registry_path.is_file() {
        eprintln!(
            "skipping: {} not present alongside this checkout (fine in isolated CI)",
            registry_path.display()
        );
        return;
    }

    let source = std::fs::read_to_string(&registry_path).expect("registry file must be readable");
    assert!(
        !source.contains(DRIFTED_NAME),
        "regression: `{DRIFTED_NAME}` reappeared in {} — see dashboard fingerprint 1e82309de65f.",
        registry_path.display()
    );
}
