//! Build manifest emission — Plugin Contracts v2 (RFC, Accepted 2026-06-09).
//!
//! See `foundation/spec/plugins/contracts/artifacts.md` §5–§7 for the contract.
//!
//! Every compiler build emits a `build-manifest.json` listing every output the
//! build produced (the main WASM, `frontend.wasm`, future plugin-declared
//! artifacts). Hosts read this file at startup to locate artifacts by their
//! declared `purpose` instead of probing the filesystem for conventional
//! names. This closes SRV004 (clean-server looking for `frontend.wasm` in CWD
//! while `cln build` wrote it next to `app.wasm`).
//!
//! Phase B status: the manifest is emitted unconditionally, including for
//! v1.0.0 plugins that have not yet migrated. Hosts can begin consuming it
//! before any plugin opts into v2. See contracts/migration.md §4.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Canonical filename for the manifest. Hosts look for this exact name as a
/// sibling of the main WASM. See contracts/artifacts.md §5.
pub const BUILD_MANIFEST_FILENAME: &str = "build-manifest.json";

/// Top-level manifest record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildManifest {
    /// Schema version of this manifest. Bumped only on breaking changes to
    /// the JSON shape (not on content changes).
    pub schema_version: String,
    /// Compiler version that produced the artifacts.
    pub compiler_version: String,
    /// Plugin contract version observed by the compiler at build time.
    /// See `crate::plugins::plugin_abi::PLUGIN_CONTRACT_VERSION`.
    pub contract_version: String,
    /// Clean runtime ABI version emitted into module imports.
    pub runtime_abi_version: String,
    /// Unix epoch seconds when the build finished.
    pub generated_at_epoch_seconds: u64,
    /// One entry per output file the build produced.
    pub artifacts: Vec<BuildArtifact>,
    /// Plugin Contracts v2 — resolved callback contracts from
    /// `[bridge.functions.callback]` blocks across all loaded plugins.
    /// Empty when no plugin declares callbacks.
    /// See contracts/bridge-host-classes.md §4.
    #[serde(default)]
    pub callbacks: Vec<CallbackContract>,
    /// Plugin Contracts v2 — compiler-managed key→value store populated
    /// during the build via the `_build_state_set` bridge in the plugin
    /// sandbox. Lets `expand_block` calls communicate discoveries (e.g.
    /// registered component tags) to later lifecycle slot calls AND
    /// surfaces that state to hosts at startup so they can resolve
    /// callbacks without re-parsing every plugin.toml.
    /// See contracts/lifecycle.md §2.5.
    ///
    /// Keys should be plugin-namespaced (`frame.ui:components`,
    /// `frame.server:routes`). Keys beginning with `__compiler:` are
    /// reserved for the compiler's own use.
    #[serde(default)]
    pub build_state: std::collections::BTreeMap<String, String>,
}

/// Resolved callback contract from a `[bridge.functions.callback]` block.
/// Plugin Contracts v2 — `foundation/spec/plugins/contracts/bridge-host-classes.md` §4.
///
/// Hosts read this list at startup to learn which bridge functions dispatch
/// back into the WASM module and how (e.g. `_ui_render_page` looking up
/// `{tagname}_render` exports for the `component_tag_render` purpose).
/// This replaces hand-coded host knowledge of plugin-specific symbols.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackContract {
    /// Bridge function the callback is attached to (e.g. `_ui_render_page`).
    pub bridge: String,
    /// Documented purpose. Values from contracts/bridge-host-classes.md §4.1.
    /// Examples: `"component_tag_render"`, `"route_dispatch"`, `"migration_apply"`,
    /// `"event_dispatch"`.
    pub purpose: String,
    /// Plugin whose exports the host should look up
    /// (e.g. `"frame.ui"` for `component_tag_render`).
    pub plugin_target: String,
    /// How the host finds the right export.
    /// Values: `"exports_matching"`, `"manifest_lookup"`, `"explicit_argument"`.
    pub discovery: String,
    /// Symbol pattern with `{placeholder}` substitution when
    /// `discovery == "exports_matching"`.
    pub export_pattern: Option<String>,
    /// Fallback when no matching export is found.
    /// Values: `"passthrough"`, `"error"`, `"empty"`.
    pub fallback: String,
    /// Plugin that declared this callback. Lets the host attribute the
    /// contract back to its source when diagnosing mismatches.
    pub declared_by_plugin: String,
}

/// A single artifact (output file) the build produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildArtifact {
    /// Canonical file name as the host expects to find it (`app.wasm`,
    /// `frontend.wasm`, `components.json`).
    pub name: String,
    /// Path of the file relative to the build output directory. Hosts
    /// resolve this against the directory of the WASM they were given.
    pub path_relative: String,
    /// Documented purpose. Values match contracts/artifacts.md §4.
    /// `main_module`, `client_hydration`, `static_asset`, `manifest`,
    /// `data_migration`.
    pub purpose: String,
    /// Whether the artifact is intended to be served to web clients.
    pub public: bool,
    /// MIME type. Hosts use this when serving the artifact at `/{name}`.
    pub content_type: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// SHA-256 of the artifact bytes (lowercase hex). Hosts may use this for
    /// caching and integrity checks.
    pub sha256: String,
    /// Plugin name that produced this artifact, when the source was a
    /// `[[artifacts]]` declaration. `null` for compiler-produced artifacts
    /// like the main WASM.
    pub source_plugin: Option<String>,
}

