//! `--plugin-override <name>=<path>` end-to-end CLI tests.
//!
//! Covers the three externally-visible failure modes and the banner emission
//! for the success path. Full happy-path plugin loading is covered by the
//! separate plugin loader tests — here we just verify the CLI wrapper
//! (argument parse, validation, banner, thread-local install) does what the
//! prompt spec requires. See prompt 69218d10 and
//! `foundation/spec/plugins/plugin-contract.md` §11.

use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

fn cln_binary() -> &'static str {
    env!("CARGO_BIN_EXE_cln")
}

/// A tiny `.cln` source that declares one plugin in a `plugins:` block. The
/// plugin does not need to load for the override-validation error paths
/// tested here — validation runs before any plugin resolution.
const SOURCE_WITH_ONE_PLUGIN: &str = "plugins: [frame.data]\n\nstart:\n\tprint(\"hi\")\n";

/// A `.cln` source with no `plugins:` block.
const SOURCE_WITH_NO_PLUGINS: &str = "start:\n\tprint(\"hi\")\n";

fn write_source(dir: &TempDir, name: &str, source: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, source).unwrap();
    path
}

fn write_stub_plugin(dir: &TempDir, name: &str, version: &str) -> PathBuf {
    let plugin_dir = dir.path().join(name);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        format!(
            "[package]\nname = \"{}\"\nversion = \"{}\"\n",
            name, version
        ),
    )
    .unwrap();
    // Any bytes — validation only checks existence, not that the WASM parses.
    std::fs::write(plugin_dir.join("plugin.wasm"), [0x00, 0x61, 0x73, 0x6d]).unwrap();
    plugin_dir
}

/// Failure mode 1 (per prompt §Failure model, row 1):
/// override names a plugin not loaded by any file in the compilation.
/// Must fail fast with the discovered set listed.
#[test]
fn unmatched_plugin_name_hard_errors_with_discovered_set() {
    let dir = TempDir::new().unwrap();
    let src = write_source(&dir, "main.cln", SOURCE_WITH_ONE_PLUGIN);
    let plugin_dir = write_stub_plugin(&dir, "frame.foo", "9.9.9");

    let out = Command::new(cln_binary())
        .args([
            "check",
            src.to_str().unwrap(),
            "--plugin-override",
            &format!("frame.foo={}", plugin_dir.display()),
        ])
        .output()
        .expect("cln check failed to spawn");

    assert!(!out.status.success(), "must fail on unmatched plugin name");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no file loads plugin 'frame.foo'"),
        "stderr must name the missing plugin: {}",
        stderr
    );
    assert!(
        stderr.contains("Plugins loaded by this project: frame.data"),
        "stderr must list the discovered plugin set: {}",
        stderr
    );
}

/// Failure mode 1b: override name doesn't match, and the source declares no
/// plugins at all. Discovered set is `<none>`.
#[test]
fn unmatched_plugin_name_with_no_plugins_declared_shows_none() {
    let dir = TempDir::new().unwrap();
    let src = write_source(&dir, "main.cln", SOURCE_WITH_NO_PLUGINS);
    let plugin_dir = write_stub_plugin(&dir, "frame.foo", "1.0.0");

    let out = Command::new(cln_binary())
        .args([
            "check",
            src.to_str().unwrap(),
            "--plugin-override",
            &format!("frame.foo={}", plugin_dir.display()),
        ])
        .output()
        .expect("cln check failed to spawn");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Plugins loaded by this project: <none>"),
        "stderr: {}",
        stderr
    );
}

/// Failure mode 2 (per prompt §Failure model, row 2, sub-case A):
/// override points at a directory that has no `plugin.wasm` inside.
#[test]
fn missing_plugin_wasm_hard_errors() {
    let dir = TempDir::new().unwrap();
    let src = write_source(&dir, "main.cln", SOURCE_WITH_ONE_PLUGIN);
    let plugin_dir = dir.path().join("frame.data");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    // toml exists but no wasm
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        "[package]\nname = \"frame.data\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();

    let out = Command::new(cln_binary())
        .args([
            "check",
            src.to_str().unwrap(),
            "--plugin-override",
            &format!("frame.data={}", plugin_dir.display()),
        ])
        .output()
        .expect("cln check failed to spawn");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("plugin.wasm not found"),
        "stderr: {}",
        stderr
    );
    assert!(stderr.contains("frame.data"), "stderr: {}", stderr);
}

