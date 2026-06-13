//! Plugin artifact orchestration — Plugin Contracts v2 (RFC Accepted 2026-06-09).
//!
//! See `foundation/spec/plugins/contracts/artifacts.md`.
//!
//! Reads `[[artifacts]]` declarations from loaded plugins, evaluates each
//! entry's `required_when` predicate against the current build state, and
//! produces bytes for each matching artifact via the declared source
//! (`from_module = "client_only_build"`, callback, or static file).
//!
//! Phase C coverage:
//! - Predicates: `"always"`, `"never"`, `"has_client_init"`,
//!   `"has_lifecycle.<slot>"`, `"has_artifact.<name>"`.
//! - Sources: `from_module = "client_only_build"` (the BUILD_FRONTEND closure).
//!   Other sources surface a clear "not supported in this phase" warning and
//!   are skipped. Callback emission and `manifest` auto-generation land in
//!   follow-up commits.
//! - Cycle detection: refuses `from_module = "client_only_build"` when the
//!   active build is itself a client build.

use crate::plugins::plugin_abi::{ArtifactSource, PluginArtifact, PluginManifest};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Result of producing a single artifact.
pub struct EmittedArtifact {
    pub name: String,
    pub path_relative: String,
    pub purpose: String,
    pub public: bool,
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub source_plugin: String,
}

/// Phase C errors surfaced by artifact emission.
#[derive(Debug)]
pub enum ArtifactError {
    /// A nested `client_only_build` was requested from within an active
    /// client build. See `artifacts.md` §6.
    BuildCycle { artifact: String },
    /// The `from_module` value is recognized by the spec but not yet
    /// implemented by the compiler. Phase D will close the gap.
    UnsupportedSource { artifact: String, reason: String },
    /// I/O or compilation failure while producing the artifact bytes.
    EmissionFailed { artifact: String, reason: String },
}

impl std::fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactError::BuildCycle { artifact } => write!(
                f,
                "PLUGIN003: artifact `{}` triggers a nested build cycle (artifacts.md §6)",
                artifact
            ),
            ArtifactError::UnsupportedSource { artifact, reason } => write!(
                f,
                "PLUGIN-ARTIFACT-UNSUPPORTED: artifact `{}` declares an unsupported source: {}",
                artifact, reason
            ),
            ArtifactError::EmissionFailed { artifact, reason } => write!(
                f,
                "PLUGIN-ARTIFACT-EMIT-FAILED: artifact `{}` could not be produced: {}",
                artifact, reason
            ),
        }
    }
}

/// Evaluate a `required_when` predicate against the current build state.
/// See `artifacts.md` §7.2.
///
/// Supported predicates:
/// - `"always"` — produce.
/// - `"never"` — skip.
/// - `"has_client_init"` — any plugin declares the `client_init` lifecycle slot.
/// - `"has_lifecycle.<slot>"` — any plugin declares the named slot
///   (slots: `module_helpers`, `program_init`, `client_init`, `server_init`,
///   `per_request`, `artifact_emitters`).
/// - `"has_artifact.<name>"` — an artifact with the given `name` has already
///   been emitted earlier in the orchestration pass.
/// - `"has_build_state.<key>"` — the build state map from the compile pass
///   contains `<key>`. Requires `build_state` to be populated in the
///   `EmitContext`; evaluates to `false` (safe-degrade) when the map is empty.
///   (ARTIFACT_PREDICATE_NO_BUILD_STATE)
///
/// Unknown predicates default to `false` with a tracing warning so a typo
/// silently skips the artifact rather than producing it incorrectly.
pub fn evaluate_required_when(
    predicate: &str,
    loaded_manifests: &HashMap<String, PluginManifest>,
    artifacts_so_far: &[String],
    build_state: &std::collections::BTreeMap<String, String>,
) -> bool {
    if predicate == "always" {
        return true;
    }
    if predicate == "never" {
        return false;
    }
    if predicate == "has_client_init" {
        return loaded_manifests
            .values()
            .any(|m| m.lifecycle.client_init.is_some());
    }
    if let Some(slot) = predicate.strip_prefix("has_lifecycle.") {
        return loaded_manifests.values().any(|m| match slot {
            "module_helpers" => m.lifecycle.module_helpers.is_some(),
            "program_init" => m.lifecycle.program_init.is_some(),
            "client_init" => m.lifecycle.client_init.is_some(),
            "server_init" => m.lifecycle.server_init.is_some(),
            "per_request" => m.lifecycle.per_request.is_some(),
            "artifact_emitters" => m.lifecycle.artifact_emitters.is_some(),
            _ => false,
        });
    }
    if let Some(artifact) = predicate.strip_prefix("has_artifact.") {
        return artifacts_so_far.iter().any(|a| a == artifact);
    }
    if let Some(key) = predicate.strip_prefix("has_build_state.") {
        return build_state.contains_key(key);
    }
    tracing::warn!(
        target: "plugin_contracts_v2",
        predicate = %predicate,
        "unknown required_when predicate — treating as false"
    );
    false
}

