//! `cln compile --target <host>` writes the module and nothing next to it
//! that pretends to be a host runtime.
//!
//! The compiler used to emit bridge.js + loader.js (browser), bridge.mjs +
//! run.mjs (node), CleanBridge.swift (ios) and CleanBridge.kt (android)
//! beside the .wasm. Those files could not run the module, and the browser
//! one overwrote a real loader.js in the output directory. The targets still
//! select the host class; they no longer write bridge files.

use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

const BRIDGE_FILES: [&str; 6] = [
    "bridge.js",
    "loader.js",
    "bridge.mjs",
    "run.mjs",
    "CleanBridge.swift",
    "CleanBridge.kt",
];

fn file_names(dir: &std::path::Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect()
}

#[test]
fn host_targets_write_no_bridge_files_and_keep_an_existing_loader() {
    for target in ["browser", "web", "node", "nodejs", "ios", "android"] {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(tmp.path().join("main.cln"), "start:\n\tprint(\"hi\")\n").unwrap();
        // A hand-written loader that a browser build must leave alone.
        fs::write(tmp.path().join("loader.js"), "// real loader\n").unwrap();

        let out = Command::new(env!("CARGO_BIN_EXE_cln"))
            .args(["compile", "main.cln", "-o", "out.wasm", "--target", target])
            .env("CLEEN_TELEMETRY_FORCE", "local")
            .current_dir(tmp.path())
            .output()
            .expect("cln runs");
        assert!(
            out.status.success(),
            "--target {target} must compile.\nstderr=\n{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let names = file_names(tmp.path());
        assert!(names.contains("out.wasm"), "--target {target}: {names:?}");
        for bridge in BRIDGE_FILES.iter().filter(|n| **n != "loader.js") {
            assert!(
                !names.contains(*bridge),
                "--target {target} wrote {bridge}: {names:?}"
            );
        }
        assert_eq!(
            fs::read_to_string(tmp.path().join("loader.js")).unwrap(),
            "// real loader\n",
            "--target {target} overwrote loader.js"
        );
    }
}
