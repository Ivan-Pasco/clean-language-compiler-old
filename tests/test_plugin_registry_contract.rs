//! Plugin ↔ registry contract test.
//!
//! Verifies that every `[bridge] functions` entry in every framework
//! plugin.toml matches its corresponding entry in
//! `foundation/platform-architecture/function-registry.toml`.
//!
//! This is the cross-component sync guard described in
//! `foundation/management/cross-component-prompts/compiler-adopt-function-registry-feb2026.md`.
//!
//! Failure modes the test catches:
//!   1. Plugin declares a bridge function that does not exist in the registry.
//!   2. Plugin's `params` disagree with the registry's `params`.
//!   3. Plugin's `returns` disagree with the registry's `returns`
//!      (normalized — "string" and "ptr" returns are treated as equivalent).
//!   4. Plugin's `hosts` is not a subset of the registry's `hosts`.

use std::path::{Path, PathBuf};

use clean_language_compiler::plugins::plugin_abi::PluginManifest;
use clean_language_compiler::plugins::registry_loader::{RegistryIndex, ValidationPolicy};

/// Returns the path to the clean-framework plugins directory, or `None` if
/// this checkout is missing the sibling workspace (which is the case for
/// CI runs that build the compiler in isolation).
fn framework_plugins_dir() -> Option<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let candidate = Path::new(manifest_dir)
        .parent()? // up out of clean-language-compiler/
        .join("clean-framework")
        .join("plugins");
    if candidate.is_dir() {
        Some(candidate)
    } else {
        None
    }
}

#[test]
fn registry_loads_with_expected_shape() {
    let idx = RegistryIndex::load().expect("registry must load");
    assert!(
        !idx.version().is_empty(),
        "registry must have [meta] version"
    );
    // A handful of well-known canonical names that have shipped for a long time.
    for canonical in ["print", "_db_query", "_session_get", "math_sin"] {
        assert!(
            idx.lookup(canonical).is_some(),
            "registry missing canonical name `{canonical}` — registry parse may be incomplete"
        );
    }
}

// Gated by `CLEAN_PLUGIN_REGISTRY_VALIDATION`. Default (unset) is `All` since
// 2026-06-15, so by default every framework plugin is validated against the
// registry. Set the env var explicitly to `"off"` for emergency triage when
// a brand-new plugin/registry edit is mid-flight; set to a comma-separated
// allowlist like `frame.data,frame.auth` to narrow the scope.
//
// This mirrors the gating in `PluginRegistryBuilder::build` so a single env
// setting drives both compile-time and test-time enforcement.
#[test]
fn framework_plugins_match_registry() {
    let policy = ValidationPolicy::from_env();
    if !policy.is_active() {
        eprintln!(
            "CLEAN_PLUGIN_REGISTRY_VALIDATION is explicitly `off` — skipping framework drift scan."
        );
        return;
    }
    run_framework_scan(&policy);
}

