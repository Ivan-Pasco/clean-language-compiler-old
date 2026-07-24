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

/// Baked-in copy of the registry. The source of truth lives in the sibling
/// `foundation/platform-architecture/function-registry.toml`, but the file
/// next to this source is a vendored copy committed to the compiler repo so
/// CI (which only checks out clean-language-compiler) can build.
///
/// `build.rs` at the crate root auto-syncs the vendored copy from foundation
/// whenever foundation is present alongside this checkout. When foundation
/// is not present (e.g. CI), the committed copy is used as-is. Developers
/// who edit the foundation copy must commit the resulting vendored update.
const EMBEDDED_REGISTRY: &str = include_str!("function-registry.toml");

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
    pub category: String,
    #[serde(default)]
    pub module: String,
    #[serde(default)]
    pub params: Vec<String>,
    /// Optional semantic names for each entry in `params`, in the same order.
    /// When present, host conformance tests check that the corresponding
    /// host closure uses the same names — this catches argument-order drift
    /// that pure-type checking can't (e.g. swapping `(type_id, size)` for
    /// `(size, _align)` when both expand to `(i32, i32)` at the WASM level).
    #[serde(default)]
    pub param_names: Vec<String>,
    #[serde(default)]
    pub returns: String,
    /// Whether `"string"` params expand to (ptr, len) at the WASM level.
    /// Default `true` matches the registry's "always-expand" convention (see
    /// header of function-registry.toml). Set to `false` for entries whose
    /// canonical plugin declaration ships bare length-prefixed pointers
    /// (`expand_strings = false`) — this lets the compiler describe the slot
    /// as `"string"` at the language level (so `parse_bridge_hir_type` maps
    /// it to `HirType::String`) while still matching the plugin's WASM ABI
    /// for `check_bridge`. Introduced to fix SEM001-JSON-GET-STRING-BROWSER-TARGET
    /// (dashboard fp 7ba4d133b44a).
    #[serde(default = "default_expand_strings")]
    pub expand_strings: bool,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

fn default_expand_strings() -> bool {
    true
}

