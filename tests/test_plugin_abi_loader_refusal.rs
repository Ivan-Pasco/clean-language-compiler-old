//! Plugin Contracts v2 Phase B cycle 2 — loader refusal + absent-stamp warning.
//!
//! Cycle 1 stamps every `cln compile --target=plugin` build with a
//! `clean.abi_version` custom section. This test exercises the loader's
//! three-case decision on that stamp (see runtime-abi.md §5):
//!   - Supported   → load proceeds.
//!   - Unsupported → load is refused with PLUGIN-ABI-MISMATCH.
//!   - Absent      → load proceeds with a warning.
//!
//! The mismatch and absent fixtures are produced by byte-patching the WASM
//! emitted from `tests/cln/plugins/abi_stamp_{mismatch,absent}` rather than
//! pulling in `wasm-encoder` for a hand-crafted module: the patches are tiny
//! and keep the fixtures faithful to real compiler output.
//!
//! See `foundation/spec/plugins/contracts/runtime-abi.md` §4–§5.

use std::path::{Path, PathBuf};
use std::process::Command;

use clean_language_compiler::plugins::{AbiStampOutcome, WasmPluginLoader};
use tempfile::TempDir;
use wasmparser::{Parser, Payload};

fn cln_binary() -> &'static str {
    env!("CARGO_BIN_EXE_cln")
}

const FIXTURE_SOURCE: &str =
    "functions:\n\tstring expand_block(string name, string attrs, string body)\n\t\treturn body\n";

/// Compile a freshly stamped plugin.wasm into `dest_dir`. Returns the bytes
/// of the produced wasm.
fn build_stamped_plugin(dest_dir: &Path, plugin_name: &str) -> Vec<u8> {
    std::fs::create_dir_all(dest_dir.join("src")).unwrap();
    std::fs::write(
        dest_dir.join("plugin.toml"),
        format!(
            "[plugin]\nname = \"{}\"\nversion = \"1.0.0\"\n\n[compatibility]\nabi_version = \"1.0.0\"\n\n[handles]\nblocks = [\"{}\"]\n",
            plugin_name, plugin_name
        ),
    )
    .unwrap();
    std::fs::write(dest_dir.join("src/main.cln"), FIXTURE_SOURCE).unwrap();

    let status = Command::new(cln_binary())
        .args([
            "compile",
            "src/main.cln",
            "-o",
            "plugin.wasm",
            "--target=plugin",
        ])
        .current_dir(dest_dir)
        .status()
        .expect("failed to run cln");
    assert!(status.success(), "cln compile --target=plugin must succeed");

    std::fs::read(dest_dir.join("plugin.wasm")).expect("plugin.wasm exists")
}

/// Find the `(start, end)` byte range of the `clean.abi_version` custom
/// section in `bytes`, where `start` is the offset of the section-ID byte (0x00)
/// and `end` is one past the section's last payload byte. Returns `None` if
/// the section is absent.
fn locate_abi_section(bytes: &[u8]) -> Option<(usize, usize)> {
    for payload in Parser::new(0).parse_all(bytes).flatten() {
        if let Payload::CustomSection(reader) = payload {
            if reader.name() == "clean.abi_version" {
                // CustomSectionReader::range covers the section body (after
                // the size field). Walk back to include the size ULEB + the
                // section ID byte by scanning for the 0x00 sentinel — fine
                // because cycle 1's emitter always places this section last
                // and Parser yields sections in order.
                let body = reader.range();
                let mut id_idx = body.start - 1;
                // Skip the size ULEB128 backwards: walk until we find a byte
                // whose continuation bit (0x80) is clear AND its predecessor's
                // is set (i.e. start of the ULEB) — or until we reach the
                // section ID byte 0x00.
                while id_idx > 0 && bytes[id_idx] != 0x00 {
                    id_idx -= 1;
                }
                return Some((id_idx, body.end));
            }
        }
    }
    None
}

