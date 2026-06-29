//! Plugin Contracts v2 Phase B cycle 3 — v1/v2 [lifecycle] manifest parity.
//!
//! Phase B in this repo ships the v2 manifest READ path: `[lifecycle]` fields
//! are parsed into `PluginLifecycle`, `opts_into_v2()` correctly distinguishes
//! v1 and v2 plugins, and predicates like `has_lifecycle.module_helpers` are
//! evaluated. The slot-call mechanism (invoking the named WASM exports and
//! splicing their output into program/client/server entry points) lands in
//! Phase C as part of the frame.* plugin migration.
//!
//! That means we cannot — yet — assert byte-for-byte equality between a v1
//! and v2 compile because the v2 slots are declared but never invoked. The
//! tests below therefore pin the Phase B-observable behavior:
//!
//!   1. Both plugin variants compile to a working plugin.wasm.
//!   2. The v1 manifest reports `opts_into_v2() == false`; the v2 reports true.
//!   3. The v2 manifest exposes the declared slot names; the v1 leaves them
//!      `None`.
//!   4. Both produced plugin.wasms carry the cycle-1 `clean.abi_version`
//!      stamp the loader accepts.
//!
//! See foundation/spec/plugins/contracts/lifecycle.md §2 + §6 ("compatibility
//! with v1.0.0").

use std::path::{Path, PathBuf};
use std::process::Command;

use clean_language_compiler::plugins::plugin_abi::PluginManifest;
use wasmparser::{Parser, Payload};

fn cln_binary() -> &'static str {
    env!("CARGO_BIN_EXE_cln")
}

fn fixture_dir(variant: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cln/plugins/lifecycle_v2")
        .join(variant)
}

fn compile_fixture(variant: &str) -> Vec<u8> {
    // Copy fixture into a tmp dir before compiling — `cln compile
    // --target=plugin` stamps the sibling plugin.toml with [build]
    // built_with_compiler, which would mutate the checked-in fixture
    // otherwise.
    let src_dir = fixture_dir(variant);
    let tmp = tempfile::TempDir::new().unwrap();
    for name in ["main.cln", "plugin.toml"] {
        std::fs::copy(src_dir.join(name), tmp.path().join(name))
            .unwrap_or_else(|e| panic!("copy {}/{} failed: {}", variant, name, e));
    }
    let status = Command::new(cln_binary())
        .args([
            "compile",
            "main.cln",
            "-o",
            "plugin.wasm",
            "--target=plugin",
        ])
        .current_dir(tmp.path())
        .status()
        .expect("failed to run cln");
    assert!(status.success(), "cln compile must succeed for {}", variant);
    std::fs::read(tmp.path().join("plugin.wasm")).expect("plugin wasm exists")
}

fn load_manifest(variant: &str) -> PluginManifest {
    let path = fixture_dir(variant).join("plugin.toml");
    let src = std::fs::read_to_string(&path).expect("plugin.toml exists");
    toml::from_str(&src).expect("plugin.toml parses")
}

fn has_abi_stamp(bytes: &[u8]) -> bool {
    Parser::new(0)
        .parse_all(bytes)
        .any(|p| matches!(p, Ok(Payload::CustomSection(ref r)) if r.name() == "clean.abi_version"))
}

#[test]
fn v1_plugin_opts_out_of_v2_dispatch() {
    let m = load_manifest("v1");
    assert!(
        !m.lifecycle.opts_into_v2(),
        "v1 fixture must NOT opt into v2 dispatch"
    );
    assert!(m.lifecycle.module_helpers.is_none());
    assert!(m.lifecycle.program_init.is_none());
    assert!(!m.lifecycle.module_helpers_are_roots);
    assert!(m.compatibility.abi_version.is_none());
}

#[test]
fn v2_plugin_opts_into_v2_dispatch_with_declared_slots() {
    let m = load_manifest("v2");
    assert!(
        m.lifecycle.opts_into_v2(),
        "v2 fixture must opt into v2 dispatch"
    );
    assert_eq!(
        m.lifecycle.module_helpers.as_deref(),
        Some("emit_helpers"),
        "module_helpers slot name must be exposed verbatim"
    );
    assert_eq!(
        m.lifecycle.program_init.as_deref(),
        Some("emit_program_init"),
        "program_init slot name must be exposed verbatim"
    );
    assert!(
        m.lifecycle.module_helpers_are_roots,
        "module_helpers_are_roots flag must round-trip"
    );
    assert_eq!(m.compatibility.abi_version.as_deref(), Some("1.0.0"));
}

#[test]
fn both_variants_compile_to_a_stamped_plugin_wasm() {
    let v1_bytes = compile_fixture("v1");
    let v2_bytes = compile_fixture("v2");
    assert!(!v1_bytes.is_empty(), "v1 compile produced empty wasm");
    assert!(!v2_bytes.is_empty(), "v2 compile produced empty wasm");
    assert!(
        has_abi_stamp(&v1_bytes),
        "v1 plugin wasm must carry the cycle-1 ABI stamp"
    );
    assert!(
        has_abi_stamp(&v2_bytes),
        "v2 plugin wasm must carry the cycle-1 ABI stamp"
    );
}

#[test]
fn fixture_files_exist_on_disk() {
    for variant in ["v1", "v2"] {
        let dir = fixture_dir(variant);
        assert!(
            dir.join("plugin.toml").is_file(),
            "missing plugin.toml for {}: {}",
            variant,
            dir.display()
        );
        assert!(
            dir.join("main.cln").is_file(),
            "missing main.cln for {}: {}",
            variant,
            dir.display()
        );
    }
}

// NOTE on equivalence strategy: byte-diff between v1 and v2 wasms is NOT
// useful in Phase B because (a) the v2 lifecycle slot caller hasn't landed
// (Phase C work) so the v2 slot exports are dead weight in the produced
// wasm, and (b) even ignoring that, the modules carry distinct plugin
// metadata that bakes into the `clean:build` custom section. Observable
// stdout equivalence is also moot until the slot caller lands and starts
// actually invoking program_init/module_helpers. The assertions above
// cover the Phase B contract; full v1/v2 equivalence is a Phase C
// follow-up once frame.* plugins migrate.
fn _equivalence_strategy_note(_p: &Path) {}
