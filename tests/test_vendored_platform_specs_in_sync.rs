//! The function registry and the runtime ABI the compiler checks plugins
//! against are vendored copies of `foundation/spec/platform/function-registry.toml`
//! and `foundation/spec/platform/runtime-abi/v1.toml` (build.rs syncs them when
//! the foundation folder is next to the compiler). They must not drift from
//! the authoritative files (Ivan-Pasco/clean-language-compiler-old#72).
//!
//! The comparison is skipped when the foundation folder is not reachable from
//! the checkout (a lone clone in CI); the sync itself then has nothing to do.

use std::path::{Path, PathBuf};

fn foundation_platform_dir() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|dir| dir.join("foundation/spec/platform"))
        .find(|dir| dir.join("function-registry.toml").is_file())
}

fn assert_in_sync(authoritative: &Path, vendored: &str) {
    let vendored_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(vendored);
    let want = std::fs::read_to_string(authoritative)
        .unwrap_or_else(|e| panic!("read {}: {e}", authoritative.display()));
    let have = std::fs::read_to_string(&vendored_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", vendored_path.display()));
    assert!(
        want == have,
        "{vendored} has drifted from {}; copy the foundation file over it",
        authoritative.display()
    );
}

#[test]
fn vendored_function_registry_matches_foundation() {
    let Some(platform) = foundation_platform_dir() else {
        eprintln!("foundation/spec/platform not reachable; skipping");
        return;
    };
    assert_in_sync(
        &platform.join("function-registry.toml"),
        "src/plugins/function-registry.toml",
    );
}

#[test]
fn vendored_runtime_abi_matches_foundation() {
    let Some(platform) = foundation_platform_dir() else {
        eprintln!("foundation/spec/platform not reachable; skipping");
        return;
    };
    assert_in_sync(
        &platform.join("runtime-abi/v1.toml"),
        "src/plugins/runtime-abi-v1.toml",
    );
}

#[test]
fn build_script_reads_the_current_foundation_layout() {
    let build_rs = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs"))
        .expect("read build.rs");
    assert!(
        !build_rs.contains("foundation/platform-architecture"),
        "build.rs must sync from foundation/spec/platform, not the old platform-architecture folder"
    );
    assert!(build_rs.contains("foundation/spec/platform/function-registry.toml"));
    assert!(build_rs.contains("foundation/spec/platform/runtime-abi/v1.toml"));
}
