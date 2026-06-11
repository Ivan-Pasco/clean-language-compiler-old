//! Regression test for PLUGIN-BUILD-STAMP (errors.cleanlanguage.dev fp=10a45d39):
//! `cln compile --target=plugin` must stamp `built_with_compiler` into the
//! adjacent plugin.toml's `[build]` section. Without the stamp, every load of
//! the rebuilt plugin emits a "plugin has no build stamp" warning, and the
//! "rebuild with build.sh" hint becomes a misleading feedback loop because
//! rebuilding does nothing visible.
//!
//! The stamp logic lived in `src/bin/cln.rs` which is dead code — the active
//! `cln` binary is built from `src/main.rs`. Fix ported the helper functions
//! and wired the call into `handle_compile`'s post-success path.

use std::process::Command;
use tempfile::TempDir;

/// Path to the locally-built `cln` binary. Cargo sets `CARGO_BIN_EXE_<name>`
/// for any `[[bin]]` target when running integration tests.
fn cln_binary() -> &'static str {
    env!("CARGO_BIN_EXE_cln")
}

/// Build a tree on disk that looks like a real plugin layout:
///   <tmp>/plugin.toml
///   <tmp>/src/main.cln          (compile target)
///   <tmp>/plugin.wasm           (compile output)
fn write_plugin_project(root: &std::path::Path, toml_body: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("plugin.toml"), toml_body).unwrap();
    std::fs::write(
        root.join("src/main.cln"),
        "functions:\n\tstring expand_block(string name, string attrs, string body)\n\t\treturn body\n",
    )
    .unwrap();
}

#[test]
fn plugin_compile_appends_build_section_when_absent() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let toml_body = "[plugin]\nname = \"test.plugin\"\nversion = \"1.0.0\"\n";
    write_plugin_project(root, toml_body);

    let status = Command::new(cln_binary())
        .args([
            "compile",
            "src/main.cln",
            "-o",
            "plugin.wasm",
            "--target=plugin",
        ])
        .current_dir(root)
        .status()
        .expect("failed to run cln");
    assert!(status.success(), "cln compile --target=plugin must succeed");

    let stamped = std::fs::read_to_string(root.join("plugin.toml")).unwrap();
    assert!(
        stamped.contains("[build]"),
        "plugin.toml must gain a [build] section: got\n{}",
        stamped
    );
    assert!(
        stamped.contains("built_with_compiler"),
        "plugin.toml must contain built_with_compiler: got\n{}",
        stamped
    );
    assert!(
        stamped.contains(env!("CARGO_PKG_VERSION")),
        "built_with_compiler must record this compiler's version ({}): got\n{}",
        env!("CARGO_PKG_VERSION"),
        stamped
    );
}

#[test]
fn plugin_compile_updates_existing_build_stamp() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let toml_body = "[plugin]\nname = \"test.plugin\"\nversion = \"1.0.0\"\n\n\
                     [build]\nbuilt_with_compiler = \"0.0.1\"\n";
    write_plugin_project(root, toml_body);

    let status = Command::new(cln_binary())
        .args([
            "compile",
            "src/main.cln",
            "-o",
            "plugin.wasm",
            "--target=plugin",
        ])
        .current_dir(root)
        .status()
        .expect("failed to run cln");
    assert!(status.success(), "cln compile --target=plugin must succeed");

    let stamped = std::fs::read_to_string(root.join("plugin.toml")).unwrap();
    assert!(
        !stamped.contains("\"0.0.1\""),
        "stale stamp 0.0.1 must be replaced: got\n{}",
        stamped
    );
    assert!(
        stamped.contains(env!("CARGO_PKG_VERSION")),
        "current compiler version ({}) must be present after replace: got\n{}",
        env!("CARGO_PKG_VERSION"),
        stamped
    );
}

#[test]
fn compile_without_target_flag_still_stamps_adjacent_plugin_toml() {
    // Framework build.sh scripts are inconsistent about passing --target=plugin
    // (frame.auth, frame.canvas, frame.data don't). Presence of plugin.toml
    // next to the output is itself the signal that this is a plugin build,
    // so we stamp unconditionally on successful compile. False positives are
    // harmless — the stamp affects nothing except plugin loading.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let toml_body = "[plugin]\nname = \"flagless.plugin\"\nversion = \"1.0.0\"\n";
    write_plugin_project(root, toml_body);

    let status = Command::new(cln_binary())
        .args(["compile", "src/main.cln", "-o", "out.wasm"])
        .current_dir(root)
        .status()
        .expect("failed to run cln");
    assert!(status.success(), "cln compile must succeed");

    let stamped = std::fs::read_to_string(root.join("plugin.toml")).unwrap();
    assert!(
        stamped.contains("built_with_compiler"),
        "plugin.toml adjacent to the output must be stamped even without --target=plugin: got\n{}",
        stamped
    );
}