/// Substitute `{output_dir}` in an `output_relative` template against the
/// resolved output directory. `artifacts.md` §2 lists this as the only
/// supported substitution.
pub fn resolve_output_relative(template: &str, output_dir: &Path) -> PathBuf {
    let resolved = template.replace("{output_dir}", &output_dir.to_string_lossy());
    PathBuf::from(resolved)
}

/// Context for an artifact-emission pass. Carries flags that gate certain
/// sources (cycle detection) and the data the source types need to operate.
pub struct EmitContext<'a> {
    pub entry_path: &'a Path,
    pub output_dir: &'a Path,
    pub opt_level: u8,
    /// True when this emission is happening from within a nested build
    /// (e.g. a `client_only_build` triggered by a parent build). Used to
    /// refuse `from_module = "client_only_build"` recursion per §6.
    pub in_nested_build: bool,
    /// Build state snapshot from the preceding compile pass. Populated by
    /// plugins via `_build_state_set` during `expand_block`. Enables the
    /// `has_build_state.<key>` predicate in `required_when`. Pass an empty
    /// `BTreeMap::new()` when the state is unavailable — the predicate
    /// safely degrades to false. (ARTIFACT_PREDICATE_NO_BUILD_STATE)
    pub build_state: std::collections::BTreeMap<String, String>,
}

/// Produce bytes for a single artifact according to its declared source.
///
/// Phase C: only `from_module = "client_only_build"` is implemented. All
/// other sources return `UnsupportedSource` so the caller can log and skip.
/// Phase D closes the gap.
pub fn emit_artifact_bytes(
    artifact: &PluginArtifact,
    ctx: &EmitContext,
) -> Result<Vec<u8>, ArtifactError> {
    match &artifact.emit {
        Some(ArtifactSource::Module { from_module }) => match from_module.as_str() {
            "client_only_build" => {
                if ctx.in_nested_build {
                    return Err(ArtifactError::BuildCycle {
                        artifact: artifact.name.clone(),
                    });
                }
                let lib_paths: Vec<PathBuf> = ctx
                    .entry_path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(|p| vec![p.to_path_buf()])
                    .unwrap_or_default();
                crate::compile_multi_file_client_mode(ctx.entry_path, lib_paths, ctx.opt_level)
                    .map_err(|errors| {
                        let msgs: Vec<String> = errors.iter().map(|e| format!("{}", e)).collect();
                        ArtifactError::EmissionFailed {
                            artifact: artifact.name.clone(),
                            reason: format!("client_only_build: {}", msgs.join("; ")),
                        }
                    })
            }
            "server_only_build" => Err(ArtifactError::UnsupportedSource {
                artifact: artifact.name.clone(),
                reason: "from_module = \"server_only_build\" lands in a Phase C follow-up commit"
                    .to_string(),
            }),
            "manifest" => Err(ArtifactError::UnsupportedSource {
                artifact: artifact.name.clone(),
                reason: "from_module = \"manifest\" lands in a Phase C follow-up commit"
                    .to_string(),
            }),
            other => Err(ArtifactError::UnsupportedSource {
                artifact: artifact.name.clone(),
                reason: format!("unknown from_module value: {:?}", other),
            }),
        },
        Some(ArtifactSource::Callback(_)) => Err(ArtifactError::UnsupportedSource {
            artifact: artifact.name.clone(),
            reason: "emit callback (WASM export) lands in a Phase C follow-up commit".to_string(),
        }),
        None => Err(ArtifactError::UnsupportedSource {
            artifact: artifact.name.clone(),
            reason: "static_path source lands in a Phase C follow-up commit".to_string(),
        }),
    }
}

