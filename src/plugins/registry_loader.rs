/*!
 * Function Registry Loader
 *
 * Parses `foundation/platform-architecture/function-registry.toml` — the
 * single source of truth for every host bridge function the compiler may
 * emit as a WASM import. Plugin manifests declare a subset of these
 * functions in their `[bridge] functions` sections; this loader provides
 * the index used to validate that each declared function exists in the
 * registry with matching params/returns.
 *
 * # Path resolution
 *
 * 1. `CLEAN_FUNCTION_REGISTRY` env var (absolute path) — for dev overrides.
 * 2. Embedded copy baked at build time with `include_str!`. This is the
 *    fallback used by installed `cln` binaries that ship without the
 *    `foundation/` workspace next to them.
 */
use std::collections::HashMap;

use serde::Deserialize;

/// Baked-in copy of the registry. Rebuilt every time the compiler is built,
/// so an installed `cln` always carries the registry it was compiled against.
const EMBEDDED_REGISTRY: &str =
    include_str!("../../../foundation/platform-architecture/function-registry.toml");

#[derive(Debug, Clone, Deserialize)]
struct RegistryDocument {
    #[serde(default)]
    meta: RegistryMeta,
    #[serde(default)]
    functions: Vec<RegistryFunction>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RegistryMeta {
    #[serde(default)]
    version: String,
}

/// One `[[functions]]` entry from the registry.
#[derive(Debug, Clone, Deserialize)]
pub struct RegistryFunction {
    pub name: String,
    #[serde(default)]
    pub layer: u8,
    #[serde(default)]
    pub module: String,
    #[serde(default)]
    pub params: Vec<String>,
    #[serde(default)]
    pub returns: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Indexed view over the registry. Lookup is by canonical name OR alias —
/// plugin manifests sometimes use the dot-notation alias.
#[derive(Debug, Clone)]
pub struct RegistryIndex {
    version: String,
    by_lookup_name: HashMap<String, RegistryFunction>,
}

impl RegistryIndex {
    /// Load from `CLEAN_FUNCTION_REGISTRY` env var, falling back to the
    /// embedded copy baked at compiler build time.
    pub fn load() -> Result<Self, RegistryError> {
        let source = if let Ok(path) = std::env::var("CLEAN_FUNCTION_REGISTRY") {
            std::fs::read_to_string(&path).map_err(|e| RegistryError::ReadFailed {
                path: path.clone(),
                source: e.to_string(),
            })?
        } else {
            EMBEDDED_REGISTRY.to_string()
        };
        Self::from_toml_str(&source)
    }

