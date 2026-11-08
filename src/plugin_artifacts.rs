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
//!   `"has_lifecycle.<slot>"`, `"has_artifact.<name>"`, `"has_build_state.<key>"`.
//! - Sources:
//!   - `from_module = "client_only_build"` — nested client build (BUILD_FRONTEND).
//!   - `from_module = "manifest"` — JSON aggregated from build_state under the
//!     scoped key `<source_plugin>.<artifact_name>` (or `manifest.<name>`).
//!   - `static_path = "<file>"` — file copy from the plugin's installed dir.
//!   - `emit = "<callback>"` (callback source) — bytes read from build_state
//!     under `<source_plugin>.<artifact_name>`, which the plugin populates
//!     while its blocks expand. The export name itself is not called: the
//!     compiler does not invoke artifact callbacks (see
//!     `emit_static_asset_from_build_state`).
//!   - `from_module = "server_only_build"` is not supported and is reported
//!     as a skipped artifact.
//! - Cycle detection: refuses `from_module = "client_only_build"` when the
//!   active build is itself a client build.

use crate::plugins::plugin_abi::{ArtifactSource, PluginArtifact, PluginManifest};
use indexmap::IndexMap;
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
    /// The declared source is one this compiler does not produce
    /// (`from_module = "server_only_build"`, an unknown `from_module`, or
    /// neither `emit` nor `static_path`). The artifact is skipped with a
    /// warning.
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
///   (slots: `module_helpers`, `program_init`, `client_init`, `server_init`).
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
    loaded_manifests: &IndexMap<String, PluginManifest>,
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
        return loaded_manifests
            .values()
            .any(|m| m.lifecycle.slot_export(slot).is_some());
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
/// supported substitution. A template without the placeholder is relative
/// to the output directory as well.
pub fn resolve_output_relative(template: &str, output_dir: &Path) -> PathBuf {
    match artifact_relative_path(template) {
        Some(relative) => output_dir.join(relative),
        None => PathBuf::from(template.replace("{output_dir}", &output_dir.to_string_lossy())),
    }
}

/// The path, relative to the build output directory, that an
/// `output_relative` template names (`artifacts.md` §2: "Path relative to
/// the build's `--output` argument, with substitution for `{output_dir}`").
///
/// `{output_dir}/a/b.css` and `a/b.css` both name `a/b.css`. Returns `None`
/// for a template that would write outside the output directory: an
/// absolute path, a `..` component, or `{output_dir}` anywhere but at the
/// start.
pub fn artifact_relative_path(template: &str) -> Option<String> {
    let relative = match template.strip_prefix("{output_dir}") {
        Some(rest) => rest.trim_start_matches(['/', '\\']),
        None => template,
    };
    (!relative.contains("{output_dir}") && is_contained_relative_path(relative))
        .then(|| relative.to_string())
}

/// True when `path` is a non-empty relative path made only of normal
/// components (and `.`), so joining it to a directory stays inside it.
fn is_contained_relative_path(path: &str) -> bool {
    use std::path::Component;
    let path = Path::new(path);
    path.components().next().is_some()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
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
    /// Resolved on-disk directory for each loaded plugin, keyed by plugin
    /// name. Used to resolve `static_path` relative file references per
    /// `artifacts.md` §2/§6. A `static_path` artifact whose plugin has no
    /// entry here cannot be produced and fails the build.
    pub plugin_dirs: std::collections::HashMap<String, PathBuf>,
}