impl BuildManifest {
    /// Construct a new manifest with current compiler/contract/abi versions.
    pub fn new(compiler_version: impl Into<String>) -> Self {
        Self {
            schema_version: "1.0.0".to_string(),
            compiler_version: compiler_version.into(),
            contract_version: crate::plugins::plugin_abi::PLUGIN_CONTRACT_VERSION.to_string(),
            runtime_abi_version: crate::plugins::plugin_abi::DEFAULT_RUNTIME_ABI_VERSION
                .to_string(),
            generated_at_epoch_seconds: now_epoch_seconds(),
            artifacts: Vec::new(),
            callbacks: Vec::new(),
            build_state: std::collections::BTreeMap::new(),
        }
    }

    /// Add a resolved callback contract to the manifest.
    pub fn add_callback(&mut self, callback: CallbackContract) {
        self.callbacks.push(callback);
    }

    /// Populate the manifest's build state from a snapshot of the
    /// compiler-owned key/value store. Called at end of build per
    /// contracts/lifecycle.md §2.5.3.
    pub fn set_build_state(&mut self, state: std::collections::BTreeMap<String, String>) {
        self.build_state = state;
    }

    /// Add an artifact entry. Computes size and SHA-256 from the bytes.
    pub fn add_artifact(
        &mut self,
        name: impl Into<String>,
        path_relative: impl Into<String>,
        purpose: impl Into<String>,
        public: bool,
        content_type: impl Into<String>,
        bytes: &[u8],
        source_plugin: Option<String>,
    ) {
        self.artifacts.push(BuildArtifact {
            name: name.into(),
            path_relative: path_relative.into(),
            purpose: purpose.into(),
            public,
            content_type: content_type.into(),
            size_bytes: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            source_plugin,
        });
    }

    /// Serialize and write the manifest as a sibling of the main WASM at
    /// `<main_wasm_dir>/build-manifest.json`. Returns the resolved path.
    pub fn write_alongside(&self, main_wasm_path: &Path) -> std::io::Result<PathBuf> {
        let dir = main_wasm_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty());
        let manifest_path = match dir {
            Some(d) => d.join(BUILD_MANIFEST_FILENAME),
            None => PathBuf::from(BUILD_MANIFEST_FILENAME),
        };
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(&manifest_path, json)?;
        Ok(manifest_path)
    }
}

fn now_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in result {
        hex.push_str(&format!("{:02x}", byte));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_serializes_with_expected_shape() {
        let mut m = BuildManifest::new("0.30.256");
        m.add_artifact(
            "app.wasm",
            "app.wasm",
            "main_module",
            false,
            "application/wasm",
            b"WASM_BYTES_PLACEHOLDER",
            None,
        );

        let json = serde_json::to_string(&m).unwrap();
        // Contract field names every host must be able to read.
        assert!(json.contains("\"schema_version\""));
        assert!(json.contains("\"compiler_version\":\"0.30.256\""));
        assert!(json.contains("\"contract_version\""));
        assert!(json.contains("\"runtime_abi_version\""));
        assert!(json.contains("\"artifacts\""));
        // Artifact fields.
        assert!(json.contains("\"name\":\"app.wasm\""));
        assert!(json.contains("\"purpose\":\"main_module\""));
        assert!(json.contains("\"size_bytes\":22"));
        // SHA-256 is 64 hex chars.
        let parsed: BuildManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.artifacts[0].sha256.len(), 64);
        assert!(parsed.artifacts[0]
            .sha256
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_write_alongside_lands_next_to_main_wasm() {
        let dir = tempfile::tempdir().expect("tempdir");
        let main = dir.path().join("dist").join("app.wasm");
        std::fs::create_dir_all(main.parent().unwrap()).unwrap();
        std::fs::write(&main, b"PLACEHOLDER").unwrap();

        let mut m = BuildManifest::new("0.30.256");
        m.add_artifact(
            "app.wasm",
            "app.wasm",
            "main_module",
            false,
            "application/wasm",
            b"PLACEHOLDER",
            None,
        );

        let manifest_path = m.write_alongside(&main).expect("write");
        assert_eq!(
            manifest_path,
            main.parent().unwrap().join(BUILD_MANIFEST_FILENAME)
        );
        assert!(manifest_path.exists());

        let read_back: BuildManifest =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        assert_eq!(read_back.compiler_version, "0.30.256");
        assert_eq!(read_back.artifacts.len(), 1);
        assert_eq!(read_back.artifacts[0].name, "app.wasm");
    }

    #[test]
    fn test_multiple_artifacts_preserve_order() {
        let mut m = BuildManifest::new("0.30.256");
        m.add_artifact(
            "app.wasm",
            "app.wasm",
            "main_module",
            false,
            "application/wasm",
            b"a",
            None,
        );
        m.add_artifact(
            "frontend.wasm",
            "frontend.wasm",
            "client_hydration",
            true,
            "application/wasm",
            b"b",
            Some("frame.ui".to_string()),
        );

        assert_eq!(m.artifacts.len(), 2);
        assert_eq!(m.artifacts[0].name, "app.wasm");
        assert_eq!(m.artifacts[1].name, "frontend.wasm");
        assert_eq!(m.artifacts[1].source_plugin.as_deref(), Some("frame.ui"));
        assert!(m.artifacts[1].public);
        assert!(!m.artifacts[0].public);
    }
}