/// Orchestrate the artifact emission pass. Iterates `[[artifacts]]` entries
/// across all loaded plugins in plugin-load order, evaluating each
/// `required_when` predicate and producing matching artifacts.
///
/// Returns the produced artifacts and a list of warnings (unsupported
/// sources, unknown predicates). Hard failures (cycle, emission error)
/// short-circuit and return as the error path.
pub fn orchestrate(
    loaded_manifests: &HashMap<String, PluginManifest>,
    ctx: &EmitContext,
) -> Result<(Vec<EmittedArtifact>, Vec<String>), ArtifactError> {
    let mut emitted = Vec::new();
    let mut warnings = Vec::new();
    let mut emitted_names: Vec<String> = Vec::new();

    for (plugin_name, manifest) in loaded_manifests {
        for artifact in &manifest.artifacts {
            if !evaluate_required_when(
                &artifact.required_when,
                loaded_manifests,
                &emitted_names,
                &ctx.build_state,
            ) {
                continue;
            }
            match emit_artifact_bytes(artifact, ctx) {
                Ok(bytes) => {
                    let resolved_path =
                        resolve_output_relative(&artifact.output_relative, ctx.output_dir);
                    let path_relative =
                        relative_path_from_output_dir(&resolved_path, ctx.output_dir);
                    emitted.push(EmittedArtifact {
                        name: artifact.name.clone(),
                        path_relative,
                        purpose: artifact.purpose.clone(),
                        public: artifact.public,
                        content_type: artifact
                            .content_type
                            .clone()
                            .unwrap_or_else(|| infer_content_type(&artifact.name)),
                        bytes,
                        source_plugin: plugin_name.clone(),
                    });
                    emitted_names.push(artifact.name.clone());
                }
                Err(ArtifactError::BuildCycle { .. }) => {
                    return Err(ArtifactError::BuildCycle {
                        artifact: artifact.name.clone(),
                    });
                }
                Err(ArtifactError::EmissionFailed { artifact, reason }) => {
                    return Err(ArtifactError::EmissionFailed { artifact, reason });
                }
                Err(ArtifactError::UnsupportedSource { artifact, reason }) => {
                    let msg = format!(
                        "skipping artifact `{}` (declared by `{}`): {}",
                        artifact, plugin_name, reason
                    );
                    tracing::warn!(target: "plugin_contracts_v2", "{}", msg);
                    warnings.push(msg);
                }
            }
        }
    }
    Ok((emitted, warnings))
}

fn relative_path_from_output_dir(resolved: &Path, output_dir: &Path) -> String {
    resolved
        .strip_prefix(output_dir)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| {
            resolved
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("artifact")
                .to_string()
        })
}

