//! Plugin Contracts v2 Phase B cycle 3 — `[[artifacts]]` manifest declaration.
//!
//! Phase C closed the gap: the artifact emitters for `purpose = "manifest"`
//! and `purpose = "static_asset"` now produce real files, so a plugin
//! declaring those purposes lands its entries in `build-manifest.json`
//! alongside `client_hydration` artifacts. See
//! `src/plugin_artifacts.rs::emit_artifact_bytes` and the spec at
//! `foundation/spec/plugins/contracts/artifacts.md` §3-§6.

use clean_language_compiler::plugin_artifacts::{orchestrate, EmitContext};
use clean_language_compiler::plugins::plugin_abi::{ArtifactSource, PluginManifest};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

/// Phase C: load the fixture plugin.toml, run the artifact orchestrator
/// directly, and assert both declared artifacts reach the EmittedArtifact
/// list with the correct purpose, path, and bytes — which is exactly what
/// main.rs serializes into build-manifest.json.
#[test]
fn declared_artifacts_appear_in_build_manifest_json() {
    let m = load_manifest();
    let mut manifests = HashMap::new();
    manifests.insert("test.artifacts_v2".to_string(), m);

    // build_state mimics what `test.artifacts_v2`'s expand_block would push
    // during compilation. The emitters look up scoped keys
    // `<plugin>.<artifact_name>`.
    let mut state = std::collections::BTreeMap::new();
    state.insert(
        "test.artifacts_v2.components.json".to_string(),
        r#"{"button":"button_render"}"#.to_string(),
    );
    state.insert(
        "test.artifacts_v2.theme.css".to_string(),
        ":root { --primary: #4a90e2; }".to_string(),
    );

    let output_dir = PathBuf::from("/tmp/dist-artifacts-v2-test");
    let ctx = EmitContext {
        entry_path: Path::new("/tmp/whatever.cln"),
        output_dir: &output_dir,
        opt_level: 2,
        in_nested_build: false,
        build_state: state,
        plugin_dirs: Default::default(),
    };

    let (emitted, warnings) = orchestrate(&manifests, &ctx).expect("orchestrate ok");
    assert!(warnings.is_empty(), "no warnings expected: {:?}", warnings);
    assert_eq!(emitted.len(), 2, "both declared artifacts must be emitted");

    let manifest_art = emitted
        .iter()
        .find(|a| a.name == "components.json")
        .expect("manifest artifact missing from emitted list");
    assert_eq!(manifest_art.purpose, "manifest");
    assert_eq!(manifest_art.path_relative, "components.json");
    assert_eq!(manifest_art.source_plugin, "test.artifacts_v2");
    // Per artifacts.md §4.3 the manifest is JSON — round-trip to prove it.
    let parsed: serde_json::Value = serde_json::from_slice(&manifest_art.bytes)
        .expect("manifest artifact bytes must be valid JSON");
    assert_eq!(parsed["button"], "button_render");

    let css_art = emitted
        .iter()
        .find(|a| a.name == "theme.css")
        .expect("static_asset artifact missing from emitted list");
    assert_eq!(css_art.purpose, "static_asset");
    assert_eq!(css_art.path_relative, "theme.css");
    assert_eq!(css_art.content_type, "text/css");
    assert!(css_art.public);
    assert_eq!(css_art.bytes, b":root { --primary: #4a90e2; }");
}
