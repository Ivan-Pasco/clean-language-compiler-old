//! `cln compile --release` must run the same pipeline as a normal build.
//!
//! `compile_multi_file_release` used to be a hand-maintained copy of
//! `compile_multi_file_with_memory_tier` that had drifted: it skipped the
//! `package:` `target:` host-class derivation (so a `target: browser`
//! package built with `--release` could stub server-only bridges instead of
//! rejecting them), the plugin type stubs, and the required-parameter
//! counts for plugin functions with defaults. Both entry points now share
//! one pipeline; release only strips `always:` invariant checks.

use clean_language_compiler::{
    compile_multi_file_release, compile_multi_file_with_memory_tier,
    set_target_host_class_override, target_host_class_override, MemoryTier,
};
use tempfile::TempDir;

#[test]
fn release_build_derives_host_class_from_package_target() {
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("main.cln");
    std::fs::write(
        &main,
        "package:\n\tname: \"demo\"\n\ttarget: browser\n\nstart:\n\tinteger x = 42\n\tprint(x)\n",
    )
    .expect("write main.cln");

    // The host-class override is thread-local; start from a clean slate.
    set_target_host_class_override(None);
    // The compile itself may fail later (no plugin owns `package:` here);
    // what matters is that the release pipeline read the manifest's target
    // before doing anything else, exactly like the development pipeline.
    let _ = compile_multi_file_release(
        &main,
        vec![tmp.path().to_path_buf()],
        0,
        None,
        MemoryTier::Standard,
    );
    let derived = target_host_class_override();
    set_target_host_class_override(None);

    assert_eq!(
        derived.as_deref(),
        Some("browser"),
        "release build must honour `package:` `target: browser`"
    );
}

#[test]
fn release_and_development_builds_match_without_invariants() {
    // With no `always:` invariants to strip, the release pipeline has
    // nothing to change, so both builds must produce byte-identical WASM.
    let tmp = TempDir::new().expect("tempdir");
    let main = tmp.path().join("main.cln");
    std::fs::write(
        &main,
        "start:\n\tinteger x = double(21)\n\tprint(x)\n\nfunctions:\n\tinteger double(integer n)\n\t\treturn n * 2\n",
    )
    .expect("write main.cln");

    set_target_host_class_override(None);
    let (dev_wasm, _) = compile_multi_file_with_memory_tier(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
        false,
    )
    .unwrap_or_else(|errs| panic!("development build failed: {errs:?}"));
    let (release_wasm, _) = compile_multi_file_release(
        &main,
        vec![tmp.path().to_path_buf()],
        2,
        None,
        MemoryTier::Standard,
    )
    .unwrap_or_else(|errs| panic!("release build failed: {errs:?}"));

    assert!(!dev_wasm.is_empty());
    assert_eq!(
        dev_wasm, release_wasm,
        "release and development builds must be identical when there is nothing to strip"
    );
}