fn infer_content_type(name: &str) -> String {
    if name.ends_with(".wasm") {
        "application/wasm".to_string()
    } else if name.ends_with(".json") {
        "application/json".to_string()
    } else if name.ends_with(".css") {
        "text/css".to_string()
    } else if name.ends_with(".html") {
        "text/html".to_string()
    } else if name.ends_with(".js") {
        "application/javascript".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::plugin_abi::{
        PluginCompatibility, PluginHandles, PluginInfo, PluginLifecycle,
    };

    fn manifest_with_lifecycle(name: &str, lifecycle: PluginLifecycle) -> PluginManifest {
        PluginManifest {
            plugin: PluginInfo {
                name: name.to_string(),
                version: "1.0.0".to_string(),
                description: String::new(),
                author: String::new(),
            },
            compatibility: PluginCompatibility::default(),
            handles: PluginHandles {
                blocks: vec![],
                expressions: vec![],
            },
            exports: Default::default(),
            bridge: Default::default(),
            language: Default::default(),
            ai: Default::default(),
            paths: Default::default(),
            enforcement: Default::default(),
            memory: Default::default(),
            build: Default::default(),
            lifecycle,
            artifacts: Vec::new(),
        }
    }

    /// Issue #2 from `compiler-frame-ui-v2-followup-trap-and-silent-orchestrator.md`:
    /// confirm that when a manifest declares `[[artifacts]] required_when = "has_client_init"`
    /// AND a separate plugin manifest declares `lifecycle.client_init`, the orchestrator
    /// invokes `evaluate_required_when` for the artifact and the predicate returns true.
    ///
    /// Guards the silent-no-emit codepath: prior to this test, an artifact could be
    /// declared with `required_when = "has_client_init"` and the orchestrator would
    /// emit nothing because either (a) the predicate didn't see the lifecycle slot
    /// or (b) the source type fell through to `UnsupportedSource` and the warning
    /// was swallowed.
    #[test]
    fn test_orchestrate_invokes_required_when_for_has_client_init() {
        // Plugin A declares the artifact with required_when = "has_client_init".
        // Plugin B declares the lifecycle.client_init slot.
        // Both predicate truthiness AND orchestrator invocation are verified.
        let mut declarer = manifest_with_lifecycle("plugin.declarer", PluginLifecycle::default());
        declarer.artifacts.push(PluginArtifact {
            name: "frontend.wasm".to_string(),
            purpose: "client_hydration".to_string(),
            emit: Some(ArtifactSource::Module {
                from_module: "manifest".to_string(), // unsupported in Phase C
            }),
            static_path: None,
            output_relative: "{output_dir}/frontend.wasm".to_string(),
            required_when: "has_client_init".to_string(),
            public: true,
            cache: "build_input_hash".to_string(),
            content_type: Some("application/wasm".to_string()),
        });

        let opter_in = manifest_with_lifecycle(
            "plugin.optin",
            PluginLifecycle {
                client_init: Some("emit_client_init".to_string()),
                ..Default::default()
            },
        );

        let mut manifests = HashMap::new();
        manifests.insert("plugin.declarer".to_string(), declarer);
        manifests.insert("plugin.optin".to_string(), opter_in);

        // 1. The predicate ALONE returns true when one plugin's lifecycle.client_init
        //    is set, even though the declaring plugin's lifecycle is empty.
        assert!(
            evaluate_required_when("has_client_init", &manifests, &[], &Default::default()),
            "has_client_init must see lifecycle.client_init from any loaded plugin"
        );

        // 2. The orchestrator MUST invoke evaluate_required_when for the artifact
        //    (not silently skip the plugin's [[artifacts]] entries). The unsupported
        //    `from_module = "manifest"` source path produces a warning entry that we
        //    can detect — if the warning is present, the orchestrator at least
        //    REACHED the artifact and tried to emit it.
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/whatever.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: false,
            build_state: Default::default(),
        };
        let (emitted, warnings) =
            orchestrate(&manifests, &ctx).expect("orchestrate should not hard-error");
        assert!(
            emitted.is_empty(),
            "manifest source unsupported in Phase C — no artifact bytes expected"
        );
        assert!(
            warnings.iter().any(|w| w.contains("frontend.wasm")),
            "orchestrator must surface warning for unsupported source, NOT silently skip; got: {:?}",
            warnings
        );
    }

    /// Negative companion: if NO plugin declares `lifecycle.client_init`, the
    /// `has_client_init` predicate returns false and the artifact is not produced.
    #[test]
    fn test_orchestrate_skips_artifact_when_predicate_false() {
        let mut declarer = manifest_with_lifecycle("plugin.declarer", PluginLifecycle::default());
        declarer.artifacts.push(PluginArtifact {
            name: "frontend.wasm".to_string(),
            purpose: "client_hydration".to_string(),
            emit: Some(ArtifactSource::Module {
                from_module: "client_only_build".to_string(),
            }),
            static_path: None,
            output_relative: "{output_dir}/frontend.wasm".to_string(),
            required_when: "has_client_init".to_string(),
            public: true,
            cache: "build_input_hash".to_string(),
            content_type: Some("application/wasm".to_string()),
        });

        // No plugin declares client_init. Predicate must short-circuit.
        let mut manifests = HashMap::new();
        manifests.insert("plugin.declarer".to_string(), declarer);

        assert!(
            !evaluate_required_when("has_client_init", &manifests, &[], &Default::default()),
            "predicate must return false when no plugin declares client_init"
        );

        let ctx = EmitContext {
            entry_path: Path::new("/tmp/whatever.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: false,
            build_state: Default::default(),
        };
        let (emitted, warnings) = orchestrate(&manifests, &ctx).expect("orchestrate ok");
        assert!(emitted.is_empty(), "no artifacts when predicate false");
        assert!(
            warnings.is_empty(),
            "no warnings either — artifact correctly skipped (not even reached)"
        );
    }

    #[test]
    fn test_evaluate_always_is_true() {
        let m = HashMap::new();
        assert!(evaluate_required_when(
            "always",
            &m,
            &[],
            &Default::default()
        ));
    }

    #[test]
    fn test_evaluate_never_is_false() {
        let m = HashMap::new();
        assert!(!evaluate_required_when(
            "never",
            &m,
            &[],
            &Default::default()
        ));
    }

    #[test]
    fn test_has_client_init_when_a_plugin_declares_it() {
        let mut manifests = HashMap::new();
        manifests.insert(
            "frame.ui".to_string(),
            manifest_with_lifecycle(
                "frame.ui",
                PluginLifecycle {
                    client_init: Some("emit_ui_client_init".to_string()),
                    ..Default::default()
                },
            ),
        );
        assert!(evaluate_required_when(
            "has_client_init",
            &manifests,
            &[],
            &Default::default()
        ));
    }

    #[test]
    fn test_has_client_init_when_no_plugin_declares_it() {
        let mut manifests = HashMap::new();
        manifests.insert(
            "frame.server".to_string(),
            manifest_with_lifecycle("frame.server", PluginLifecycle::default()),
        );
        assert!(!evaluate_required_when(
            "has_client_init",
            &manifests,
            &[],
            &Default::default()
        ));
    }

    #[test]
    fn test_has_lifecycle_for_arbitrary_slot() {
        let mut manifests = HashMap::new();
        manifests.insert(
            "frame.server".to_string(),
            manifest_with_lifecycle(
                "frame.server",
                PluginLifecycle {
                    server_init: Some("emit_server_init".to_string()),
                    ..Default::default()
                },
            ),
        );
        assert!(evaluate_required_when(
            "has_lifecycle.server_init",
            &manifests,
            &[],
            &Default::default()
        ));
        assert!(!evaluate_required_when(
            "has_lifecycle.client_init",
            &manifests,
            &[],
            &Default::default()
        ));
        // Unknown slot name → false.
        assert!(!evaluate_required_when(
            "has_lifecycle.bogus",
            &manifests,
            &[],
            &Default::default()
        ));
    }

    #[test]
    fn test_has_artifact_checks_prior_emissions() {
        let manifests = HashMap::new();
        let prior = vec!["frontend.wasm".to_string()];
        assert!(evaluate_required_when(
            "has_artifact.frontend.wasm",
            &manifests,
            &prior,
            &Default::default()
        ));
        assert!(!evaluate_required_when(
            "has_artifact.theme.css",
            &manifests,
            &prior,
            &Default::default()
        ));
    }

    #[test]
    fn test_has_build_state_key_present() {
        let manifests = HashMap::new();
        let mut state = std::collections::BTreeMap::new();
        state.insert("has_client_components".to_string(), "true".to_string());
        assert!(
            evaluate_required_when(
                "has_build_state.has_client_components",
                &manifests,
                &[],
                &state
            ),
            "predicate must return true when key is present in build_state"
        );
        assert!(
            !evaluate_required_when("has_build_state.missing_key", &manifests, &[], &state),
            "predicate must return false when key is absent from build_state"
        );
    }

    #[test]
    fn test_has_build_state_empty_map() {
        let manifests = HashMap::new();
        let state = std::collections::BTreeMap::new();
        assert!(
            !evaluate_required_when("has_build_state.any_key", &manifests, &[], &state),
            "predicate must return false against an empty build_state"
        );
    }

    #[test]
    fn test_unknown_predicate_defaults_to_false() {
        let manifests = HashMap::new();
        assert!(!evaluate_required_when(
            "typo",
            &manifests,
            &[],
            &Default::default()
        ));
        assert!(!evaluate_required_when(
            "has_foo_bar",
            &manifests,
            &[],
            &Default::default()
        ));
    }

    #[test]
    fn test_resolve_output_relative_substitutes_output_dir() {
        let resolved = resolve_output_relative("{output_dir}/frontend.wasm", Path::new("dist"));
        assert_eq!(resolved, PathBuf::from("dist/frontend.wasm"));
    }

    #[test]
    fn test_infer_content_type_for_common_extensions() {
        assert_eq!(infer_content_type("frontend.wasm"), "application/wasm");
        assert_eq!(infer_content_type("theme.css"), "text/css");
        assert_eq!(infer_content_type("components.json"), "application/json");
        assert_eq!(infer_content_type("index.html"), "text/html");
        assert_eq!(infer_content_type("loader.js"), "application/javascript");
        assert_eq!(infer_content_type("unknown"), "application/octet-stream");
    }

    #[test]
    fn test_emit_rejects_client_build_inside_nested_build() {
        let artifact = PluginArtifact {
            name: "frontend.wasm".to_string(),
            purpose: "client_hydration".to_string(),
            emit: Some(ArtifactSource::Module {
                from_module: "client_only_build".to_string(),
            }),
            static_path: None,
            output_relative: "{output_dir}/frontend.wasm".to_string(),
            required_when: "always".to_string(),
            public: true,
            cache: "build_input_hash".to_string(),
            content_type: Some("application/wasm".to_string()),
        };
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/whatever.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: true,
            build_state: Default::default(),
        };
        let err = emit_artifact_bytes(&artifact, &ctx).expect_err("must reject");
        matches!(err, ArtifactError::BuildCycle { .. });
    }

    #[test]
    fn test_emit_unsupported_source_returns_clear_error() {
        let artifact = PluginArtifact {
            name: "components.json".to_string(),
            purpose: "manifest".to_string(),
            emit: Some(ArtifactSource::Module {
                from_module: "manifest".to_string(),
            }),
            static_path: None,
            output_relative: "{output_dir}/components.json".to_string(),
            required_when: "always".to_string(),
            public: false,
            cache: "build_input_hash".to_string(),
            content_type: None,
        };
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/whatever.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: false,
            build_state: Default::default(),
        };
        let err = emit_artifact_bytes(&artifact, &ctx).expect_err("must error");
        let msg = format!("{}", err);
        assert!(msg.contains("PLUGIN-ARTIFACT-UNSUPPORTED"));
        assert!(msg.contains("components.json"));
        assert!(msg.contains("manifest"));
    }
}