fn run_framework_scan(policy: &ValidationPolicy) {
    let Some(plugins_dir) = framework_plugins_dir() else {
        eprintln!(
            "skipping: clean-framework/plugins not present alongside this checkout. \
This is fine in CI but means cross-component drift cannot be checked locally."
        );
        return;
    };

    let idx = RegistryIndex::load().expect("registry must load");

    let mut all_issues: Vec<String> = Vec::new();
    let mut plugins_scanned = 0usize;
    let mut bridges_scanned = 0usize;

    let mut entries: Vec<_> = std::fs::read_dir(&plugins_dir)
        .expect("read framework plugins dir")
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let plugin_toml = path.join("plugin.toml");
        if !plugin_toml.is_file() {
            continue;
        }

        let source = std::fs::read_to_string(&plugin_toml).expect("read plugin.toml");
        let manifest: PluginManifest = match toml::from_str(&source) {
            Ok(m) => m,
            Err(e) => {
                all_issues.push(format!(
                    "  - {}: plugin.toml failed to parse: {e}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                ));
                continue;
            }
        };
        let plugin_name = manifest.plugin.name.clone();
        if !policy.includes(&plugin_name) {
            continue;
        }
        plugins_scanned += 1;
        for decl in &manifest.bridge.functions {
            bridges_scanned += 1;
            all_issues.extend(idx.check_bridge(&plugin_name, decl));
        }
    }

    eprintln!("scanned {plugins_scanned} plugins, {bridges_scanned} bridge function declarations");

    assert!(
        all_issues.is_empty(),
        "PLUGIN-REGISTRY-DRIFT: {} mismatch(es) between framework plugin.toml files and \
foundation/platform-architecture/function-registry.toml:\n{}\n\n\
This test runs against the real clean-framework checkout. To fix:\n\
  1. If the plugin is wrong: file a cross-component report for the framework owner.\n\
  2. If the registry is wrong: propose a registry change with developer approval.\n\
Do NOT relax this test to make it pass.",
        all_issues.len(),
        all_issues.join("\n"),
    );
}

#[test]
fn synthetic_plugin_with_unknown_name_is_rejected() {
    let idx = RegistryIndex::load().expect("registry loads");
    let decl = clean_language_compiler::plugins::plugin_abi::BridgeFunction {
        name: "_definitely_not_a_real_bridge_function".to_string(),
        params: vec!["string".to_string()],
        returns: "void".to_string(),
        ..Default::default()
    };
    let issues = idx.check_bridge("synth.test", &decl);
    assert_eq!(
        issues.len(),
        1,
        "expected exactly one issue, got {issues:?}"
    );
    assert!(
        issues[0].contains("not declared in function-registry.toml"),
        "wrong error message: {}",
        issues[0]
    );
}

#[test]
fn synthetic_plugin_with_param_mismatch_is_rejected() {
    let idx = RegistryIndex::load().expect("registry loads");
    // print takes (string) → expands to (i32, i32) at the WASM level.
    // Declare a single `integer` (one i32 after lowering) — fewer i32s than
    // the registry expects, so the mismatch is on shape arity, not type
    // designator. (Pre-SYNC-PLUGIN-DRIFT this asserted `["integer", "integer"]`
    // mismatched, but `integer` now correctly lowers to i32 same as
    // expand_strings on `"string"`, so two-integer would coincidentally
    // match the (i32, i32) shape and miss the test's intent.)
    let decl = clean_language_compiler::plugins::plugin_abi::BridgeFunction {
        name: "print".to_string(),
        params: vec!["integer".to_string()],
        returns: "void".to_string(),
        ..Default::default()
    };
    let issues = idx.check_bridge("synth.test", &decl);
    assert!(
        issues.iter().any(|i| i.contains("params")),
        "expected a params mismatch in issues, got {issues:?}"
    );
}

#[test]
fn synthetic_plugin_with_return_mismatch_is_rejected() {
    let idx = RegistryIndex::load().expect("registry loads");
    // print returns void, not integer
    let decl = clean_language_compiler::plugins::plugin_abi::BridgeFunction {
        name: "print".to_string(),
        params: vec!["string".to_string()],
        returns: "integer".to_string(),
        ..Default::default()
    };
    let issues = idx.check_bridge("synth.test", &decl);
    assert!(
        issues.iter().any(|i| i.contains("returns")),
        "expected a returns mismatch in issues, got {issues:?}"
    );
}

#[test]
fn string_and_ptr_returns_are_treated_as_equivalent() {
    // Regression guard: the registry uses "ptr" for length-prefixed string
    // returns (mechanistic); plugins often use "string" (semantic). Both
    // produce the same WASM signature and must validate identically.
    //
    // Fixture matches the real frame.data plugin.toml: _db_query has
    // expand_strings = true, so string params expand to (ptr, len) pairs at
    // the WASM level — matching what the registry's "string" expands to under
    // its default expand convention.
    let idx = RegistryIndex::load().expect("registry loads");
    let decl = clean_language_compiler::plugins::plugin_abi::BridgeFunction {
        name: "_db_query".to_string(),
        params: vec!["string".to_string(), "string".to_string()],
        returns: "string".to_string(),
        expand_strings: true,
        ..Default::default()
    };
    let issues = idx.check_bridge("synth.test", &decl);
    assert!(
        issues.is_empty(),
        "string/ptr equivalence broken: {issues:?}"
    );
}

/// Build a minimal manifest declaring `print` with the wrong params. The real
/// registry entry for `print` is `(string) -> void`; we declare `(integer) -> void`
/// so any active validation must flag it.
fn drifted_manifest(plugin_name: &str) -> PluginManifest {
    use clean_language_compiler::plugins::plugin_abi::{
        BridgeFunction, PluginBridge, PluginCompatibility, PluginExports, PluginHandles,
        PluginInfo, PluginLanguage,
    };
    PluginManifest {
        plugin: PluginInfo {
            name: plugin_name.to_string(),
            version: "0.0.1".to_string(),
            description: "test fixture".to_string(),
            author: "test".to_string(),
        },
        compatibility: PluginCompatibility::default(),
        handles: PluginHandles {
            blocks: Vec::new(),
            expressions: Vec::new(),
        },
        exports: PluginExports::default(),
        bridge: PluginBridge {
            functions: vec![BridgeFunction {
                name: "print".to_string(),
                // real registry signature is ["string"]; this is deliberate drift
                params: vec!["integer".to_string()],
                returns: "void".to_string(),
                module: "env".to_string(),
                ..Default::default()
            }],
        },
        language: PluginLanguage {
            blocks: Vec::new(),
            keywords: Vec::new(),
            types: Vec::new(),
            functions: Vec::new(),
            completions: Vec::new(),
            owns_paths: Vec::new(),
        },
        ai: Default::default(),
        paths: Default::default(),
        enforcement: Default::default(),
        memory: Default::default(),
        build: Default::default(),
        lifecycle: Default::default(),
        artifacts: Vec::new(),
        blocks: Default::default(),
    }
}

#[test]
fn build_succeeds_when_policy_is_off() {
    use clean_language_compiler::plugins::PluginRegistryBuilder;
    let result = PluginRegistryBuilder::new()
        .add_manifest(
            "drifted.plugin".to_string(),
            drifted_manifest("drifted.plugin"),
        )
        .with_validation_policy(ValidationPolicy::Off)
        .build();
    assert!(
        result.is_ok(),
        "Off policy must let drifted manifests build"
    );
}

#[test]
fn build_fails_when_policy_targets_drifted_plugin() {
    use clean_language_compiler::plugins::PluginRegistryBuilder;
    let mut allow = std::collections::HashSet::new();
    allow.insert("drifted.plugin".to_string());
    let result = PluginRegistryBuilder::new()
        .add_manifest(
            "drifted.plugin".to_string(),
            drifted_manifest("drifted.plugin"),
        )
        .with_validation_policy(ValidationPolicy::Allowlist(allow))
        .build();
    assert!(
        result.is_err(),
        "Allowlist including the drifted plugin must fail the build"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("PLUGIN-REGISTRY-DRIFT"),
        "expected drift error, got: {err}"
    );
    assert!(
        err.contains("drifted.plugin/print"),
        "expected the specific function flagged, got: {err}"
    );
}

#[test]
fn build_succeeds_when_policy_excludes_drifted_plugin() {
    use clean_language_compiler::plugins::PluginRegistryBuilder;
    // Allowlist only targets a *different* plugin name — drifted.plugin must
    // pass through unchecked even though its declarations are wrong.
    let mut allow = std::collections::HashSet::new();
    allow.insert("some.other.plugin".to_string());
    let result = PluginRegistryBuilder::new()
        .add_manifest(
            "drifted.plugin".to_string(),
            drifted_manifest("drifted.plugin"),
        )
        .with_validation_policy(ValidationPolicy::Allowlist(allow))
        .build();
    assert!(
        result.is_ok(),
        "Allowlist excluding the drifted plugin must allow build to succeed"
    );
}

#[test]
fn build_fails_under_all_policy() {
    use clean_language_compiler::plugins::PluginRegistryBuilder;
    let result = PluginRegistryBuilder::new()
        .add_manifest(
            "drifted.plugin".to_string(),
            drifted_manifest("drifted.plugin"),
        )
        .with_validation_policy(ValidationPolicy::All)
        .build();
    assert!(result.is_err(), "All policy must fail on any drift");
}