    pub fn from_toml_str(source: &str) -> Result<Self, RegistryError> {
        let doc: RegistryDocument =
            toml::from_str(source).map_err(|e| RegistryError::ParseFailed(e.to_string()))?;
        let mut by_lookup_name = HashMap::with_capacity(doc.functions.len() * 2);
        for f in &doc.functions {
            by_lookup_name.insert(f.name.clone(), f.clone());
            for alias in &f.aliases {
                by_lookup_name
                    .entry(alias.clone())
                    .or_insert_with(|| f.clone());
            }
        }
        Ok(Self {
            version: doc.meta.version,
            by_lookup_name,
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn lookup(&self, name: &str) -> Option<&RegistryFunction> {
        self.by_lookup_name.get(name)
    }

    /// Verify that a plugin-declared bridge function matches the registry.
    /// Returns a list of human-readable mismatches; empty means conformant.
    pub fn check_bridge(
        &self,
        plugin_name: &str,
        decl: &crate::plugins::plugin_abi::BridgeFunction,
    ) -> Vec<String> {
        let mut issues = Vec::new();
        let Some(reg) = self.lookup(&decl.name) else {
            issues.push(format!(
                "  - {plugin_name}/{}: not declared in function-registry.toml. \
Either add it to the registry (with developer approval) or remove the \
declaration from this plugin.",
                decl.name,
            ));
            return issues;
        };

        // Compare WASM-level shapes, not type designators. The same i32 may
        // be described as `"string"` (with expand_strings=false), `"i32"`,
        // `"boolean"`, etc. — they all emit a single i32 import. Only the
        // expanded shape is what the linker actually checks.
        //
        // The registry uses the "expand" convention by default (per the
        // header doc: `"string" -> WASM (i32, i32)`), so we expand its
        // params with `expand_strings=true`. The plugin side respects
        // whatever flag the manifest declares.
        let plugin_shape = params_to_wasm_shape(&decl.params, decl.expand_strings);
        let registry_shape = params_to_wasm_shape(&reg.params, true);
        if plugin_shape != registry_shape {
            issues.push(format!(
                "  - {plugin_name}/{}: params {:?} (expand_strings={}) emit WASM {:?}, registry {:?} expects WASM {:?}",
                decl.name,
                decl.params,
                decl.expand_strings,
                plugin_shape,
                reg.params,
                registry_shape,
            ));
        }

        let plugin_ret_norm = normalize_return(&decl.returns);
        let registry_ret_norm = normalize_return(&reg.returns);
        if plugin_ret_norm != registry_ret_norm {
            issues.push(format!(
                "  - {plugin_name}/{}: returns {:?} does not match registry {:?}",
                decl.name, decl.returns, reg.returns,
            ));
        }

        // Hosts: declared hosts must be a subset of what the registry permits.
        if !reg.hosts.is_empty() && !reg.hosts.iter().any(|h| h == "all") {
            if let Some(plugin_hosts) = &decl.hosts {
                for h in plugin_hosts {
                    if h != "all" && !reg.hosts.iter().any(|rh| rh == h) {
                        issues.push(format!(
                            "  - {plugin_name}/{}: host {:?} not permitted by registry {:?}",
                            decl.name, h, reg.hosts,
                        ));
                    }
                }
            }
        }

        issues
    }
}

/// "string" returns and "ptr" returns are equivalent at the WASM level
/// (both single i32 pointing at a length-prefixed buffer); the registry
/// uses "ptr" mechanistically while plugin authors often write "string"
/// semantically. We treat them as the same shape for validation.
fn normalize_return(t: &str) -> &'static str {
    match strip_tag(t) {
        "string" | "ptr" => "i32_ptr",
        "integer" | "i64" => "i64",
        "number" | "f64" => "f64",
        "boolean" | "i32" => "i32",
        "void" | "" => "void",
        _ => "unknown",
    }
}

/// Expand a list of bridge param type designators into the sequence of WASM
/// primitive types the compiler will emit when generating the import. This
/// is the WASM-level "shape" that must match between plugin manifest and
/// host implementation — it's what the wasmtime linker checks against the
/// registered function.
///
/// `"string"` expands differently depending on `expand_strings`:
/// - `true`: the compiler builds a wrapper that unpacks each Clean string
///   into a (ptr, len) pair, so the raw import takes two i32s per string.
/// - `false`: the compiler passes a single i32 pointer to the length-
///   prefixed string already in memory. No wrapper, no unpacking.
///
/// All other primitives expand 1:1.
fn params_to_wasm_shape(params: &[String], expand_strings: bool) -> Vec<&'static str> {
    let mut shape = Vec::with_capacity(params.len() * 2);
    for p in params {
        match strip_tag(p) {
            "string" if expand_strings => {
                shape.push("i32"); // ptr
                shape.push("i32"); // len
            }
            "string" | "ptr" => shape.push("i32"), // single lp-ptr
            "integer" | "i64" => shape.push("i64"),
            "number" | "f64" => shape.push("f64"),
            "boolean" | "i32" | "handler" => shape.push("i32"),
            "void" | "" => {} // void params don't exist in WASM
            _ => shape.push("unknown"),
        }
    }
    shape
}

fn strip_tag(t: &str) -> &str {
    t.split(':').next().unwrap_or(t)
}

/// Validation policy for `CLEAN_PLUGIN_REGISTRY_VALIDATION` env var.
///
/// Used by both `PluginRegistryBuilder::build` (compile-time hard-error gate)
/// and the `framework_plugins_match_registry` integration test (CI gate).
///
/// # Grammar
/// - unset, empty, or `"off"` → `Off` (no validation, current default)
/// - `"all"` → `All` (validate every plugin)
/// - comma-separated names, e.g. `"frame.data"` or `"frame.data,frame.auth"` →
///   `Allowlist(set)` (only the named plugins are validated; others pass freely)
///
/// Whitespace around items is trimmed. Items are case-sensitive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationPolicy {
    Off,
    All,
    Allowlist(std::collections::HashSet<String>),
}

impl ValidationPolicy {
    /// Read the policy from the `CLEAN_PLUGIN_REGISTRY_VALIDATION` env var.
    pub fn from_env() -> Self {
        Self::from_raw(
            std::env::var("CLEAN_PLUGIN_REGISTRY_VALIDATION")
                .ok()
                .as_deref(),
        )
    }