/// Produce bytes for a single artifact according to its declared source.
///
/// `source_plugin` is the name of the plugin whose manifest declares this
/// artifact — used to scope `build_state` lookups and resolve `static_path`
/// against `ctx.plugin_dirs`.
///
/// Phase C coverage:
/// - `from_module = "client_only_build"` → nested client build.
/// - `from_module = "manifest"` (purpose: `manifest`) → JSON aggregated from
///   `build_state` entries the plugin populated during `expand_block`
///   (see artifacts.md §4.3, §6). Lookup key:
///   `<source_plugin>.<artifact_name>` first, then `manifest.<artifact_name>`.
///   When neither is present, `{}` is written and a warning is pushed onto
///   `warnings`, so a declared-but-unpopulated manifest is reported without
///   failing the build.
/// - `static_path` (no `emit` field) → file copy from the plugin's installed
///   directory (resolved through `ctx.plugin_dirs`). A missing or unreadable
///   file, or a path leaving the plugin directory, fails the artifact.
/// - `emit = "<callback>"` (`ArtifactSource::Callback`) used for
///   `static_asset` purposes → read pre-computed bytes from `build_state`
///   under `<source_plugin>.<artifact_name>` (plugins push content there
///   from `expand_block`). On a miss an empty file is written and a warning
///   is pushed onto `warnings`.
///
/// Other sources return `UnsupportedSource`.
pub fn emit_artifact_bytes(
    artifact: &PluginArtifact,
    source_plugin: &str,
    ctx: &EmitContext,
    warnings: &mut Vec<String>,
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
                reason: "from_module = \"server_only_build\" is not supported by this \
                         compiler; use client_only_build, manifest, a build_state-backed \
                         `emit`, or static_path"
                    .to_string(),
            }),
            "manifest" => Ok(emit_manifest_bytes(artifact, source_plugin, ctx, warnings)),
            other => Err(ArtifactError::UnsupportedSource {
                artifact: artifact.name.clone(),
                reason: format!("unknown from_module value: {:?}", other),
            }),
        },
        Some(ArtifactSource::Callback(_)) => Ok(emit_static_asset_from_build_state(
            artifact,
            source_plugin,
            ctx,
            warnings,
        )),
        None => {
            // No `emit` field — spec §2 requires either `emit` or `static_path`.
            if let Some(static_path) = artifact.static_path.as_deref() {
                emit_static_asset_from_path(artifact, source_plugin, static_path, ctx)
            } else {
                Err(ArtifactError::UnsupportedSource {
                    artifact: artifact.name.clone(),
                    reason: "artifact declares neither `emit` nor `static_path` (artifacts.md §2)"
                        .to_string(),
                })
            }
        }
    }
}

/// Emit a `purpose = "manifest"` artifact by serializing build_state.
///
/// Lookup order:
/// 1. `<source_plugin>.<artifact_name>` — plugin-scoped, recommended.
/// 2. `manifest.<artifact_name>` — legacy/shared bucket.
///
/// If a value is present, it is written verbatim (plugins are responsible
/// for producing valid JSON). If absent, `{}` is written and a warning is
/// pushed, so a misspelt build_state key is reported without failing the
/// build. (ARTIFACT_MANIFEST_EMPTY_SOURCE)
fn emit_manifest_bytes(
    artifact: &PluginArtifact,
    source_plugin: &str,
    ctx: &EmitContext,
    warnings: &mut Vec<String>,
) -> Vec<u8> {
    let scoped = format!("{}.{}", source_plugin, artifact.name);
    let fallback = format!("manifest.{}", artifact.name);
    if let Some(v) = ctx.build_state.get(&scoped) {
        return v.as_bytes().to_vec();
    }
    if let Some(v) = ctx.build_state.get(&fallback) {
        return v.as_bytes().to_vec();
    }
    warnings.push(format!(
        "ARTIFACT_MANIFEST_EMPTY_SOURCE: manifest artifact `{}` declared by `{}` has no \
         build_state entry under `{}` or `{}`; wrote an empty JSON object",
        artifact.name, source_plugin, scoped, fallback,
    ));
    b"{}".to_vec()
}

/// Emit an artifact declared with `emit = "<callback>"`.
///
/// `artifacts.md` §3 describes a WASM export the compiler calls for the
/// bytes; this compiler does not call it. Plugins instead stage the bytes in
/// `build_state` under `<source_plugin>.<artifact_name>` while their blocks
/// expand (frame.ui does this for `theme.css`), and the bytes are read from
/// there. The declared export name is not used.
///
/// On a miss an empty file is written and a warning is pushed.
/// (ARTIFACT_STATIC_ASSET_EMPTY_SOURCE)
fn emit_static_asset_from_build_state(
    artifact: &PluginArtifact,
    source_plugin: &str,
    ctx: &EmitContext,
    warnings: &mut Vec<String>,
) -> Vec<u8> {
    let key = format!("{}.{}", source_plugin, artifact.name);
    if let Some(v) = ctx.build_state.get(&key) {
        return v.as_bytes().to_vec();
    }
    warnings.push(format!(
        "ARTIFACT_STATIC_ASSET_EMPTY_SOURCE: artifact `{}` declared by `{}` has no \
         build_state entry under `{}`; wrote an empty file",
        artifact.name, source_plugin, key,
    ));
    Vec::new()
}