/// Patch the `clean.abi_version` payload in-place. `new_payload` MUST have
/// the same byte length as the existing payload so the section's size ULEB
/// stays valid without re-encoding.
fn patch_abi_payload_same_length(bytes: &mut Vec<u8>, new_payload: &[u8]) {
    let payload_range = Parser::new(0)
        .parse_all(bytes)
        .flatten()
        .find_map(|p| match p {
            Payload::CustomSection(reader) if reader.name() == "clean.abi_version" => {
                Some(reader.range())
            }
            _ => None,
        })
        .expect("fixture must already carry a clean.abi_version stamp");
    let stamp_offset = payload_range.end - 5; // existing payload "1.0.0" = 5 bytes
    assert_eq!(
        new_payload.len(),
        5,
        "patch requires same-length payload to avoid resizing the section ULEB"
    );
    bytes[stamp_offset..stamp_offset + 5].copy_from_slice(new_payload);
}

/// Truncate the `clean.abi_version` custom section off the end of `bytes`.
/// Relies on cycle 1 placing the stamp section last; we assert that's true.
fn strip_abi_section(bytes: &mut Vec<u8>) {
    let (start, end) = locate_abi_section(bytes).expect("section must be present");
    assert_eq!(
        end,
        bytes.len(),
        "clean.abi_version is expected to be the last section in cycle 1 output"
    );
    bytes.truncate(start);
}

/// Install a forged plugin.wasm + plugin.toml into a fresh temp plugins
/// directory at `<root>/<plugin_name>/<version>/`, mirroring the on-disk
/// layout `WasmPluginLoader` walks.
fn install_plugin(plugins_root: &Path, plugin_name: &str, wasm_bytes: &[u8]) -> PathBuf {
    let plugin_dir = plugins_root.join(plugin_name).join("1.0.0");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let toml = format!(
        "[plugin]\nname = \"{}\"\nversion = \"1.0.0\"\n\n[handles]\nblocks = [\"{}\"]\n",
        plugin_name, plugin_name
    );
    std::fs::write(plugin_dir.join("plugin.toml"), toml).unwrap();
    std::fs::write(plugin_dir.join("plugin.wasm"), wasm_bytes).unwrap();
    plugin_dir.join("plugin.wasm")
}

// ---------------------------------------------------------------------------
// classify_abi_stamp — unit-level coverage of the three branches.
// ---------------------------------------------------------------------------

#[test]
fn classify_recognises_supported_stamp() {
    let build_root = TempDir::new().unwrap();
    let bytes = build_stamped_plugin(build_root.path(), "abi_stamp_supported");
    match WasmPluginLoader::classify_abi_stamp(&bytes) {
        AbiStampOutcome::Supported(v) => assert_eq!(v, "1.0.0"),
        other => panic!("expected Supported(\"1.0.0\"), got {:?}", other),
    }
}

#[test]
fn classify_recognises_unsupported_stamp() {
    let build_root = TempDir::new().unwrap();
    let mut bytes = build_stamped_plugin(build_root.path(), "abi_stamp_unsupported");
    patch_abi_payload_same_length(&mut bytes, b"9.0.0");
    match WasmPluginLoader::classify_abi_stamp(&bytes) {
        AbiStampOutcome::Unsupported(v) => assert_eq!(v, "9.0.0"),
        other => panic!("expected Unsupported(\"9.0.0\"), got {:?}", other),
    }
}

#[test]
fn classify_recognises_absent_stamp() {
    let build_root = TempDir::new().unwrap();
    let mut bytes = build_stamped_plugin(build_root.path(), "abi_stamp_strip");
    strip_abi_section(&mut bytes);
    assert_eq!(
        WasmPluginLoader::classify_abi_stamp(&bytes),
        AbiStampOutcome::Absent,
    );
}

// ---------------------------------------------------------------------------
// End-to-end loader behaviour — mismatch refuses, supported loads, absent
// loads with the documented warning.
// ---------------------------------------------------------------------------