    /// Parse a raw value (as would come from the env var). Exposed for tests.
    pub fn from_raw(raw: Option<&str>) -> Self {
        let Some(s) = raw else {
            return Self::Off;
        };
        let trimmed = s.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("off") {
            return Self::Off;
        }
        if trimmed.eq_ignore_ascii_case("all") {
            return Self::All;
        }
        let set: std::collections::HashSet<String> = trimmed
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if set.is_empty() {
            Self::Off
        } else {
            Self::Allowlist(set)
        }
    }

    /// Returns true if the given plugin should be validated under this policy.
    pub fn includes(&self, plugin_name: &str) -> bool {
        match self {
            Self::Off => false,
            Self::All => true,
            Self::Allowlist(set) => set.contains(plugin_name),
        }
    }

    /// True if this policy validates at least one plugin.
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Off)
    }
}

#[derive(Debug, Clone)]
pub enum RegistryError {
    ReadFailed { path: String, source: String },
    ParseFailed(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::ReadFailed { path, source } => {
                write!(
                    f,
                    "failed to read function-registry.toml at {path}: {source}"
                )
            }
            RegistryError::ParseFailed(msg) => {
                write!(f, "failed to parse function-registry.toml: {msg}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_registry_parses() {
        let idx = RegistryIndex::load().expect("embedded registry must parse");
        assert!(
            !idx.version.is_empty(),
            "[meta] version must be set in function-registry.toml"
        );
        assert!(
            idx.lookup("print").is_some(),
            "core builtin `print` must be in the registry"
        );
    }

    #[test]
    fn alias_lookup_resolves_to_canonical() {
        let idx = RegistryIndex::load().expect("registry loads");
        let canonical = idx.lookup("_db_query");
        let aliased = idx.lookup("db.query");
        assert!(canonical.is_some(), "canonical _db_query missing");
        assert_eq!(
            canonical.map(|f| &f.name),
            aliased.map(|f| &f.name),
            "alias must resolve to the canonical entry"
        );
    }

    #[test]
    fn normalize_return_equates_string_and_ptr() {
        assert_eq!(normalize_return("string"), normalize_return("ptr"));
    }

    #[test]
    fn wasm_shape_string_without_expand_is_single_i32() {
        // Direct-call path: Clean string passed as single i32 length-prefixed
        // pointer. Matches the host that reads via read_string_from_caller(ptr).
        let shape = params_to_wasm_shape(&["string".into()], false);
        assert_eq!(shape, vec!["i32"]);
    }

    #[test]
    fn wasm_shape_string_with_expand_is_ptr_len_pair() {
        // Wrapper path: Clean string unpacked into (ptr, len) pair. Matches
        // host that takes (ptr: i32, len: i32) and reads via read_raw_string.
        let shape = params_to_wasm_shape(&["string".into()], true);
        assert_eq!(shape, vec!["i32", "i32"]);
    }

    #[test]
    fn wasm_shape_session_store_pattern() {
        // _session_store: plugin says ["string"×3, "i32"×2] without expand_strings.
        // Registry says ["i32"×5]. Both must emit the same 5-i32 WASM shape.
        let plugin_shape = params_to_wasm_shape(
            &[
                "string".into(),
                "string".into(),
                "string".into(),
                "i32".into(),
                "i32".into(),
            ],
            false,
        );
        let registry_shape = params_to_wasm_shape(
            &[
                "i32".into(),
                "i32".into(),
                "i32".into(),
                "i32".into(),
                "i32".into(),
            ],
            true,
        );
        assert_eq!(plugin_shape, registry_shape);
        assert_eq!(plugin_shape.len(), 5);
    }

    #[test]
    fn wasm_shape_http_redirect_route_pattern() {
        // _http_redirect_route: plugin says ["string"×3, "i32"] with expand_strings=true.
        // Registry says ["string"×3, "i32"] (registry always uses expand convention).
        // Both must emit 7 i32 (3 ptr+len pairs + 1 i32 status).
        let plugin_shape = params_to_wasm_shape(
            &[
                "string".into(),
                "string".into(),
                "string".into(),
                "i32".into(),
            ],
            true,
        );
        let registry_shape = params_to_wasm_shape(
            &[
                "string".into(),
                "string".into(),
                "string".into(),
                "i32".into(),
            ],
            true,
        );
        assert_eq!(plugin_shape, registry_shape);
        assert_eq!(
            plugin_shape,
            vec!["i32", "i32", "i32", "i32", "i32", "i32", "i32"]
        );
    }

    #[test]
    fn wasm_shape_catches_integer_vs_i32() {
        // Real mismatch: integer (i64) vs i32 must still be flagged.
        let plugin_shape = params_to_wasm_shape(&["integer".into()], false);
        let registry_shape = params_to_wasm_shape(&["i32".into()], true);
        assert_ne!(plugin_shape, registry_shape);
        assert_eq!(plugin_shape, vec!["i64"]);
        assert_eq!(registry_shape, vec!["i32"]);
    }

    #[test]
    fn validation_policy_unset_is_off() {
        assert_eq!(ValidationPolicy::from_raw(None), ValidationPolicy::Off);
        assert_eq!(ValidationPolicy::from_raw(Some("")), ValidationPolicy::Off);
        assert_eq!(
            ValidationPolicy::from_raw(Some("   ")),
            ValidationPolicy::Off
        );
        assert_eq!(
            ValidationPolicy::from_raw(Some("off")),
            ValidationPolicy::Off
        );
        assert_eq!(
            ValidationPolicy::from_raw(Some("OFF")),
            ValidationPolicy::Off
        );
    }

    #[test]
    fn validation_policy_all_means_all() {
        assert_eq!(
            ValidationPolicy::from_raw(Some("all")),
            ValidationPolicy::All
        );
        assert_eq!(
            ValidationPolicy::from_raw(Some(" ALL ")),
            ValidationPolicy::All
        );
        assert!(ValidationPolicy::All.includes("frame.data"));
        assert!(ValidationPolicy::All.includes("anything"));
    }

    #[test]
    fn validation_policy_allowlist_parses_csv() {
        let policy = ValidationPolicy::from_raw(Some("frame.data , frame.auth"));
        assert!(policy.includes("frame.data"));
        assert!(policy.includes("frame.auth"));
        assert!(!policy.includes("frame.ui"));
        assert!(policy.is_active());
    }

    #[test]
    fn validation_policy_off_includes_nothing() {
        assert!(!ValidationPolicy::Off.includes("frame.data"));
        assert!(!ValidationPolicy::Off.is_active());
    }
}
