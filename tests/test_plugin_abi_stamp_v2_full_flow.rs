//! Plugin Contracts v2 Phase B cycle 3 — end-to-end ABI stamp flow.
//!
//! Cycle 1 stamps every plugin.wasm with `clean.abi_version`; cycle 2
//! reads the stamp and refuses unsupported versions. This test pins the
//! seam where the two halves meet: a plugin built by THIS compiler must
//! load silently — no warning, no error — through the same loader the
//! compile pipeline uses. If cycle 1 stops emitting the stamp, or cycle 2
//! starts rejecting the current version, this test breaks.
//!
//! See:
//!   - foundation/spec/plugins/contracts/runtime-abi.md §4 (stamp emission)
//!   - foundation/spec/plugins/contracts/runtime-abi.md §5 (loader behavior)

use std::path::PathBuf;
use std::process::Command;

use clean_language_compiler::plugins::plugin_abi::{
    DEFAULT_RUNTIME_ABI_VERSION, SUPPORTED_RUNTIME_ABI_VERSIONS,
};
use clean_language_compiler::plugins::{AbiStampOutcome, WasmPluginLoader};
use tempfile::TempDir;
use wasmparser::{Parser, Payload};

fn cln_binary() -> &'static str {
    env!("CARGO_BIN_EXE_cln")
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cln/plugins/abi_stamp_v2_full_flow")
}

/// Compile the fixture and return the produced plugin.wasm bytes.
fn build_fixture_plugin() -> Vec<u8> {
    let tmp = TempDir::new().unwrap();
    let src = fixture_dir();
    std::fs::copy(src.join("plugin.toml"), tmp.path().join("plugin.toml")).unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::copy(src.join("main.cln"), tmp.path().join("src/main.cln")).unwrap();

    let status = Command::new(cln_binary())
        .args([
            "compile",
            "src/main.cln",
            "-o",
            "plugin.wasm",
            "--target=plugin",
        ])
        .current_dir(tmp.path())
        .status()
        .expect("cln runs");
    assert!(status.success(), "fixture plugin must compile cleanly");
    std::fs::read(tmp.path().join("plugin.wasm")).expect("plugin.wasm exists")
}

#[test]
fn full_flow_cycle1_emits_then_cycle2_accepts() {
    // 1. Cycle 1 — the compiler stamps the plugin during build.
    let wasm = build_fixture_plugin();

    let mut found_stamp: Option<String> = None;
    for payload in Parser::new(0).parse_all(&wasm).flatten() {
        if let Payload::CustomSection(reader) = payload {
            if reader.name() == "clean.abi_version" {
                found_stamp = Some(std::str::from_utf8(reader.data()).unwrap().to_string());
                break;
            }
        }
    }
    let stamp = found_stamp.expect("cycle-1 emitter must produce clean.abi_version section");
    assert_eq!(
        stamp, DEFAULT_RUNTIME_ABI_VERSION,
        "stamp must equal the declared [compatibility].abi_version (1.0.0)"
    );
    assert!(
        SUPPORTED_RUNTIME_ABI_VERSIONS.contains(&stamp.as_str()),
        "stamped version must be in SUPPORTED set, else cycle-2 would refuse"
    );

    // 2. Cycle 2 — classifier accepts the stamp without warning.
    match WasmPluginLoader::classify_abi_stamp(&wasm) {
        AbiStampOutcome::Supported(v) => assert_eq!(v, stamp),
        other => panic!(
            "cycle-2 classifier must return Supported for current-compiler stamp; got {:?}",
            other
        ),
    }

    // 3. Cycle 2 — load_plugins succeeds without ABI errors against a
    //    fresh plugins dir holding only this fixture.
    let plugins_root = TempDir::new().unwrap();
    let plugin_dir = plugins_root.path().join("abi.full_flow").join("1.0.0");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let toml = format!(
        "[plugin]\nname = \"abi.full_flow\"\nversion = \"1.0.0\"\n\n\
         [compatibility]\nabi_version = \"1.0.0\"\nemission_ops_hash = \"{}\"\n\n\
         [handles]\nblocks = [\"abi_full_flow_block\"]\n\n\
         [lifecycle]\nmodule_helpers_are_roots = true\n",
        clean_language_compiler::plugins::plugin_abi::EMISSION_OPS_HASH
    );
    std::fs::write(plugin_dir.join("plugin.toml"), toml).unwrap();
    std::fs::write(plugin_dir.join("plugin.wasm"), &wasm).unwrap();

    let mut loader = WasmPluginLoader::with_plugins_dir(plugins_root.path().to_path_buf()).unwrap();
    loader
        .load_plugins(&["abi.full_flow".to_string()])
        .expect("loader must accept current-compiler-stamped plugin without error");
}

#[test]
fn fixture_is_minimal_and_real() {
    // Sanity-check that the fixture is a real, parsable plugin.toml that
    // declares the abi_version we care about. Without this, a regression
    // that mutates the fixture (e.g. drops the [compatibility] section)
    // would silently turn the round-trip test into a no-op.
    use clean_language_compiler::plugins::plugin_abi::PluginManifest;
    let toml_src = std::fs::read_to_string(fixture_dir().join("plugin.toml")).unwrap();
    let m: PluginManifest = toml::from_str(&toml_src).expect("fixture plugin.toml parses");
    assert_eq!(
        m.compatibility.abi_version.as_deref(),
        Some("1.0.0"),
        "fixture must declare [compatibility].abi_version = \"1.0.0\""
    );
}