/// Failure mode 2 (sub-case B): override points at a `plugin.wasm` file but
/// no sibling `plugin.toml` exists.
#[test]
fn missing_sibling_plugin_toml_hard_errors() {
    let dir = TempDir::new().unwrap();
    let src = write_source(&dir, "main.cln", SOURCE_WITH_ONE_PLUGIN);
    let wasm_path = dir.path().join("bare-plugin.wasm");
    std::fs::write(&wasm_path, [0x00, 0x61, 0x73, 0x6d]).unwrap();

    let out = Command::new(cln_binary())
        .args([
            "check",
            src.to_str().unwrap(),
            "--plugin-override",
            &format!("frame.data={}", wasm_path.display()),
        ])
        .output()
        .expect("cln check failed to spawn");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("sibling plugin.toml not found"),
        "stderr: {}",
        stderr
    );
}

/// Failure mode 2 (sub-case C): override points at a directory containing
/// a malformed `plugin.toml`.
#[test]
fn malformed_plugin_toml_hard_errors() {
    let dir = TempDir::new().unwrap();
    let src = write_source(&dir, "main.cln", SOURCE_WITH_ONE_PLUGIN);
    let plugin_dir = dir.path().join("frame.data");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("plugin.wasm"), [0x00, 0x61, 0x73, 0x6d]).unwrap();
    std::fs::write(plugin_dir.join("plugin.toml"), "this is not = valid [ toml").unwrap();

    let out = Command::new(cln_binary())
        .args([
            "check",
            src.to_str().unwrap(),
            "--plugin-override",
            &format!("frame.data={}", plugin_dir.display()),
        ])
        .output()
        .expect("cln check failed to spawn");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("malformed plugin.toml"),
        "stderr: {}",
        stderr
    );
}

/// Shape failure: missing `=`.
#[test]
fn malformed_arg_shape_hard_errors() {
    let dir = TempDir::new().unwrap();
    let src = write_source(&dir, "main.cln", SOURCE_WITH_ONE_PLUGIN);

    let out = Command::new(cln_binary())
        .args([
            "check",
            src.to_str().unwrap(),
            "--plugin-override",
            "no-equals-sign",
        ])
        .output()
        .expect("cln check failed to spawn");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("expects `<name>=<path>`"),
        "stderr: {}",
        stderr
    );
}

/// Banner reads version from `[plugin].version` (the canonical key used by
/// real Clean plugins — see any file under `~/.cleen/plugins/*/plugin.toml`).
#[test]
fn banner_reads_version_from_plugin_section() {
    let dir = TempDir::new().unwrap();
    let src = write_source(&dir, "main.cln", SOURCE_WITH_ONE_PLUGIN);
    let plugin_dir = dir.path().join("frame.data");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("plugin.wasm"), [0x00, 0x61, 0x73, 0x6d]).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        "[plugin]\nname = \"frame.data\"\nversion = \"5.5.5\"\n",
    )
    .unwrap();

    let out = Command::new(cln_binary())
        .args([
            "check",
            src.to_str().unwrap(),
            "--plugin-override",
            &format!("frame.data={}", plugin_dir.display()),
        ])
        .output()
        .expect("cln check failed to spawn");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Plugin override: frame.data →") && stdout.contains("(v5.5.5)"),
        "banner should show [plugin].version: {}",
        stdout
    );
}

/// Banner emission: happy-path validation succeeds and one line per override
/// is printed to stdout before compile work starts. This test stops before
/// actual plugin loading — the validation-only surface is sufficient because
/// the banner is emitted immediately after validation succeeds. We assert
/// on the banner even though the subsequent plugin load may fail (the stub
/// bytes are not a valid WASM module).
#[test]
fn banner_emitted_on_successful_validation() {
    let dir = TempDir::new().unwrap();
    let src = write_source(&dir, "main.cln", SOURCE_WITH_ONE_PLUGIN);
    let plugin_dir = write_stub_plugin(&dir, "frame.data", "9.9.9");

    let out = Command::new(cln_binary())
        .args([
            "check",
            src.to_str().unwrap(),
            "--plugin-override",
            &format!("frame.data={}", plugin_dir.display()),
        ])
        .output()
        .expect("cln check failed to spawn");

    let stdout = String::from_utf8_lossy(&out.stdout);
    // Version comes from plugin.toml `[package].version`.
    assert!(
        stdout.contains("Plugin override: frame.data →"),
        "banner missing: {}",
        stdout
    );
    assert!(
        stdout.contains("(v9.9.9)"),
        "version missing from banner: {}",
        stdout
    );
    assert!(
        stdout.contains("frame.data"),
        "banner missing plugin dir: {}",
        stdout
    );
}
