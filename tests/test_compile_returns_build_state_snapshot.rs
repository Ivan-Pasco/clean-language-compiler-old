//! Regression test for PLUGIN_BUILD_STATE_NOT_PERSISTED
//! (fingerprint `4f36d40aa3444138e895194838aca9e3c11787b534dc78c74a510198ee36a59c`,
//! reported against compiler 0.30.276).
//!
//! Background: `compile_multi_file_with_memory_tier` and
//! `compile_multi_file_release` previously returned only the compiled WASM
//! bytes. Every plugin `_build_state_set` call made during `expand_block`
//! went into a per-build `BuildState` Arc owned by the in-flight registry,
//! which was then dropped at end of compile. Downstream consumers in
//! `main.rs` re-discovered plugins through a fresh registry (which never
//! ran any expand_block) and snapshotted that empty BuildState — so the
//! manifest's `build_state` field, the artifact orchestrator's
//! `has_build_state.*` predicate, and the lifecycle slot's `BuildContext`
//! all saw an empty map. Symptom: `frontend.wasm` not emitted, and
//! `build-manifest.json` showed `"build_state": {}`.
//!
//! The fix threads the populated snapshot out of the compile pass as the
//! second tuple element of the `Result`. This test locks in the new API
//! contract by verifying:
//!   (a) the function returns `(Vec<u8>, BTreeMap<String, String>)` —
//!       compile-time guarantee from the destructuring, and
//!   (b) the no-plugin case returns a non-empty WASM and an empty
//!       snapshot (the snapshot map exists, just has nothing in it).
//!
//! The "with-plugins" case (snapshot reflects real `_build_state_set`
//! writes) is covered end-to-end by
//! `tests/test_hydrate_auto_e2e.rs::client_init_splice_reaches_start_body`
//! when frame.ui is installed in the dev environment.

use std::fs;
use tempfile::TempDir;

use clean_language_compiler::MemoryTier;

#[test]
fn compile_multi_file_with_memory_tier_returns_build_state_tuple() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("main.cln");
    fs::write(&main, "start:\n\tprintln(\"hello\")\n").expect("write main.cln");

    let result = clean_language_compiler::compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        0,
        None,
        MemoryTier::Standard,
        false,
    );

    let (wasm, build_state) = result.unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!("trivial no-plugin compile must succeed");
    });

    assert!(
        !wasm.is_empty(),
        "compile must return non-empty WASM bytes (got {} bytes)",
        wasm.len()
    );

    // The build_state snapshot is the tuple's second element. For a project
    // with no plugins declared, no `_build_state_set` calls can have run, so
    // the snapshot must be empty (not absent — the contract is that the
    // caller always receives a map it can inspect).
    assert!(
        build_state.is_empty(),
        "no-plugin compile must yield an empty build_state snapshot; got {:?}",
        build_state
    );
}

#[test]
fn compile_multi_file_release_returns_build_state_tuple() {
    // Same contract for the release-mode entry point. Pre-fix the release
    // path used bare `load_plugins` (not `_with_build_state`), so even with
    // plugins it produced an empty BuildState. The fix routes both paths
    // through `load_plugins_with_build_state` and returns the snapshot.
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("main.cln");
    fs::write(&main, "start:\n\tprintln(\"hello\")\n").expect("write main.cln");

    let result = clean_language_compiler::compile_multi_file_release(
        &main,
        vec![tmp.path().to_path_buf()],
        0,
        None,
        MemoryTier::Standard,
    );

    let (wasm, build_state) = result.unwrap_or_else(|errors| {
        for e in &errors {
            eprintln!("compile error: {e}");
        }
        panic!("trivial no-plugin release compile must succeed");
    });

    assert!(
        !wasm.is_empty(),
        "release compile must return non-empty WASM"
    );
    assert!(
        build_state.is_empty(),
        "no-plugin release compile must yield an empty build_state snapshot; got {:?}",
        build_state
    );
}