/// Emit a `static_path` artifact by copying a file shipped inside the
/// plugin's installed directory (resolved via `ctx.plugin_dirs`).
///
/// A plugin directory that is not registered, a `static_path` that would
/// leave the plugin directory (absolute, or with a `..` component), or a
/// file that cannot be read fails the artifact: the plugin install is
/// broken and writing an empty file would hide it.
/// (ARTIFACT_STATIC_PATH_MISSING_SOURCE)
fn emit_static_asset_from_path(
    artifact: &PluginArtifact,
    source_plugin: &str,
    static_path: &str,
    ctx: &EmitContext,
) -> Result<Vec<u8>, ArtifactError> {
    let fail = |reason: String| ArtifactError::EmissionFailed {
        artifact: artifact.name.clone(),
        reason: format!("ARTIFACT_STATIC_PATH_MISSING_SOURCE: {}", reason),
    };
    if !is_contained_relative_path(static_path) {
        return Err(fail(format!(
            "static_path `{}` declared by `{}` must be a relative path inside the plugin \
             directory",
            static_path, source_plugin
        )));
    }
    let Some(dir) = ctx.plugin_dirs.get(source_plugin) else {
        return Err(fail(format!(
            "the directory of plugin `{}` is unknown, so static_path `{}` cannot be read",
            source_plugin, static_path
        )));
    };
    let resolved = dir.join(static_path);
    std::fs::read(&resolved).map_err(|e| {
        fail(format!(
            "static_path source `{}` declared by `{}` could not be read: {}",
            resolved.display(),
            source_plugin,
            e
        ))
    })
}

