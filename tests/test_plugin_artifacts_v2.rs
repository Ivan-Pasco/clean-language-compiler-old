//! Plugin Contracts v2 Phase B cycle 3 — `[[artifacts]]` manifest declaration.
//!
//! Phase B in this repo ships the v2 manifest READ path and the partial
//! emission path: `[[artifacts]]` entries parse into `PluginArtifact`,
//! `evaluate_required_when` consults them, and `from_module =
//! "client_only_build"` triggers a nested compile that lands a real
//! frontend.wasm in the build manifest. The other artifact sources
//! (`from_module = "manifest"`, `static_path = ...`, and the WASM-export
//! callback used by `static_asset` purposes like compiled CSS) currently
//! return `UnsupportedSource` and are SKIPPED — the entries never reach
//! `build-manifest.json`. See `src/plugin_artifacts.rs::emit_artifact_bytes`
//! (Phase B comments cite "Phase C follow-up commit") and the spec at
//! `foundation/spec/plugins/contracts/artifacts.md` §3.
//!
//! Consequence for cycle 3: the only assertion we can make end-to-end about
//! a plugin declaring `manifest` + `static_asset` artifacts is at the
//! manifest-PARSE level. The "artifacts land in build-manifest.json"
//! assertion is gated until Phase C lands the remaining emitters; that
//! gap is tracked via report_error.

use clean_language_compiler::plugins::plugin_abi::{ArtifactSource, PluginManifest};
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cln/plugins/artifacts_v2")
}

fn load_manifest() -> PluginManifest {
    let src =
        std::fs::read_to_string(fixture_dir().join("plugin.toml")).expect("plugin.toml exists");
    toml::from_str(&src).expect("plugin.toml parses")
}

#[test]
fn artifacts_section_round_trips_both_purposes() {
    let m = load_manifest();
    assert_eq!(m.artifacts.len(), 2, "fixture declares two artifacts");

    let manifest_art = m
        .artifacts
        .iter()
        .find(|a| a.purpose == "manifest")
        .expect("manifest-purpose artifact must parse");
    assert_eq!(manifest_art.name, "components.json");
    assert_eq!(manifest_art.output_relative, "{output_dir}/components.json");
    assert_eq!(manifest_art.required_when, "always");
    assert!(!manifest_art.public);
    match manifest_art.emit.as_ref().expect("emit set") {
        ArtifactSource::Module { from_module } => assert_eq!(from_module, "manifest"),
        other => panic!("expected from_module=\"manifest\", got {:?}", other),
    }

    let static_art = m
        .artifacts
        .iter()
        .find(|a| a.purpose == "static_asset")
        .expect("static_asset artifact must parse");
    assert_eq!(static_art.name, "theme.css");
    assert!(static_art.public);
    assert_eq!(static_art.content_type.as_deref(), Some("text/css"));
    match static_art.emit.as_ref().expect("emit set") {
        ArtifactSource::Callback(name) => assert_eq!(name, "emit_theme_css"),
        other => panic!("expected callback emit, got {:?}", other),
    }
}

#[test]
fn build_manifest_schema_is_emitted_for_user_code_compiles() {
    // Compile any tiny .cln program and verify the compiler writes a
    // build-manifest.json with the schema_version field. This is the
    // Phase B unconditional emission per artifacts.md §7.5 and is
    // independent of whether any plugin declares [[artifacts]].
    use std::process::Command;
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.cln"), "start:\n\tprintl(\"hi\")\n").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_cln"))
        .args(["compile", "hello.cln", "-o", "hello.wasm"])
        .current_dir(tmp.path())
        .status()
        .expect("cln runs");
    assert!(status.success());

    let manifest_path = tmp.path().join("build-manifest.json");
    assert!(
        manifest_path.is_file(),
        "build-manifest.json must be written next to the WASM"
    );
    let body = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(
        body.contains("\"schema_version\""),
        "manifest must carry schema_version field: {}",
        body
    );
    assert!(
        body.contains("\"runtime_abi_version\""),
        "manifest must carry runtime_abi_version field: {}",
        body
    );
    assert!(
        body.contains("\"artifacts\""),
        "manifest must carry artifacts field: {}",
        body
    );
}

// Spec-vs-impl gap: `manifest` and `static_asset` artifact emitters return
// UnsupportedSource in Phase B (src/plugin_artifacts.rs:189-211), so a
// plugin declaring those purposes ends up with EMPTY artifact list in
// build-manifest.json — the entries are silently skipped with a tracing
// warning. The assertion below would document the intended end-state
// (entries surface in the manifest) but cannot pass until Phase C lands
// the remaining emitters. Re-enable when the report below is closed.
//
// Tracked as compiler bug — see report_error MCP call from cycle 3.
#[test]
#[ignore = "Phase C: artifact emitters for `manifest` + `static_asset` purposes return UnsupportedSource (src/plugin_artifacts.rs:189-211)"]
fn declared_artifacts_appear_in_build_manifest_json() {
    // When Phase C lands the manifest + callback emitters, drive a real
    // compile against `tests/cln/plugins/artifacts_v2/` and assert the
    // two declared entries reach build-manifest.json with their declared
    // purposes. The infrastructure (orchestrate_artifacts + manifest
    // emission) already exists — only the emit_artifact_bytes branches
    // need filling in.
}