/// Indexed view over the registry. Lookup is by canonical name OR alias —
/// plugin manifests sometimes use the dot-notation alias.
#[derive(Debug, Clone)]
pub struct RegistryIndex {
    version: String,
    by_lookup_name: HashMap<String, RegistryFunction>,
    canonical_order: Vec<String>,
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
        let mut canonical_order = Vec::with_capacity(doc.functions.len());
        for f in &doc.functions {
            by_lookup_name.insert(f.name.clone(), f.clone());
            canonical_order.push(f.name.clone());
            for alias in &f.aliases {
                by_lookup_name
                    .entry(alias.clone())
                    .or_insert_with(|| f.clone());
            }
        }
        Ok(Self {
            version: doc.meta.version,
            by_lookup_name,
            canonical_order,
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn lookup(&self, name: &str) -> Option<&RegistryFunction> {
        self.by_lookup_name.get(name)
    }

    /// Iterate over every canonical `[[functions]]` entry in registry order.
    /// Each entry appears once (aliases are not yielded). Used by host and
    /// compiler conformance checks to enumerate the full contract surface.
    pub fn functions(&self) -> impl Iterator<Item = &RegistryFunction> {
        self.canonical_order
            .iter()
            .filter_map(move |n| self.by_lookup_name.get(n))
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

        // JSON stdlib migration transitional exception ([P2-cont] compiler
        // 0.33.135). The updated `foundation` registry declares `_json_encode`
        // / `_json_encode_pretty` / `_json_decode` with the new boxed-Any
        // signatures. Installed frame.server plugins predating [P3a] still
        // declare the old string-in / string-out shapes. Rather than fail
        // every compile that references frame.server during the soak window,
        // silently skip the shape + return-type checks for these three names.
        //
        // The host-mismatch check below still runs (bridges must be reachable
        // on the target host). Once [P3a] updates frame.server (and other
        // hosts) to the new shapes, remove this exception — the drift check
        // will then catch any stragglers as intended.
        const JSON_MIGRATION_EXEMPT: &[&str] =
            &["_json_encode", "_json_encode_pretty", "_json_decode"];
        let json_migration_exempt = JSON_MIGRATION_EXEMPT.contains(&decl.name.as_str());

        // Compare WASM-level shapes, not type designators. The same i32 may
        // be described as `"string"` (with expand_strings=false), `"i32"`,
        // `"boolean"`, etc. — they all emit a single i32 import. Only the
        // expanded shape is what the linker actually checks.
        //
        // The registry defaults to the "expand" convention (per the header
        // doc: `"string" -> WASM (i32, i32)`), but individual entries can
        // opt out via `expand_strings = false` in the toml to match plugins
        // that ship bare length-prefixed pointers. The plugin side respects
        // whatever flag its manifest declares.
        let plugin_shape = params_to_wasm_shape(&decl.params, decl.expand_strings);
        let registry_shape = params_to_wasm_shape(&reg.params, reg.expand_strings);
        if plugin_shape != registry_shape && !json_migration_exempt {
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
        if plugin_ret_norm != registry_ret_norm && !json_migration_exempt {
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
    // Clean Language `integer` is i32 (foundation/spec/type-system.md §"Primitive
    // types": unqualified `integer` = Signed 32-bit integer = I32). Functions that
    // genuinely need 64-bit range declare `"i64"` explicitly in the registry
    // entry (e.g. `print_integer`, `_server_sleep`, `_time_now`). Per
    // foundation/spec/plugins/plugin-contract.md §"Type vocabulary", plugin
    // bridge `"integer"` parameters/returns lower to WASM i32 to match what
    // `register_plugin_bridge_imports` actually emits (SYNC-PLUGIN-DRIFT).
    match strip_tag(t) {
        "string" | "ptr" => "i32_ptr",
        // "any" is a pointer to a 12-byte boxed struct
        // (`[tag@0:i32][value1@4:i32][value2@8:i32]`, foundation/spec/type-system.md).
        // At the WASM level it's a single i32 that shares its shape with
        // "string" / "ptr" (all length-prefixed or tagged pointers). We
        // classify it as `i32_ptr` so validation treats it as a pointer
        // rather than a plain integer, and reject accidental cross-wiring
        // between `any`-returning bridges and `integer`-returning hosts.
        "any" => "i32_ptr",
        "i64" => "i64",
        "number" | "f64" => "f64",
        "integer" | "boolean" | "i32" => "i32",
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
    // See `normalize_return` above for the `integer` → i32 rationale (SYNC-PLUGIN-DRIFT).
    let mut shape = Vec::with_capacity(params.len() * 2);
    for p in params {
        match strip_tag(p) {
            "string" if expand_strings => {
                shape.push("i32"); // ptr
                shape.push("i32"); // len
            }
            "string" | "ptr" => shape.push("i32"), // single lp-ptr
            // "any" is a single i32 pointing at a 12-byte boxed struct
            // `[tag@0:i32][value1@4:i32][value2@8:i32]`. The wrapper is a
            // pass-through: the host reads the tag and dispatches. This
            // matches the compiler-stdlib json.get contract
            // (`__json_get_path(obj_boxed_ptr, ...)`) so a plugin that
            // declares `params=["any", "string"]` shares its ABI shape
            // with the builtin json.get and never conflicts with the
            // Any-boxing MIR-builder emission.
            //
            // `expand_strings` is intentionally ignored for `any` — it
            // has no meaning here (no length prefix to unpack; the tag
            // *is* the type discriminator).
            "any" => shape.push("i32"),
            "i64" => shape.push("i64"),
            "number" | "f64" => shape.push("f64"),
            "integer" | "boolean" | "i32" | "handler" => shape.push("i32"),
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
/// - unset → `All` (default since 2026-06-15; was `Off` during the cross-
///   component cleanup tracked in foundation/management/cross-component-prompts/)
/// - explicit `"off"` or empty string → `Off` (escape hatch for emergency
///   fallback when a brand-new plugin/registry edit is mid-flight)
/// - `"all"` → `All` (explicit; same as unset)
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
    ///
    /// Default (unset) is `All` — every plugin manifest must conform to the
    /// registry. To opt out for emergency triage, set the env var explicitly
    /// to `"off"` or the empty string.
    pub fn from_raw(raw: Option<&str>) -> Self {
        let Some(s) = raw else {
            return Self::All;
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
    fn wasm_shape_integer_is_i32_per_spec() {
        // Per foundation/spec/type-system.md §"Primitive types" and
        // plugin-contract.md §"Type vocabulary", unqualified `integer` lowers
        // to WASM i32 — same as the explicit `"i32"` designator. SYNC-PLUGIN-DRIFT
        // arose from this loader previously mapping `"integer"` to i64, which
        // disagreed with what `register_plugin_bridge_imports` actually emits.
        let integer_shape = params_to_wasm_shape(&["integer".into()], false);
        let i32_shape = params_to_wasm_shape(&["i32".into()], true);
        assert_eq!(integer_shape, i32_shape);
        assert_eq!(integer_shape, vec!["i32"]);

        // Real 64-bit integer ABI is opt-in via the explicit `"i64"` designator
        // (used by `print_integer`, `_server_sleep`, `_time_now`).
        let i64_shape = params_to_wasm_shape(&["i64".into()], false);
        assert_eq!(i64_shape, vec!["i64"]);
        assert_ne!(integer_shape, i64_shape);
    }

    #[test]
    fn wasm_shape_any_is_single_i32_ptr_regardless_of_expand_strings() {
        // "any" is a pointer to a 12-byte boxed struct
        // `[tag@0:i32][value1@4:i32][value2@8:i32]`
        // (foundation/spec/type-system.md). At the WASM level it is one
        // i32; `expand_strings` is meaningless here because there is no
        // length prefix to unpack — the *tag* is the discriminator.
        //
        // Regression guard for the framework prompt 4de6f0df /
        // CODEGEN-STRING-ARG-ALIAS-JSONGET follow-up. Before this fix,
        // "any" fell through `_ => shape.push("unknown")` in both
        // `params_to_wasm_shape` and `normalize_return`, so any
        // plugin.toml declaring `params=["any", ...]` (or
        // `returns="any"`) produced an unrepresentable WASM signature
        // and validation-time rejection.
        let no_expand = params_to_wasm_shape(&["any".into()], false);
        let with_expand = params_to_wasm_shape(&["any".into()], true);
        assert_eq!(no_expand, vec!["i32"]);
        assert_eq!(
            no_expand, with_expand,
            "`any` must ignore expand_strings — the tag encodes the type"
        );
    }

    #[test]
    fn wasm_shape_any_matches_string_ptr_shape() {
        // Both `any` and `string` (without expand_strings) lower to a
        // single i32 that points at a length-prefixed / tag-prefixed
        // buffer. The compiler's json.get stdlib is defined against a
        // boxed-Any first arg (`__json_get_path(obj_boxed_ptr, ...)`),
        // and this equivalence lets a plugin.toml bridge declaration
        // `_json_get(any, string) -> any` register a raw WASM import
        // whose shape matches what MIR-builder call sites emit after
        // BoxAny.
        assert_eq!(
            params_to_wasm_shape(&["any".into(), "string".into()], false),
            vec!["i32", "i32"]
        );
    }

    #[test]
    fn normalize_return_any_is_i32_ptr() {
        // `returns = "any"` in plugin.toml must resolve to the same
        // shape as `returns = "string"` / `returns = "ptr"` — a single
        // i32 pointing at a runtime-tagged buffer. Anything else
        // (previously "unknown") produced WASM validation errors when
        // the bridge was invoked.
        assert_eq!(normalize_return("any"), "i32_ptr");
        assert_eq!(normalize_return("any"), normalize_return("string"));
        assert_eq!(normalize_return("any"), normalize_return("ptr"));
    }

    #[test]
    fn validation_policy_unset_is_all() {
        // Default changed 2026-06-15: unset env var → All, since the
        // cross-component cleanup is complete and registry drift is zero.
        // Explicit "off" or empty string remains the escape hatch.
        assert_eq!(ValidationPolicy::from_raw(None), ValidationPolicy::All);
    }

    #[test]
    fn validation_policy_explicit_off_is_off() {
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