#[test]
fn loader_refuses_mismatched_stamp_with_actionable_error() {
    let build_root = TempDir::new().unwrap();
    let mut bytes = build_stamped_plugin(build_root.path(), "abi_mismatch_test");
    patch_abi_payload_same_length(&mut bytes, b"9.0.0");

    let plugins_root = TempDir::new().unwrap();
    let wasm_path = install_plugin(plugins_root.path(), "abi.mismatch", &bytes);

    let mut loader = WasmPluginLoader::with_plugins_dir(plugins_root.path().to_path_buf()).unwrap();
    let err = loader
        .load_plugins(&["abi.mismatch".to_string()])
        .expect_err("loader must refuse a plugin with an unsupported abi_version");
    let msg = format!("{}", err);

    // Spec-mandated content — error code, found version, supported list,
    // plugin path, and spec pointer all reachable in a single message.
    assert!(
        msg.contains("PLUGIN-ABI-MISMATCH"),
        "error must carry error code PLUGIN-ABI-MISMATCH: {}",
        msg
    );
    assert!(
        msg.contains("9.0.0"),
        "error must cite found version: {}",
        msg
    );
    assert!(
        msg.contains("1.0.0"),
        "error must list supported versions including 1.0.0: {}",
        msg
    );
    assert!(
        msg.contains(&wasm_path.display().to_string()),
        "error must include the plugin path: {}",
        msg
    );
    assert!(
        msg.contains("foundation/spec/plugins/contracts/runtime-abi.md"),
        "error must point at the runtime-abi spec: {}",
        msg
    );

    let expected = WasmPluginLoader::format_abi_mismatch_error("abi.mismatch", &wasm_path, "9.0.0");
    assert_eq!(
        msg, expected,
        "error message must match format_abi_mismatch_error verbatim",
    );
}

#[test]
fn loader_accepts_mismatched_fixture_when_reset_to_supported_payload() {
    // Guard against the loader becoming over-strict: the same fixture, with
    // the stamp reset to a supported version, must load successfully.
    let build_root = TempDir::new().unwrap();
    let mut bytes = build_stamped_plugin(build_root.path(), "abi_supported_test");
    patch_abi_payload_same_length(&mut bytes, b"9.0.0");
    patch_abi_payload_same_length(&mut bytes, b"1.0.0");

    let plugins_root = TempDir::new().unwrap();
    install_plugin(plugins_root.path(), "abi.supported", &bytes);

    let mut loader = WasmPluginLoader::with_plugins_dir(plugins_root.path().to_path_buf()).unwrap();
    loader
        .load_plugins(&["abi.supported".to_string()])
        .expect("loader must accept a plugin with abi_version 1.0.0");
}

#[test]
fn loader_accepts_absent_stamp_with_documented_warning_text() {
    let build_root = TempDir::new().unwrap();
    let mut bytes = build_stamped_plugin(build_root.path(), "abi_absent_test");
    strip_abi_section(&mut bytes);

    let plugins_root = TempDir::new().unwrap();
    let wasm_path = install_plugin(plugins_root.path(), "abi.absent", &bytes);

    let mut loader = WasmPluginLoader::with_plugins_dir(plugins_root.path().to_path_buf()).unwrap();
    loader
        .load_plugins(&["abi.absent".to_string()])
        .expect("Phase B contract: absent stamp must NOT block loading");

    // Verify the warning string the loader would print is exactly the
    // contract format — kept in lockstep via format_abi_absent_warning so
    // the eprintln! call site can never silently drift.
    let warning = WasmPluginLoader::format_abi_absent_warning("abi.absent", &wasm_path);
    assert!(
        warning.contains("has no clean.abi_version stamp"),
        "warning must name the missing section: {}",
        warning
    );
    assert!(
        warning.contains("assuming 1.0.0"),
        "warning must state the default version: {}",
        warning
    );
    assert!(
        warning.contains(&wasm_path.display().to_string()),
        "warning must include the plugin path: {}",
        warning
    );
    assert!(
        warning.contains("abi.absent"),
        "warning must include the plugin name: {}",
        warning
    );
}
