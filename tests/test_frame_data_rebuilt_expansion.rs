//! Rebuild frame.data (with frame.server and frame.auth) from source with the compiler under
//! test, then compile the framework's frame.data unit tests against those
//! freshly built plugins.
//!
//! Plugins are Clean programs compiled by this compiler, so a code
//! generation bug can break a plugin that was fine when built by the
//! previous release. Testing only against the installed plugins (built by an
//! older compiler) let such a regression ship in 0.33.168: frame.data built
//! by 0.33.168 handed an empty statement to `_emit_stmt_from_source` in
//! `expand_first`, and six of the ten frame.data unit tests failed to
//! expand.
//!
//! Needs the framework checked out as a sibling (`../clean-framework`); the
//! test is skipped, with a note on stderr, when it is absent.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

// Every plugin the frame.data unit tests declare (tenant-scope-test.cln also
// declares frame.auth). On a CI runner nothing is installed in
// ~/.cleen/plugins, so a plugin left out here cannot be found at all.
const REBUILT_PLUGINS: [&str; 3] = ["frame.auth", "frame.data", "frame.server"];

fn framework_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("clean-framework");
    let complete = REBUILT_PLUGINS
        .iter()
        .all(|p| dir.join("plugins").join(p).join("src/main.cln").is_file())
        && dir.join("tests/framework/unit/plugins/data").is_dir();
    complete.then_some(dir)
}

/// Copy `src` into `dst`, leaving out any prebuilt `plugin.wasm`.
fn copy_plugin_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_plugin_tree(&from, &to);
        } else if entry.file_name() != "plugin.wasm" {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// Build one plugin the way its `build.sh` does, into a scratch copy.
fn build_plugin(framework: &Path, name: &str, scratch: &Path) -> PathBuf {
    let dir = scratch.join(name);
    copy_plugin_tree(&framework.join("plugins").join(name), &dir);
    let out = Command::new(env!("CARGO_BIN_EXE_cln"))
        .current_dir(&dir)
        .args([
            "compile",
            "src/main.cln",
            "-o",
            "plugin.wasm",
            "--target=plugin",
        ])
        .output()
        .expect("failed to spawn cln");
    assert!(
        out.status.success() && dir.join("plugin.wasm").is_file(),
        "building {name} with the compiler under test failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    dir
}

/// Plugin names listed in a file's `plugins:` block.
fn declared_plugins(source: &str) -> Vec<String> {
    let mut lines = source.lines().skip_while(|l| l.trim_end() != "plugins:");
    lines.next();
    lines
        .take_while(|l| l.starts_with('\t') || l.starts_with(' '))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[test]
fn frame_data_built_by_this_compiler_expands_the_data_tests() {
    let Some(framework) = framework_dir() else {
        eprintln!("skipped: ../clean-framework with frame.data and frame.server not found");
        return;
    };

    let scratch = TempDir::new().unwrap();
    let overrides: Vec<(&str, String)> = REBUILT_PLUGINS
        .iter()
        .map(|name| {
            let dir = build_plugin(&framework, name, scratch.path());
            (*name, format!("{name}={}", dir.display()))
        })
        .collect();

    let tests_dir = framework.join("tests/framework/unit/plugins/data");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&tests_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "cln"))
        .collect();
    cases.sort();
    assert!(
        !cases.is_empty(),
        "no .cln files in {}",
        tests_dir.display()
    );

    let mut failures = Vec::new();
    for case in &cases {
        let wasm = scratch.path().join("case.wasm");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_cln"));
        cmd.current_dir(scratch.path())
            .arg("compile")
            .arg(case)
            .arg("-o")
            .arg(&wasm);
        // Override only the plugins this case declares: the compiler rejects
        // an override for a plugin no file loads.
        let declared = declared_plugins(&std::fs::read_to_string(case).unwrap());
        for (name, o) in &overrides {
            if declared.iter().any(|d| d == name) {
                cmd.args(["--plugin-override", o]);
            }
        }
        let out = cmd.output().expect("failed to spawn cln");
        if !out.status.success() {
            let stderr = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            );
            let first_error = stderr
                .lines()
                .find(|l| l.starts_with("error"))
                .unwrap_or("(no error line)");
            failures.push(format!(
                "{}: {first_error}",
                case.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "frame.data unit tests failed to compile with plugins built by this compiler:\n{}",
        failures.join("\n")
    );
}
