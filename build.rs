//! Vendors `foundation/platform-architecture/function-registry.toml` into
//! `src/plugins/function-registry.toml` when the sibling foundation
//! workspace is present.
//!
//! - Local dev (foundation present): auto-syncs the vendored copy from the
//!   source of truth, ensuring `include_str!` always picks up edits.
//! - CI (foundation absent): the committed vendored copy is used as-is.
//!
//! Updating the foundation registry locally and rebuilding will refresh the
//! vendored file; the developer is responsible for committing the resulting
//! `src/plugins/function-registry.toml` change.

use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let foundation_registry = manifest_dir
        .parent()
        .map(|p| p.join("foundation/platform-architecture/function-registry.toml"));
    let vendored_registry = manifest_dir.join("src/plugins/function-registry.toml");

    if let Some(src) = foundation_registry {
        if src.is_file() {
            let bytes = std::fs::read(&src)
                .unwrap_or_else(|e| panic!("read foundation registry at {}: {e}", src.display()));
            // Avoid spurious rebuilds when content is unchanged.
            let existing = std::fs::read(&vendored_registry).unwrap_or_default();
            if existing != bytes {
                std::fs::write(&vendored_registry, &bytes).unwrap_or_else(|e| {
                    panic!(
                        "write vendored registry at {}: {e}",
                        vendored_registry.display()
                    )
                });
            }
            println!("cargo:rerun-if-changed={}", src.display());
        }
    }
    println!("cargo:rerun-if-changed={}", vendored_registry.display());
    println!("cargo:rerun-if-changed=build.rs");
}