/// Orchestrate the artifact emission pass. Iterates `[[artifacts]]` entries
/// across all loaded plugins in plugin-load order, evaluating each
/// `required_when` predicate and producing matching artifacts.
///
/// Returns the produced artifacts and a list of warnings (unsupported
/// sources, unknown predicates). Hard failures (cycle, emission error)
/// short-circuit and return as the error path.
pub fn orchestrate(
    loaded_manifests: &IndexMap<String, PluginManifest>,
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
            let Some(path_relative) = artifact_relative_path(&artifact.output_relative) else {
                return Err(ArtifactError::EmissionFailed {
                    artifact: artifact.name.clone(),
                    reason: format!(
                        "output_relative `{}` declared by `{}` must stay inside the output \
                         directory (artifacts.md §2)",
                        artifact.output_relative, plugin_name
                    ),
                });
            };
            match emit_artifact_bytes(artifact, plugin_name, ctx, &mut warnings) {
                Ok(bytes) => {
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
            aliases: Default::default(),
            blocks: Default::default(),
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
                // server_only_build still returns UnsupportedSource — Phase C
                // landed manifest + static_asset emitters but not this one.
                from_module: "server_only_build".to_string(),
            }),
            static_path: None,
            output_relative: "{output_dir}/frontend.wasm".to_string(),
            required_when: "has_client_init".to_string(),
            public: true,
            content_type: Some("application/wasm".to_string()),
        });

        let opter_in = manifest_with_lifecycle(
            "plugin.optin",
            PluginLifecycle {
                client_init: Some("emit_client_init".to_string()),
                ..Default::default()
            },
        );

        let mut manifests = IndexMap::new();
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
            plugin_dirs: Default::default(),
        };
        let (emitted, warnings) =
            orchestrate(&manifests, &ctx).expect("orchestrate should not hard-error");
        assert!(
            emitted.is_empty(),
            "server_only_build source still unsupported — no artifact bytes expected"
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
            content_type: Some("application/wasm".to_string()),
        });

        // No plugin declares client_init. Predicate must short-circuit.
        let mut manifests = IndexMap::new();
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
            plugin_dirs: Default::default(),
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
        let m = IndexMap::new();
        assert!(evaluate_required_when(
            "always",
            &m,
            &[],
            &Default::default()
        ));
    }

    #[test]
    fn test_evaluate_never_is_false() {
        let m = IndexMap::new();
        assert!(!evaluate_required_when(
            "never",
            &m,
            &[],
            &Default::default()
        ));
    }

    #[test]
    fn test_has_client_init_when_a_plugin_declares_it() {
        let mut manifests = IndexMap::new();
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
        let mut manifests = IndexMap::new();
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
        let mut manifests = IndexMap::new();
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
        let manifests = IndexMap::new();
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
        let manifests = IndexMap::new();
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
        let manifests = IndexMap::new();
        let state = std::collections::BTreeMap::new();
        assert!(
            !evaluate_required_when("has_build_state.any_key", &manifests, &[], &state),
            "predicate must return false against an empty build_state"
        );
    }

    #[test]
    fn test_unknown_predicate_defaults_to_false() {
        let manifests = IndexMap::new();
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
            content_type: Some("application/wasm".to_string()),
        };
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/whatever.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: true,
            build_state: Default::default(),
            plugin_dirs: Default::default(),
        };
        let err = emit_artifact_bytes(&artifact, "test.plugin", &ctx, &mut Vec::new())
            .expect_err("must reject");
        matches!(err, ArtifactError::BuildCycle { .. });
    }

    #[test]
    fn test_emit_unsupported_source_returns_clear_error() {
        // server_only_build is still unsupported in Phase C.
        let artifact = PluginArtifact {
            name: "backend.wasm".to_string(),
            purpose: "client_hydration".to_string(),
            emit: Some(ArtifactSource::Module {
                from_module: "server_only_build".to_string(),
            }),
            static_path: None,
            output_relative: "{output_dir}/backend.wasm".to_string(),
            required_when: "always".to_string(),
            public: false,
            content_type: None,
        };
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/whatever.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: false,
            build_state: Default::default(),
            plugin_dirs: Default::default(),
        };
        let err = emit_artifact_bytes(&artifact, "test.plugin", &ctx, &mut Vec::new())
            .expect_err("must error");
        let msg = format!("{}", err);
        assert!(msg.contains("PLUGIN-ARTIFACT-UNSUPPORTED"));
        assert!(msg.contains("backend.wasm"));
        assert!(msg.contains("server_only_build"));
    }

    /// Phase C: `purpose = "manifest"` reads `build_state` for the
    /// canonical scoped key and writes those bytes verbatim.
    #[test]
    fn test_emit_manifest_purpose_reads_build_state() {
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
            content_type: None,
        };
        let mut state = std::collections::BTreeMap::new();
        state.insert(
            "frame.ui.components.json".to_string(),
            r#"{"button":"button_render"}"#.to_string(),
        );
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/x.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: false,
            build_state: state,
            plugin_dirs: Default::default(),
        };
        let bytes = emit_artifact_bytes(&artifact, "frame.ui", &ctx, &mut Vec::new())
            .expect("manifest emits");
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s, r#"{"button":"button_render"}"#);
        // Round-trip as JSON to prove the emitter writes valid bytes.
        let parsed: serde_json::Value =
            serde_json::from_str(&s).expect("emitter output must be valid JSON");
        assert_eq!(parsed["button"], "button_render");
    }

    /// Phase C: when no build_state entry matches, the manifest emitter
    /// produces `{}` and a warning, NOT a hard error. Surfaces the
    /// ARTIFACT_MANIFEST_EMPTY_SOURCE diagnostic shape for log searches.
    #[test]
    fn test_emit_manifest_purpose_missing_source_writes_empty_object() {
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
            content_type: None,
        };
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/x.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: false,
            build_state: Default::default(),
            plugin_dirs: Default::default(),
        };
        let bytes = emit_artifact_bytes(&artifact, "frame.ui", &ctx, &mut Vec::new())
            .expect("not a hard error");
        assert_eq!(bytes, b"{}");
    }

    /// Phase C: `purpose = "static_asset"` with `emit = "<callback>"` reads
    /// the plugin-pushed value out of `build_state` under
    /// `<plugin>.<artifact_name>` and writes it as the file bytes.
    #[test]
    fn test_emit_static_asset_from_build_state() {
        let artifact = PluginArtifact {
            name: "theme.css".to_string(),
            purpose: "static_asset".to_string(),
            emit: Some(ArtifactSource::Callback("emit_theme_css".to_string())),
            static_path: None,
            output_relative: "{output_dir}/theme.css".to_string(),
            required_when: "always".to_string(),
            public: true,
            content_type: Some("text/css".to_string()),
        };
        let mut state = std::collections::BTreeMap::new();
        state.insert(
            "frame.ui.theme.css".to_string(),
            ":root { --color-primary: #4a90e2; }".to_string(),
        );
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/x.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: false,
            build_state: state,
            plugin_dirs: Default::default(),
        };
        let bytes = emit_artifact_bytes(&artifact, "frame.ui", &ctx, &mut Vec::new())
            .expect("static asset emits");
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            ":root { --color-primary: #4a90e2; }"
        );
    }

    /// Phase C: `purpose = "static_asset"` with `static_path = "<file>"`
    /// copies the file from the plugin's installed directory.
    #[test]
    fn test_emit_static_asset_from_static_path_copies_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("plugin");
        std::fs::create_dir_all(plugin_dir.join("assets")).unwrap();
        std::fs::write(plugin_dir.join("assets/theme.css"), b"body{margin:0}").unwrap();

        let artifact = PluginArtifact {
            name: "theme.css".to_string(),
            purpose: "static_asset".to_string(),
            emit: None,
            static_path: Some("assets/theme.css".to_string()),
            output_relative: "{output_dir}/theme.css".to_string(),
            required_when: "always".to_string(),
            public: true,
            content_type: Some("text/css".to_string()),
        };
        let mut plugin_dirs = std::collections::HashMap::new();
        plugin_dirs.insert("frame.ui".to_string(), plugin_dir);

        let ctx = EmitContext {
            entry_path: Path::new("/tmp/x.cln"),
            output_dir: Path::new("/tmp/dist"),
            opt_level: 2,
            in_nested_build: false,
            build_state: Default::default(),
            plugin_dirs,
        };
        let bytes = emit_artifact_bytes(&artifact, "frame.ui", &ctx, &mut Vec::new())
            .expect("file copy emits");
        assert_eq!(bytes, b"body{margin:0}");
    }

    /// Phase C: orchestrate-end-to-end. A plugin declaring BOTH a manifest
    /// and a static_asset artifact reaches the EmittedArtifact list with the
    /// correct `path_relative` (relative to output_dir, per artifacts.md §5).
    /// This is the assertion the previously-ignored
    /// `declared_artifacts_appear_in_build_manifest_json` test was waiting
    /// on — verified at the orchestrate boundary because the main.rs
    /// build-manifest serialization is a thin pass-through from the
    /// EmittedArtifact list.
    #[test]
    fn test_orchestrate_emits_manifest_and_static_asset_with_relative_paths() {
        let mut declarer = manifest_with_lifecycle("test.artifacts_v2", PluginLifecycle::default());
        declarer.artifacts.push(PluginArtifact {
            name: "components.json".to_string(),
            purpose: "manifest".to_string(),
            emit: Some(ArtifactSource::Module {
                from_module: "manifest".to_string(),
            }),
            static_path: None,
            output_relative: "{output_dir}/components.json".to_string(),
            required_when: "always".to_string(),
            public: false,
            content_type: None,
        });
        declarer.artifacts.push(PluginArtifact {
            name: "theme.css".to_string(),
            purpose: "static_asset".to_string(),
            emit: Some(ArtifactSource::Callback("emit_theme_css".to_string())),
            static_path: None,
            output_relative: "{output_dir}/theme.css".to_string(),
            required_when: "always".to_string(),
            public: true,
            content_type: Some("text/css".to_string()),
        });

        let mut manifests = IndexMap::new();
        manifests.insert("test.artifacts_v2".to_string(), declarer);

        let mut state = std::collections::BTreeMap::new();
        state.insert(
            "test.artifacts_v2.components.json".to_string(),
            r#"{"x":1}"#.to_string(),
        );
        state.insert(
            "test.artifacts_v2.theme.css".to_string(),
            "body{}".to_string(),
        );

        let output_dir = PathBuf::from("/tmp/dist-test-orchestrate");
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/x.cln"),
            output_dir: &output_dir,
            opt_level: 2,
            in_nested_build: false,
            build_state: state,
            plugin_dirs: Default::default(),
        };

        let (emitted, warnings) = orchestrate(&manifests, &ctx).expect("orchestrate ok");
        assert!(warnings.is_empty(), "no warnings expected: {:?}", warnings);
        assert_eq!(emitted.len(), 2, "both artifacts must be emitted");

        let m = emitted
            .iter()
            .find(|a| a.name == "components.json")
            .unwrap();
        assert_eq!(m.purpose, "manifest");
        // path_relative must be relative to output_dir (artifacts.md §5),
        // i.e. just the file name — not the absolute resolved path.
        assert_eq!(m.path_relative, "components.json");
        assert_eq!(m.bytes, b"{\"x\":1}");

        let s = emitted.iter().find(|a| a.name == "theme.css").unwrap();
        assert_eq!(s.purpose, "static_asset");
        assert_eq!(s.path_relative, "theme.css");
        assert!(s.public);
        assert_eq!(s.content_type, "text/css");
        assert_eq!(s.bytes, b"body{}");
    }

    /// `required_when = "has_artifact.X"` across plugins depends on which
    /// plugin's artifacts were produced first. Orchestration must walk the
    /// manifests in plugin-load order so the result is the same on every
    /// build — it used to iterate a `HashMap`, so the dependent artifact
    /// appeared or vanished depending on hash order.
    #[test]
    fn test_orchestrate_cross_plugin_has_artifact_follows_load_order() {
        fn css_artifact(name: &str, required_when: &str) -> PluginArtifact {
            PluginArtifact {
                name: name.to_string(),
                purpose: "static_asset".to_string(),
                emit: Some(ArtifactSource::Callback(format!("emit_{}", name))),
                static_path: None,
                output_relative: format!("{{output_dir}}/{}", name),
                required_when: required_when.to_string(),
                public: true,
                content_type: Some("text/css".to_string()),
            }
        }
        let mut base = manifest_with_lifecycle("test.base", PluginLifecycle::default());
        base.artifacts.push(css_artifact("base.css", "always"));
        let mut theme = manifest_with_lifecycle("test.theme", PluginLifecycle::default());
        theme
            .artifacts
            .push(css_artifact("theme.css", "has_artifact.base.css"));

        let mut state = std::collections::BTreeMap::new();
        state.insert("test.base.base.css".to_string(), "a{}".to_string());
        state.insert("test.theme.theme.css".to_string(), "b{}".to_string());
        let output_dir = PathBuf::from("/tmp/dist-test-orchestrate-order");
        let ctx = EmitContext {
            entry_path: Path::new("/tmp/x.cln"),
            output_dir: &output_dir,
            opt_level: 2,
            in_nested_build: false,
            build_state: state,
            plugin_dirs: Default::default(),
        };

        // Many plugins loaded before the pair so a hash-ordered walk would
        // reorder them on some runs.
        for _ in 0..32 {
            let mut manifests = IndexMap::new();
            for i in 0..8 {
                let name = format!("test.filler{}", i);
                manifests.insert(
                    name.clone(),
                    manifest_with_lifecycle(&name, PluginLifecycle::default()),
                );
            }
            manifests.insert("test.base".to_string(), base.clone());
            manifests.insert("test.theme".to_string(), theme.clone());
            let (emitted, _) = orchestrate(&manifests, &ctx).expect("orchestrate ok");
            let names: Vec<&str> = emitted.iter().map(|a| a.name.as_str()).collect();
            assert_eq!(names, vec!["base.css", "theme.css"]);
        }

        // Loaded the other way round, the dependency is not yet satisfied
        // when `test.theme` is visited — consistently.
        let mut reversed = IndexMap::new();
        reversed.insert("test.theme".to_string(), theme);
        reversed.insert("test.base".to_string(), base);
        let (emitted, _) = orchestrate(&reversed, &ctx).expect("orchestrate ok");
        let names: Vec<&str> = emitted.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["base.css"]);
    }

    fn static_artifact(name: &str, output_relative: &str, static_path: &str) -> PluginArtifact {
        PluginArtifact {
            name: name.to_string(),
            purpose: "static_asset".to_string(),
            emit: None,
            static_path: Some(static_path.to_string()),
            output_relative: output_relative.to_string(),
            required_when: "always".to_string(),
            public: true,
            content_type: None,
        }
    }

    fn ctx_with_plugin_dir<'a>(output_dir: &'a Path, plugin_dir: &Path) -> EmitContext<'a> {
        let mut plugin_dirs = std::collections::HashMap::new();
        plugin_dirs.insert("p".to_string(), plugin_dir.to_path_buf());
        EmitContext {
            entry_path: Path::new("/tmp/x.cln"),
            output_dir,
            opt_level: 2,
            in_nested_build: false,
            build_state: Default::default(),
            plugin_dirs,
        }
    }

    /// `output_relative` is relative to the output directory with or
    /// without the `{output_dir}` placeholder, and keeps its subfolders.
    #[test]
    fn output_relative_keeps_subfolders_without_placeholder() {
        assert_eq!(
            artifact_relative_path("assets/theme.css").as_deref(),
            Some("assets/theme.css")
        );
        assert_eq!(
            artifact_relative_path("{output_dir}/assets/theme.css").as_deref(),
            Some("assets/theme.css")
        );
        assert_eq!(
            resolve_output_relative("assets/theme.css", Path::new("dist")),
            PathBuf::from("dist/assets/theme.css")
        );
    }

    /// An `output_relative` that leaves the output directory is refused.
    #[test]
    fn output_relative_outside_output_dir_is_refused() {
        for bad in [
            "../escape.css",
            "{output_dir}/../escape.css",
            "/etc/escape.css",
            "a/{output_dir}/x.css",
            "{output_dir}",
            "",
        ] {
            assert_eq!(artifact_relative_path(bad), None, "{:?}", bad);
        }

        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.css"), b"x").unwrap();
        let mut manifest = manifest_with_lifecycle("p", PluginLifecycle::default());
        manifest
            .artifacts
            .push(static_artifact("a.css", "{output_dir}/../a.css", "a.css"));
        let mut manifests = IndexMap::new();
        manifests.insert("p".to_string(), manifest);
        let ctx = ctx_with_plugin_dir(Path::new("/tmp/dist"), tmp.path());
        let err = orchestrate(&manifests, &ctx).err().expect("must refuse");
        assert!(err.to_string().contains("output directory"), "{}", err);
    }

    /// A `static_path` file that is missing fails the build instead of
    /// writing an empty file.
    #[test]
    fn static_path_missing_source_is_an_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = ctx_with_plugin_dir(Path::new("/tmp/dist"), tmp.path());
        let artifact = static_artifact("theme.css", "{output_dir}/theme.css", "missing.css");
        let err = emit_artifact_bytes(&artifact, "p", &ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(err, ArtifactError::EmissionFailed { .. }),
            "{}",
            err
        );
        assert!(err.to_string().contains("missing.css"), "{}", err);

        // Unknown plugin directory.
        let err = emit_artifact_bytes(&artifact, "other", &ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(err, ArtifactError::EmissionFailed { .. }),
            "{}",
            err
        );
    }

    /// A `static_path` may not read outside the plugin's own directory.
    #[test]
    fn static_path_traversal_is_refused() {
        let outer = tempfile::TempDir::new().unwrap();
        std::fs::write(outer.path().join("secret.txt"), b"secret").unwrap();
        let plugin_dir = outer.path().join("plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let ctx = ctx_with_plugin_dir(Path::new("/tmp/dist"), &plugin_dir);
        let absolute = outer.path().join("secret.txt");
        for bad in ["../secret.txt", absolute.to_str().unwrap()] {
            let artifact = static_artifact("x.txt", "{output_dir}/x.txt", bad);
            let err = emit_artifact_bytes(&artifact, "p", &ctx, &mut Vec::new()).unwrap_err();
            assert!(err.to_string().contains("relative path"), "{}", err);
        }
    }

    /// A build_state-backed artifact with nothing staged is still written,
    /// and the miss reaches the warnings the CLI prints.
    #[test]
    fn missing_build_state_source_is_reported_as_a_warning() {
        let mut manifest = manifest_with_lifecycle("p", PluginLifecycle::default());
        manifest.artifacts.push(PluginArtifact {
            name: "theme.css".to_string(),
            purpose: "static_asset".to_string(),
            emit: Some(ArtifactSource::Callback("emit_theme".to_string())),
            static_path: None,
            output_relative: "{output_dir}/theme.css".to_string(),
            required_when: "always".to_string(),
            public: true,
            content_type: None,
        });
        let mut manifests = IndexMap::new();
        manifests.insert("p".to_string(), manifest);
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = ctx_with_plugin_dir(Path::new("/tmp/dist"), tmp.path());
        let (emitted, warnings) = orchestrate(&manifests, &ctx).unwrap();
        assert_eq!(emitted.len(), 1);
        assert!(emitted[0].bytes.is_empty());
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("ARTIFACT_STATIC_ASSET_EMPTY_SOURCE")
                    && w.contains("p.theme.css")),
            "{:?}",
            warnings
        );
    }
}
