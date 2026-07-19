/*!
 * Plugin Registry - Manages framework plugin registration and dispatch
 */

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use super::{
    FrameworkBlock, FrameworkPlugin, PluginCompletionItem, PluginDiagnostic, PluginHoverInfo,
    PluginLspContext,
};
use crate::ast::{SourceLocation, Statement};
use crate::plugins::plugin_abi::{
    BridgeFunction, BuildRegistration, CliRegistration, DataRegistration, PluginManifest,
    ServerRegistration,
};

/// Error type for plugin operations
#[derive(Debug, Clone)]
pub enum PluginError {
    /// No plugin registered for the given block type
    UnknownBlockType {
        block_name: String,
        location: Option<SourceLocation>,
    },
    /// Plugin expansion failed
    ExpansionFailed {
        plugin_name: String,
        block_name: String,
        message: String,
        location: Option<SourceLocation>,
    },
    /// Plugin validation failed
    ValidationFailed {
        plugin_name: String,
        message: String,
        location: Option<SourceLocation>,
    },
    /// Plugin registration conflict
    RegistrationConflict {
        block_name: String,
        existing_plugin: String,
        new_plugin: String,
    },
    /// Parse error in DSL content
    ParseError {
        message: String,
        line: usize,
        column: usize,
        location: Option<SourceLocation>,
    },
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::UnknownBlockType {
                block_name,
                location,
            } => {
                write!(f, "No plugin registered for block type '{}:'", block_name)?;
                if let Some(loc) = location {
                    write!(f, " at {}", loc)?;
                }
                Ok(())
            }
            PluginError::ExpansionFailed {
                plugin_name,
                block_name,
                message,
                location,
            } => {
                write!(
                    f,
                    "Plugin '{}' failed to expand '{}:': {}",
                    plugin_name, block_name, message
                )?;
                if let Some(loc) = location {
                    write!(f, " at {}", loc)?;
                }
                Ok(())
            }
            PluginError::ValidationFailed {
                plugin_name,
                message,
                location,
            } => {
                write!(f, "Plugin '{}' validation failed: {}", plugin_name, message)?;
                if let Some(loc) = location {
                    write!(f, " at {}", loc)?;
                }
                Ok(())
            }
            PluginError::RegistrationConflict {
                block_name,
                existing_plugin,
                new_plugin,
            } => {
                write!(
                    f,
                    "Registration conflict for '{}:': already handled by '{}', cannot register '{}'",
                    block_name, existing_plugin, new_plugin
                )
            }
            PluginError::ParseError {
                message,
                line,
                column,
                location,
            } => {
                write!(
                    f,
                    "Parse error in DSL content at {}:{}: {}",
                    line, column, message
                )?;
                if let Some(loc) = location {
                    write!(f, " (block at {})", loc)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for PluginError {}

/// Aggregated lifecycle registrations collected from all loaded plugins.
///
/// Each entry is a `(plugin_name, registration)` pair so that callers can
/// attribute registrations back to the originating plugin for diagnostics
/// and ordering.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PluginRegistrations {
    /// Server lifecycle registrations — middleware, startup, and shutdown hooks.
    pub server: Vec<(String, ServerRegistration)>,
    /// CLI lifecycle registrations — custom command-line commands.
    pub cli: Vec<(String, CliRegistration)>,
    /// Data lifecycle registrations — custom types, validators, and query extensions.
    pub data: Vec<(String, DataRegistration)>,
    /// Build lifecycle registrations — pre/post-build hooks and asset processors.
    pub build: Vec<(String, BuildRegistration)>,
}

/// Registry for framework plugins
///
/// Maintains a mapping from block identifiers to plugin handlers.
/// When a FrameworkBlock is encountered, the registry dispatches
/// to the appropriate plugin for expansion.
///
/// **Immutable after creation** - plugins cannot be added/removed after building.
/// This prevents plugin injection mid-compilation.
///
/// # Example
///
/// ```ignore
/// use my_framework_plugins::MyPlugin;
///
/// let registry = PluginRegistry::builder()
///     .add(MyPlugin::new())
///     .build()
///     .expect("Failed to build registry");
/// ```
pub struct PluginRegistry {
    /// Map from block identifier (e.g., "endpoints") to plugin
    handlers: HashMap<String, Arc<dyn FrameworkPlugin>>,
    /// Map from ORM expression pattern (e.g., "*.find:") to the plugin that handles it.
    ///
    /// Patterns come from `[handles] expressions` in plugin.toml.  A leading `*`
    /// acts as a wildcard matching any identifier before the `.`.
    expression_handlers: HashMap<String, Arc<dyn FrameworkPlugin>>,
    /// Track plugin names for debugging
    registered_plugins: Vec<String>,
    /// Bridge functions from all loaded plugins
    /// These are functions that plugins expect the runtime to provide (e.g., _db_query)
    bridge_functions: Vec<BridgeFunction>,
    /// Full plugin manifests for enforcement rules and path detection
    manifests: HashMap<String, PluginManifest>,
    /// Lifecycle registrations collected from all loaded plugins
    registrations: PluginRegistrations,
    /// Plugin name → set of bridge function names that plugin is allowed to call.
    ///
    /// Populated from each plugin's `[bridge] functions` declarations.  A plugin
    /// that declares no bridge functions will have an empty set here, meaning it
    /// may not call any bridge function.  Plugins that were registered via the
    /// deprecated mutable API (which lacks manifest information) are absent from
    /// this map and are therefore exempt from permission checking.
    plugin_permissions: HashMap<String, HashSet<String>>,
    /// Plugin Contracts v2 — shared per-build keystore. Bridge stubs in the
    /// loaded plugins' WASM sandboxes mutate this store; the build orchestrator
    /// snapshots it into `dist/build-manifest.json` at end of build.
    /// See `contracts/lifecycle.md` §2.5.
    build_state: super::BuildState,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("Default registry should build")
    }
}

/// Convert a snake_case string to camelCase.
/// e.g. "get_session" → "getSession", "has_any_role" → "hasAnyRole"
fn snake_to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

impl PluginRegistry {
    /// Create a new empty plugin registry
    #[deprecated(note = "Use PluginRegistry::builder() instead")]
    pub fn new() -> Self {
        #[allow(deprecated)]
        Self {
            handlers: HashMap::new(),
            expression_handlers: HashMap::new(),
            registered_plugins: Vec::new(),
            bridge_functions: Vec::new(),
            manifests: HashMap::new(),
            registrations: PluginRegistrations::default(),
            plugin_permissions: HashMap::new(),
            build_state: super::new_build_state(),
        }
    }

    /// Create a builder for constructing an immutable plugin registry
    ///
    /// # Example
    ///
    /// ```ignore
    /// use my_framework_plugins::MyPlugin;
    ///
    /// let registry = PluginRegistry::builder()
    ///     .add(MyPlugin::new())
    ///     .build()?;
    /// ```
    pub fn builder() -> PluginRegistryBuilder {
        PluginRegistryBuilder::new()
    }

    /// Register a plugin with the registry
    ///
    /// **Deprecated**: Use the builder pattern instead.
    ///
    /// # Arguments
    /// * `plugin` - The plugin to register
    ///
    /// # Returns
    /// * `Ok(())` if registration succeeds
    /// * `Err(PluginError::RegistrationConflict)` if a handler already exists
    #[deprecated(
        since = "0.13.1",
        note = "Use PluginRegistry::builder().add(plugin).build()"
    )]
    pub fn register(&mut self, plugin: Arc<dyn FrameworkPlugin>) -> Result<(), PluginError> {
        let plugin_name = plugin.name().to_string();

        for block_name in plugin.handles() {
            if let Some(existing) = self.handlers.get(*block_name) {
                return Err(PluginError::RegistrationConflict {
                    block_name: block_name.to_string(),
                    existing_plugin: existing.name().to_string(),
                    new_plugin: plugin_name,
                });
            }
        }

        // Register all block handlers
        for block_name in plugin.handles() {
            self.handlers
                .insert(block_name.to_string(), Arc::clone(&plugin));
        }

        // Register ORM expression patterns
        for pattern in plugin.expression_patterns() {
            self.expression_handlers
                .insert(pattern.clone(), Arc::clone(&plugin));
        }

        self.registered_plugins.push(plugin_name);
        Ok(())
    }

    /// Check if a block type is handled by a registered plugin.
    ///
    /// Extracts the block type (first word) from names like `"screen RegistrationForm"`
    /// before checking the handler registry.  Also checks ORM expression patterns so
    /// that `FrameworkBlock` nodes produced by the ORM path are correctly dispatched.
    pub fn handles(&self, block_name: &str) -> bool {
        // Extract block type (first word) from names like "screen RegistrationForm"
        let block_type = block_name.split_whitespace().next().unwrap_or(block_name);
        self.handlers.contains_key(block_type) || self.handles_as_expression(block_name)
    }

    /// Check whether a dotted name like `"User.find"` is handled by any plugin's
    /// ORM expression pattern (e.g., `"*.find:"`).
    ///
    /// Pattern matching rules:
    /// - Exact match: `"User.find:"` matches pattern `"User.find:"`
    /// - Glob match: pattern starting with `"*"` matches any identifier before the `.`
    ///   e.g., pattern `"*.find:"` matches `"User.find"`, `"Order.find"`, etc.
    ///
    /// Both the name and pattern may optionally include a trailing `:`.
    pub fn handles_as_expression(&self, name: &str) -> bool {
        let name_clean = name.trim_end_matches(':');

        // Exact match (strip trailing colon from both sides)
        if self
            .expression_handlers
            .keys()
            .any(|p| p.trim_end_matches(':') == name_clean)
        {
            return true;
        }

        // Glob match: pattern starting with '*'
        for pattern in self.expression_handlers.keys() {
            let pat = pattern.trim_end_matches(':');
            if let Some(suffix) = pat.strip_prefix('*') {
                if name_clean.ends_with(suffix) {
                    return true;
                }
            }
        }

        false
    }

    /// Return the plugin that handles an ORM expression name (e.g., `"User.find"`),
    /// or `None` if no plugin claims it.
    ///
    /// Uses the same pattern-matching rules as [`handles_as_expression`].
    pub fn get_expression_handler(&self, name: &str) -> Option<&Arc<dyn FrameworkPlugin>> {
        let name_clean = name.trim_end_matches(':');

        // Exact match first
        for (pattern, handler) in &self.expression_handlers {
            if pattern.trim_end_matches(':') == name_clean {
                return Some(handler);
            }
        }

        // Glob match
        for (pattern, handler) in &self.expression_handlers {
            let pat = pattern.trim_end_matches(':');
            if let Some(suffix) = pat.strip_prefix('*') {
                if name_clean.ends_with(suffix) {
                    return Some(handler);
                }
            }
        }

        None
    }

    /// Get the plugin that handles a specific block type
    ///
    /// Extracts the block type (first word) from names like "screen RegistrationForm"
    /// before looking up the handler.
    pub fn get_handler(&self, block_name: &str) -> Option<&Arc<dyn FrameworkPlugin>> {
        let block_type = block_name.split_whitespace().next().unwrap_or(block_name);
        self.handlers.get(block_type)
    }

    /// Expand a framework block using the appropriate plugin
    ///
    /// # Arguments
    /// * `block` - The framework block to expand
    ///
    /// # Returns
    /// * `Ok(Vec<Statement>)` - The expanded statements
    /// * `Err(PluginError)` - If no handler exists or expansion fails
    pub fn expand(&self, block: &FrameworkBlock) -> Result<Vec<Statement>, PluginError> {
        // Extract block type (first word) from names like "screen RegistrationForm"
        let block_type = block.name.split_whitespace().next().unwrap_or(&block.name);
        // Look up by block handler first, then fall back to expression handlers (ORM queries).
        let handler = self
            .handlers
            .get(block_type)
            .or_else(|| self.get_expression_handler(&block.name))
            .ok_or_else(|| PluginError::UnknownBlockType {
                block_name: block.name.clone(),
                location: block.location.clone(),
            })?;

        // Validate first
        handler
            .validate(block)
            .map_err(|e| PluginError::ValidationFailed {
                plugin_name: handler.name().to_string(),
                message: e.to_string(),
                location: block.location.clone(),
            })?;

        // Then expand
        handler
            .expand(block)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: handler.name().to_string(),
                block_name: block.name.clone(),
                message: e.to_string(),
                location: block.location.clone(),
            })
    }

    /// Expand a framework block using the appropriate plugin (full version)
    ///
    /// Returns a PluginExpansion that can include start functions and other definitions.
    pub fn expand_full(
        &self,
        block: &FrameworkBlock,
    ) -> Result<super::PluginExpansion, PluginError> {
        // Extract block type (first word) from names like "screen RegistrationForm"
        let block_type = block.name.split_whitespace().next().unwrap_or(&block.name);
        let handler = self
            .handlers
            .get(block_type)
            .or_else(|| self.get_expression_handler(&block.name))
            .ok_or_else(|| PluginError::UnknownBlockType {
                block_name: block.name.clone(),
                location: block.location.clone(),
            })?;

        // Validate first
        handler
            .validate(block)
            .map_err(|e| PluginError::ValidationFailed {
                plugin_name: handler.name().to_string(),
                message: e.to_string(),
                location: block.location.clone(),
            })?;

        // Then expand with full result
        handler
            .expand_full(block)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: handler.name().to_string(),
                block_name: block.name.clone(),
                message: e.to_string(),
                location: block.location.clone(),
            })
    }

    /// Get list of registered plugin names
    pub fn registered_plugins(&self) -> &[String] {
        &self.registered_plugins
    }

    /// Iterate every plugin in the registry, once per plugin, in an
    /// implementation-defined order.
    ///
    /// The internal `handlers` map keys plugins by block name so a plugin
    /// registered for multiple blocks appears multiple times. This helper
    /// dedupes by plugin name so callers walking side-effect-carrying trait
    /// methods (lint, lifecycle slots, etc.) don't invoke a plugin twice
    /// per pass. See `plugins::lint::run_lint_pass` for the primary caller.
    pub fn iter_unique_plugins(
        &self,
    ) -> impl Iterator<Item = &std::sync::Arc<dyn FrameworkPlugin>> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for plugin in self.handlers.values() {
            if seen.insert(plugin.name()) {
                out.push(plugin);
            }
        }
        out.into_iter()
    }

    /// Get list of handled block types
    pub fn handled_block_types(&self) -> Vec<&str> {
        self.handlers.keys().map(|s| s.as_str()).collect()
    }

    /// Check if the registry has any plugins registered
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Get the number of registered handlers
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }

    /// Get all bridge functions from loaded plugins
    ///
    /// Bridge functions are runtime-provided functions declared in plugin.toml [bridge] sections.
    /// These functions (e.g., _db_query, _db_execute) need to be registered with the compiler
    /// so that code calling them can be type-checked and compiled correctly.
    ///
    /// # Example
    /// ```ignore
    /// let registry = loader.load_plugins(&["frame.data"])?;
    /// for func in registry.bridge_functions() {
    ///     // Register _db_query, _db_execute, etc.
    ///     builtin_registry.register_bridge_function(func);
    /// }
    /// ```
    pub fn bridge_functions(&self) -> &[BridgeFunction] {
        &self.bridge_functions
    }

    /// Check if a function name is a bridge function
    pub fn is_bridge_function(&self, name: &str) -> bool {
        self.bridge_functions.iter().any(|f| f.name == name)
    }

    /// Get a bridge function by name
    pub fn get_bridge_function(&self, name: &str) -> Option<&BridgeFunction> {
        self.bridge_functions.iter().find(|f| f.name == name)
    }

    /// Enumerate every loaded bridge function that registers a request-handler
    /// at call time — one whose plugin.toml entry sets
    /// `registers_handler_at_arg = <n>`.
    ///
    /// Returns pairs `(bridge_name, handler_arg_index)` used by
    /// `hir::validation::ValidationContext::collect_plugin_registered_handlers`
    /// to recognize plugin-emitted route handlers as legitimate
    /// request-context sites (CONC002). Empty when no loaded plugin declares
    /// any such bridge — in that case the HIR validator falls back to its
    /// historical hardcoded recognition of frame.server's `_http_route` /
    /// `_http_route_protected` so existing installs keep working while
    /// framework maintainers add the metadata to plugin.toml.
    ///
    /// Fixes ARCH-CONC002-HARDCODES-HTTP-ROUTE (dashboard fp 12ce9f522815).
    pub fn handler_registering_bridges(&self) -> Vec<(String, usize)> {
        self.bridge_functions
            .iter()
            .filter_map(|bf| {
                bf.registers_handler_at_arg
                    .map(|idx| (bf.name.clone(), idx))
            })
            .collect()
    }

    /// Run all registered assemble hooks, merging their outputs.
    ///
    /// Called once per compilation after source discovery but before parsing.
    /// Results from all plugins are combined: injected sources are appended,
    /// transformed sources are merged (last write wins for duplicate paths).
    /// Run every loaded plugin's `assemble` hook and merge the outputs.
    ///
    /// Returns `(combined_output, errors)`:
    /// - `combined_output` aggregates successful plugins' contributions so a
    ///   single broken plugin does not erase work from the rest.
    /// - `errors` carries every per-plugin `Err` so the caller can decide
    ///   whether to fail the build (default) or downgrade to a diagnostic.
    ///
    /// Prior behaviour silently discarded `Err` results, producing builds
    /// that "succeeded" with empty injected_sources / transformed_sources —
    /// the failure mode tracked as COMPILER-ASSEMBLE-ERROR-SWALLOWED. Every
    /// caller MUST inspect the returned error vec.
    pub fn run_assemble_hooks(
        &self,
        input: &crate::plugins::plugin_abi::AssembleInput,
    ) -> (
        crate::plugins::plugin_abi::AssembleOutput,
        Vec<(String, PluginError)>,
    ) {
        use crate::plugins::plugin_abi::AssembleOutput;
        use std::collections::HashSet;

        let mut combined = AssembleOutput::default();
        let mut errors: Vec<(String, PluginError)> = Vec::new();
        // Deduplicate by raw pointer so each plugin is called exactly once
        // even when registered under multiple block names.
        let mut seen: HashSet<*const dyn FrameworkPlugin> = HashSet::new();
        for plugin in self.handlers.values() {
            let ptr = Arc::as_ptr(plugin);
            if !seen.insert(ptr) {
                continue;
            }
            let plugin_name = plugin.name().to_string();
            match plugin.assemble(input) {
                Ok(output) => {
                    tracing::debug!(
                        plugin = %plugin_name,
                        injected = output.injected_sources.len(),
                        transformed = output.transformed_sources.len(),
                        "plugin assemble produced",
                    );
                    combined.injected_sources.extend(output.injected_sources);
                    combined
                        .transformed_sources
                        .extend(output.transformed_sources);
                }
                Err(e) => {
                    tracing::error!(
                        plugin = %plugin_name,
                        error = %e,
                        "plugin assemble failed; the build will surface this as a compile error",
                    );
                    errors.push((plugin_name, e));
                }
            }
        }
        (combined, errors)
    }

    /// Get all loaded plugin manifests
    ///
    /// Returns the full manifests for enforcement rules, path detection, etc.
    /// Plugin Contracts v2 — replace the registry's per-build state container.
    /// Called by `WasmPluginLoader::load_plugins_with_build_state` after
    /// every adapter has been wired so the registry can hand the same `Arc`
    /// to the build orchestrator at end of build.
    /// See `contracts/lifecycle.md` §2.5.
    pub fn set_build_state(&mut self, state: super::BuildState) {
        self.build_state = state;
    }

    /// Plugin Contracts v2 — borrow the shared per-build state for snapshotting
    /// into the build context or `build-manifest.json`.
    pub fn build_state(&self) -> &super::BuildState {
        &self.build_state
    }

    /// Plugin Contracts v2 — take a snapshot of the build state as a plain
    /// BTreeMap for serialization into `build-manifest.json`.
    pub fn build_state_snapshot(&self) -> std::collections::BTreeMap<String, String> {
        self.build_state
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Plugin Contracts v2 — true when any loaded plugin's manifest declares
    /// the given lifecycle slot. Used by the build orchestration to decide
    /// whether a slot's downstream effects should fire (e.g. preserving the
    /// expander's client_init contribution instead of clearing client _start).
    /// See contracts/lifecycle.md §3.
    pub fn any_plugin_declares_lifecycle_slot(&self, slot_name: &str) -> bool {
        self.manifests.values().any(|m| match slot_name {
            "module_helpers" => m.lifecycle.module_helpers.is_some(),
            "program_init" => m.lifecycle.program_init.is_some(),
            "client_init" => m.lifecycle.client_init.is_some(),
            "server_init" => m.lifecycle.server_init.is_some(),
            "per_request" => m.lifecycle.per_request.is_some(),
            "artifact_emitters" => m.lifecycle.artifact_emitters.is_some(),
            _ => false,
        })
    }

    /// Plugin Contracts v2 — invoke a lifecycle slot across every registered
    /// plugin in load order and return the contributed code per plugin.
    /// See foundation/spec/plugins/contracts/lifecycle.md §2, §3.
    ///
    /// Each plugin sees a fresh snapshot of the shared per-build state taken
    /// just before dispatch, so reads of `frame.ui:components` etc. reflect
    /// writes performed earlier in this build (the per-file `expand_block`
    /// pass that populates the keystore via `_build_state_set`). The
    /// caller's `context` is cloned and the snapshot is stamped into the
    /// clone — the caller's context is never mutated.
    ///
    /// Plugins that have not declared the slot in their manifest contribute
    /// empty expansions. Failed slot invocations are logged and skipped — the
    /// build continues with whatever the other plugins contributed.
    pub fn invoke_lifecycle_slot(
        &self,
        slot_name: &str,
        context: &super::BuildContext,
    ) -> Vec<super::PluginExpansion> {
        // Take a fresh snapshot of the build state into a per-dispatch
        // context clone. Without this, `context.build_state` stays as
        // whatever the caller passed in (typically an empty map from
        // `BuildContext::new()`), and slots like `emit_ui_client_init`
        // that read `build_state.frame.ui:components` see nothing —
        // even though `_build_state_set` writes performed during the
        // earlier per-file expand pass have populated the shared Arc.
        // That gap is the root cause of HYDRATE_AUTO.
        let mut snapshotted = context.clone();
        snapshotted.snapshot_build_state(&self.build_state);

        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();
        for plugin in self.handlers.values() {
            let plugin_name = plugin.name();
            if !seen.insert(plugin_name.to_string()) {
                continue;
            }
            // Skip plugins that did not declare the slot. Avoids spinning up
            // a wasmtime instance for every (plugin, slot) pair.
            let declared = self
                .manifests
                .get(plugin_name)
                .map(|m| match slot_name {
                    "module_helpers" => m.lifecycle.module_helpers.is_some(),
                    "program_init" => m.lifecycle.program_init.is_some(),
                    "client_init" => m.lifecycle.client_init.is_some(),
                    "server_init" => m.lifecycle.server_init.is_some(),
                    "per_request" => m.lifecycle.per_request.is_some(),
                    "artifact_emitters" => m.lifecycle.artifact_emitters.is_some(),
                    _ => false,
                })
                .unwrap_or(false);
            if !declared {
                continue;
            }
            match plugin.invoke_lifecycle_slot(slot_name, &snapshotted) {
                Ok(expansion) => results.push(expansion),
                Err(e) => {
                    tracing::warn!(
                        target: "plugin_contracts_v2",
                        plugin = %plugin_name,
                        slot = %slot_name,
                        error = %e,
                        "lifecycle slot invocation failed; continuing with empty contribution"
                    );
                }
            }
        }
        results
    }

    /// Plugin Contracts v2 — return resolved callback contracts from every
    /// loaded plugin's `[bridge.functions.callback]` blocks. Used to populate
    /// the `callbacks` field in `build-manifest.json` so hosts can read the
    /// dispatch contract without re-parsing every plugin.toml.
    /// See foundation/spec/plugins/contracts/bridge-host-classes.md §4.
    pub fn callback_contracts(&self) -> Vec<crate::build_manifest::CallbackContract> {
        let mut out = Vec::new();
        for (declaring_plugin, manifest) in &self.manifests {
            for bridge in &manifest.bridge.functions {
                let Some(cb) = &bridge.callback else {
                    continue;
                };
                out.push(crate::build_manifest::CallbackContract {
                    bridge: bridge.name.clone(),
                    purpose: cb.purpose.clone(),
                    plugin_target: cb.plugin_target.clone(),
                    discovery: cb.discovery.clone(),
                    export_pattern: cb.export_pattern.clone(),
                    fallback: cb.fallback.clone(),
                    declared_by_plugin: declaring_plugin.clone(),
                });
            }
        }
        out
    }

    pub fn loaded_manifests(&self) -> &HashMap<String, PluginManifest> {
        &self.manifests
    }

    /// Find a registered plugin that declares `[exports].process_html` in its
    /// manifest. Returned plugin can be asked to convert a raw HTML page into
    /// Clean Language source via `FrameworkPlugin::process_html`.
    ///
    /// Returns `None` if no loaded plugin processes HTML — the compiler then
    /// reports a configuration error rather than silently feeding raw HTML
    /// into the Clean parser (which produced `SYN001` historically).
    pub fn find_html_processor(&self) -> Option<&Arc<dyn FrameworkPlugin>> {
        // Plugins are registered once per block they handle, so the same
        // Arc may appear under multiple keys in `handlers`. Pick the first
        // plugin whose manifest declares the export, in deterministic order
        // (Vec preserves registration order).
        for plugin_name in &self.registered_plugins {
            let manifest = self.manifests.get(plugin_name)?;
            if manifest.exports.process_html.is_some() {
                // Find any handler entry for this plugin and return that Arc.
                if let Some(arc) = self
                    .handlers
                    .values()
                    .find(|p| p.name() == plugin_name.as_str())
                {
                    return Some(arc);
                }
            }
        }
        None
    }

    /// Resolve the highest memory tier declared by any active plugin.
    ///
    /// Returns `Ok(Some(tier))` if at least one plugin declares a `[memory] tier`,
    /// `Ok(None)` if no plugin declares one, or `Err` if a plugin declares an
    /// unknown tier string (build-time error per MEMORY_POLICY.md §3.1 rule 3).
    pub fn resolve_plugin_memory_tier(
        &self,
    ) -> Result<Option<crate::MemoryTier>, crate::error::CompilerError> {
        let mut max_tier: Option<crate::MemoryTier> = None;

        for (plugin_name, manifest) in &self.manifests {
            if let Some(ref tier_str) = manifest.memory.tier {
                let tier = crate::MemoryTier::from_str(tier_str).ok_or_else(|| {
                    crate::error::CompilerError::PluginError {
                        message: format!(
                            "Plugin '{}' declares unknown memory tier '{}'. \
                                 Valid values: embedded, minimal, standard, heavy, canvas",
                            plugin_name, tier_str
                        ),
                        location: None,
                    }
                })?;
                tracing::debug!(
                    plugin = %plugin_name,
                    tier = %tier,
                    "Plugin declares memory tier"
                );
                max_tier = Some(max_tier.map_or(tier, |prev| std::cmp::max(prev, tier)));
            }
        }

        Ok(max_tier)
    }

    /// Get all lifecycle registrations collected from loaded plugins.
    pub fn registrations(&self) -> &PluginRegistrations {
        &self.registrations
    }

    /// Return `true` if any plugin registered server lifecycle hooks.
    pub fn has_server_registrations(&self) -> bool {
        !self.registrations.server.is_empty()
    }

    /// Return `true` if any plugin registered CLI commands.
    pub fn has_cli_registrations(&self) -> bool {
        !self.registrations.cli.is_empty()
    }

    /// Return `true` if any plugin registered build lifecycle hooks.
    pub fn has_build_registrations(&self) -> bool {
        !self.registrations.build.is_empty()
    }

    // ========================================================================
    // Permission Enforcement Methods
    // ========================================================================

    /// Check if a plugin is allowed to call a specific bridge function.
    ///
    /// Returns `true` only when the function appears in the plugin's `[bridge] functions`.
    /// Returns `false` if the plugin has no manifest or the function is not declared.
    pub fn plugin_can_call(&self, plugin_name: &str, function_name: &str) -> bool {
        match self.plugin_permissions.get(plugin_name) {
            None => false,
            Some(allowed) => allowed.contains(function_name),
        }
    }

    /// Get the set of bridge function names that a plugin is allowed to call.
    ///
    /// Returns `None` when the plugin is not registered, or `Some(&HashSet<String>)`
    /// with the declared bridge function names.
    pub fn plugin_allowed_functions(&self, plugin_name: &str) -> Option<&HashSet<String>> {
        self.plugin_permissions.get(plugin_name)
    }

    /// Build a mapping from language function names to bridge function names.
    ///
    /// Each entry maps a dot-notation language API name (e.g. `"db.query"`,
    /// `"req.param"`) to the underscore-prefixed WASM bridge name
    /// (e.g. `"_db_query"`, `"_req_param"`).
    ///
    /// The mapping is derived in priority order:
    /// 1. Explicit `maps_to` field on the `[[language.functions]]` entry.
    /// 2. Convention: replace `.` with `_` and prepend `_`
    ///    (`req.param` → `_req_param`).  Only accepted when the derived name
    ///    is declared in the same plugin's `[bridge]` section.
    /// 3. Explicit `aliases = [...]` list on the canonical
    ///    `foundation/platform-architecture/function-registry.toml` entry.
    ///    This covers bridges that the compiler registers directly (e.g. via
    ///    `codegen_module_builder.rs`) without a matching plugin manifest
    ///    entry, and lets new byte-safe bridges add their language surface by
    ///    editing only the registry (no codegen changes required).
    ///
    /// Language functions that cannot be resolved to a bridge function are
    /// silently omitted — they are treated as LSP-only (hover, completions).
    pub fn language_to_bridge_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();

        for manifest in self.manifests.values() {
            // Build the set of bridge function names declared by this plugin
            let bridge_names: HashSet<&str> = manifest
                .bridge
                .functions
                .iter()
                .map(|f| f.name.as_str())
                .collect();

            // Phase 1: Explicit [language].functions entries (highest priority)
            for func in &manifest.language.functions {
                if let Some(ref bridge_name) = func.maps_to {
                    // Explicit override always wins
                    map.insert(func.name.clone(), bridge_name.clone());
                } else {
                    // Convention: "req.param" → "_req_param"
                    let conventional = format!("_{}", func.name.replace('.', "_"));
                    if bridge_names.contains(conventional.as_str()) {
                        map.insert(func.name.clone(), conventional);
                    }
                }
            }

            // Phase 2: Auto-derive dot-notation aliases from ALL bridge functions
            // whose names follow the "_namespace_method" convention.
            // e.g. "_http_respond" → "http.respond", "_json_get" → "json.get"
            // Also generate camelCase variants: "_auth_get_session" → "auth.getSession"
            // Skip if already mapped in phase 1.
            //
            // IMPORTANT: Some names are intentionally excluded because they are
            // implemented as pure WASM functions (registered by json_class.rs) and
            // must never be redirected to a bridge. Mapping them to a bridge would
            // cause the bridge (which expects a string parameter) to receive a boxed
            // `any` value, producing garbage output. The pure WASM path handles all
            // types correctly via AnyTypeTag dispatch (GEN004).
            const WASM_ONLY_FUNCTIONS: &[&str] =
                &["json.encode", "json.dataToText", "json.prettyDataToText"];
            for bf in &manifest.bridge.functions {
                if let Some(stripped) = bf.name.strip_prefix('_') {
                    if let Some(underscore_pos) = stripped.find('_') {
                        let namespace = &stripped[..underscore_pos];
                        let method = &stripped[underscore_pos + 1..];
                        // snake_case variant: "auth.get_session"
                        let dot_name = format!("{}.{}", namespace, method);
                        if WASM_ONLY_FUNCTIONS.contains(&dot_name.as_str()) {
                            continue;
                        }
                        map.entry(dot_name).or_insert_with(|| bf.name.clone());
                        // camelCase variant: "auth.getSession"
                        if method.contains('_') {
                            let camel_method = snake_to_camel(method);
                            let camel_dot_name = format!("{}.{}", namespace, camel_method);
                            if WASM_ONLY_FUNCTIONS.contains(&camel_dot_name.as_str()) {
                                continue;
                            }
                            map.entry(camel_dot_name).or_insert_with(|| bf.name.clone());
                        }
                    }
                }
            }
        }

        // Phase 3: Registry-level aliases.
        //
        // The canonical function-registry.toml lists dot-notation aliases on
        // every bridge that has one. Some bridges (e.g. `_req_body_bytes`,
        // `_fs_write_bytes`) are registered directly by the compiler in
        // `codegen_module_builder.rs` and never appear in a plugin manifest's
        // `[bridge]` section — Phase 2 therefore can't see them. Consult the
        // registry so those aliases still reach the codegen resolver.
        //
        // We use `or_insert_with` so plugin-declared mappings win over the
        // registry copy (the plugin manifest is authoritative for anything it
        // covers). Load failures are silently ignored — a missing registry is
        // reported through the separate validation path.
        const WASM_ONLY_FUNCTIONS: &[&str] =
            &["json.encode", "json.dataToText", "json.prettyDataToText"];
        if let Ok(idx) = crate::plugins::registry_loader::RegistryIndex::load() {
            for reg_fn in idx.functions() {
                for alias in &reg_fn.aliases {
                    if WASM_ONLY_FUNCTIONS.contains(&alias.as_str()) {
                        continue;
                    }
                    map.entry(alias.clone())
                        .or_insert_with(|| reg_fn.name.clone());
                }
            }
        }

        map
    }

    /// Returns all `[[language.functions]]` entries across loaded plugins,
    /// keyed by the language function name.  Used by the compiler to apply
    /// `params`, `returns`, and `param_defaults` overrides when registering
    /// language-alias external functions.
    pub fn language_function_defs(&self) -> HashMap<String, &crate::plugins::PluginFunctionDef> {
        let mut map = HashMap::new();
        for manifest in self.manifests.values() {
            for func in &manifest.language.functions {
                map.entry(func.name.clone()).or_insert(func);
            }
        }
        map
    }

    /// Build a mapping from language function names to plugin-emitted helper
    /// function names for `[[language.functions]]` entries that declare
    /// `maps_to_helper`.
    ///
    /// This resolves the "third path" for language APIs whose implementation is
    /// a Clean-language wrapper the plugin generates during framework-block
    /// expansion (via `_batch_func` + `_emit_helpers_batch`, or by pushing onto
    /// `expansion.functions`), rather than a direct bridge call.
    ///
    /// Example: `auth.jwt.sign` → `jwt_sign` — the plugin emits `jwt_sign` as a
    /// user-level function that reads `__jwt_secret` / `__jwt_alg` before
    /// delegating to `_jwt_sign`; without this mapping the language name is
    /// dropped as "LSP-only" and the call site fails with SEM007 in the
    /// resolver / "not found in function map" in codegen.
    pub fn language_to_helper_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for manifest in self.manifests.values() {
            for func in &manifest.language.functions {
                if let Some(ref helper) = func.maps_to_helper {
                    // Explicit `maps_to` always wins — don't shadow a real
                    // bridge alias with a helper hop.
                    if func.maps_to.is_some() {
                        continue;
                    }
                    map.insert(func.name.clone(), helper.clone());
                }
            }
        }
        map
    }

    /// Look up which plugin declared ownership of a given bridge function.
    ///
    /// Scans all manifests and returns the first plugin name whose `[bridge]`
    /// section lists a function with the given name.  Returns `None` if no
    /// manifest claims the function.
    pub fn bridge_function_owner(&self, function_name: &str) -> Option<&str> {
        for (plugin_name, allowed) in &self.plugin_permissions {
            if allowed.contains(function_name) {
                return Some(plugin_name.as_str());
            }
        }
        None
    }

    // ========================================================================
    // Language Server Protocol (LSP) Aggregation Methods
    // ========================================================================

    /// Get all keywords from all registered plugins
    ///
    /// Returns a combined list of keywords from all plugins for syntax highlighting.
    /// Also includes the block names themselves (e.g., "endpoints", "data").
    ///
    /// # Example
    /// ```ignore
    /// let registry = PluginRegistry::builder()
    ///     .add(WebPlugin::new())
    ///     .build()?;
    ///
    /// let keywords = registry.get_all_keywords();
    /// // Returns: ["endpoints", "GET", "POST", "PUT", "DELETE", ...]
    /// ```
    pub fn get_all_keywords(&self) -> Vec<&str> {
        let mut keywords: Vec<&str> = Vec::new();

        // Add block names as keywords
        for block_name in self.handlers.keys() {
            keywords.push(block_name.as_str());
        }

        // Add keywords from each unique plugin
        let mut seen_plugins = std::collections::HashSet::new();
        for plugin in self.handlers.values() {
            if seen_plugins.insert(plugin.name()) {
                keywords.extend(plugin.get_keywords());
            }
        }

        keywords
    }

    /// Get all block keywords that plugins handle
    ///
    /// Returns just the block handler names (e.g., "data", "endpoints", "screen")
    /// without the additional DSL keywords. This is used by the parser to recognize
    /// plugin-defined keywords that don't require colons.
    ///
    /// # Returns
    /// A vector of block keyword strings that plugins handle
    ///
    /// # Example
    /// ```ignore
    /// let registry = PluginRegistry::builder()
    ///     .add(DataPlugin::new())  // Handles "data"
    ///     .add(HttpPlugin::new())  // Handles "endpoints"
    ///     .build()?;
    ///
    /// let keywords = registry.get_all_block_keywords();
    /// // Returns: ["data", "endpoints"]
    /// ```
    pub fn get_all_block_keywords(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }

    /// Get completions for a specific block type
    ///
    /// Delegates to the plugin that handles the given block type.
    ///
    /// # Arguments
    /// * `block_name` - The block type (e.g., "endpoints")
    /// * `ctx` - Context about the current editing position
    ///
    /// # Returns
    /// List of completion items, or empty if no plugin handles the block
    pub fn get_completions(
        &self,
        block_name: &str,
        ctx: &PluginLspContext,
    ) -> Vec<PluginCompletionItem> {
        if let Some(plugin) = self.handlers.get(block_name) {
            plugin.get_completions(ctx)
        } else {
            Vec::new()
        }
    }

    /// Get completions for all plugins (for top-level completion)
    ///
    /// Returns block-level completions (the block names themselves).
    ///
    /// # Returns
    /// Completion items for all registered block types
    pub fn get_block_completions(&self) -> Vec<PluginCompletionItem> {
        let mut completions = Vec::new();
        let mut seen_plugins = std::collections::HashSet::new();

        for (block_name, plugin) in &self.handlers {
            if seen_plugins.insert(plugin.name()) {
                completions.push(PluginCompletionItem {
                    label: format!("{}:", block_name),
                    kind: super::PluginCompletionKind::Keyword,
                    detail: Some(format!("Plugin: {}", plugin.name())),
                    documentation: Some(plugin.description().to_string()),
                    insert_text: Some(format!("{}:\n\t${{1:content}}", block_name)),
                    is_snippet: true,
                });
            }
        }

        completions
    }

    /// Get hover information for a keyword
    ///
    /// First checks if the keyword is a block name, then delegates to plugins.
    ///
    /// # Arguments
    /// * `keyword` - The word being hovered over
    /// * `block_name` - Optional: the block type context
    ///
    /// # Returns
    /// Hover information if found
    pub fn get_hover_info(
        &self,
        keyword: &str,
        block_name: Option<&str>,
    ) -> Option<PluginHoverInfo> {
        // Check if hovering over a block name
        if let Some(plugin) = self.handlers.get(keyword) {
            return Some(PluginHoverInfo {
                content: format!(
                    "**{}:** Block\n\n{}\n\n*Plugin: {} v{}*",
                    keyword,
                    plugin.description(),
                    plugin.name(),
                    plugin.version()
                ),
            });
        }

        // If we're inside a block, delegate to that plugin
        if let Some(block) = block_name {
            if let Some(plugin) = self.handlers.get(block) {
                return plugin.get_hover_info(keyword);
            }
        }

        // Try all plugins (for keywords that might appear outside their blocks)
        for plugin in self.handlers.values() {
            if let Some(info) = plugin.get_hover_info(keyword) {
                return Some(info);
            }
        }

        None
    }

    /// Get diagnostics for a block's content
    ///
    /// Delegates to the plugin that handles the given block type.
    ///
    /// # Arguments
    /// * `block_name` - The block type (e.g., "endpoints")
    /// * `content` - The content inside the block
    ///
    /// # Returns
    /// List of diagnostics (errors, warnings, etc.)
    pub fn get_diagnostics(&self, block_name: &str, content: &str) -> Vec<PluginDiagnostic> {
        if let Some(plugin) = self.handlers.get(block_name) {
            plugin.get_diagnostics(content)
        } else {
            Vec::new()
        }
    }

    /// Get all plugin descriptions for documentation
    ///
    /// Returns a list of (name, block_types, description) for all registered plugins.
    pub fn get_plugin_info(&self) -> Vec<(&str, Vec<&str>, &str)> {
        let mut info = Vec::new();
        let mut seen_plugins = std::collections::HashSet::new();

        for plugin in self.handlers.values() {
            if seen_plugins.insert(plugin.name()) {
                info.push((
                    plugin.name(),
                    plugin.handles().to_vec(),
                    plugin.description(),
                ));
            }
        }

        info
    }
}

impl fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginRegistry")
            .field("registered_plugins", &self.registered_plugins)
            .field("handlers", &self.handlers.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Builder for creating an immutable PluginRegistry
///
/// This builder allows plugins to be added fluently, then builds
/// an immutable registry that cannot be modified after creation.
///
/// # Example
///
/// ```ignore
/// use my_framework_plugins::MyPlugin;
///
/// let registry = PluginRegistryBuilder::new()
///     .add(MyPlugin::new())
///     .build()?;
/// ```
pub struct PluginRegistryBuilder {
    plugins: Vec<Arc<dyn FrameworkPlugin>>,
    bridge_functions: Vec<BridgeFunction>,
    manifests: HashMap<String, PluginManifest>,
    registrations: PluginRegistrations,
    /// Accumulated permission allowlists (plugin name → allowed bridge function names)
    plugin_permissions: HashMap<String, HashSet<String>>,
    /// Override for the registry-conformance validation policy. `None` means
    /// fall back to `ValidationPolicy::from_env()` at `build()` time.
    validation_policy: Option<crate::plugins::registry_loader::ValidationPolicy>,
}

impl PluginRegistryBuilder {
    /// Create a new empty builder
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            bridge_functions: Vec::new(),
            manifests: HashMap::new(),
            registrations: PluginRegistrations::default(),
            plugin_permissions: HashMap::new(),
            validation_policy: None,
        }
    }

    /// Override the registry-conformance validation policy for this build.
    /// Primarily for tests; production callers should use the
    /// `CLEAN_PLUGIN_REGISTRY_VALIDATION` env var.
    pub fn with_validation_policy(
        mut self,
        policy: crate::plugins::registry_loader::ValidationPolicy,
    ) -> Self {
        self.validation_policy = Some(policy);
        self
    }

    /// Add a plugin to the registry
    ///
    /// Plugins are validated for conflicts when `build()` is called.
    ///
    /// # Arguments
    /// * `plugin` - The plugin to add
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add(mut self, plugin: impl FrameworkPlugin + 'static) -> Self {
        self.plugins.push(Arc::new(plugin));
        self
    }

    /// Add a plugin via Arc (for when you already have an Arc)
    ///
    /// # Arguments
    /// * `plugin` - The plugin Arc to add
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add_arc(mut self, plugin: Arc<dyn FrameworkPlugin>) -> Self {
        self.plugins.push(plugin);
        self
    }

    /// Add bridge functions from a plugin manifest
    ///
    /// Bridge functions are runtime-provided functions declared in plugin.toml [bridge] sections.
    ///
    /// # Arguments
    /// * `bridge` - The bridge section from a plugin manifest
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add_bridge_functions(
        mut self,
        bridge: &crate::plugins::plugin_abi::PluginBridge,
    ) -> Self {
        self.bridge_functions.extend(bridge.functions.clone());
        self
    }

    /// Add a single bridge function
    ///
    /// # Arguments
    /// * `func` - The bridge function to add
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add_bridge_function(mut self, func: BridgeFunction) -> Self {
        self.bridge_functions.push(func);
        self
    }

    /// Add a full plugin manifest for enforcement and path detection.
    ///
    /// In addition to storing the manifest this also builds the permission
    /// allowlist for the plugin from its `[bridge] functions` declarations.
    /// Plugins that declare no bridge functions receive an empty set, which
    /// means they are not permitted to call any bridge function.
    ///
    /// # Arguments
    /// * `name` - Plugin name
    /// * `manifest` - The full plugin manifest
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add_manifest(mut self, name: String, manifest: PluginManifest) -> Self {
        // Build the permission allowlist from the manifest's bridge declarations.
        let allowed: HashSet<String> = manifest
            .bridge
            .functions
            .iter()
            .map(|f| f.name.clone())
            .collect();
        self.plugin_permissions.insert(name.clone(), allowed);
        self.manifests.insert(name, manifest);
        self
    }

    /// Inspect a plugin manifest and, for every lifecycle export that is
    /// declared, record a default registration entry.
    ///
    /// The entries are placeholders with default field values.  The actual
    /// values are populated later by `WasmPluginAdapter` when it calls the
    /// corresponding WASM export.  This two-phase approach keeps the builder
    /// ordering-independent: callers may add registrations before or after
    /// the WASM adapter is loaded.
    ///
    /// # Arguments
    /// * `plugin_name` - The plugin name used as the key in each entry
    /// * `manifest`    - The parsed `plugin.toml` manifest
    ///
    /// # Returns
    /// Self for method chaining
    pub fn add_registration(mut self, plugin_name: &str, manifest: &PluginManifest) -> Self {
        if manifest.exports.register_server.is_some() {
            self.registrations
                .server
                .push((plugin_name.to_string(), ServerRegistration::default()));
        }
        if manifest.exports.register_cli.is_some() {
            self.registrations
                .cli
                .push((plugin_name.to_string(), CliRegistration::default()));
        }
        if manifest.exports.register_data.is_some() {
            self.registrations
                .data
                .push((plugin_name.to_string(), DataRegistration::default()));
        }
        if manifest.exports.register_build.is_some() {
            self.registrations
                .build
                .push((plugin_name.to_string(), BuildRegistration::default()));
        }
        self
    }

    /// Replace the `ServerRegistration` for `plugin_name` with `reg`.
    ///
    /// If no entry for `plugin_name` exists (which should not normally happen
    /// because `add_registration` is called first), the entry is appended.
    pub fn update_server_registration(
        mut self,
        plugin_name: &str,
        reg: ServerRegistration,
    ) -> Self {
        if let Some(entry) = self
            .registrations
            .server
            .iter_mut()
            .find(|(name, _)| name == plugin_name)
        {
            entry.1 = reg;
        } else {
            self.registrations
                .server
                .push((plugin_name.to_string(), reg));
        }
        self
    }

    /// Replace the `CliRegistration` for `plugin_name` with `reg`.
    pub fn update_cli_registration(mut self, plugin_name: &str, reg: CliRegistration) -> Self {
        if let Some(entry) = self
            .registrations
            .cli
            .iter_mut()
            .find(|(name, _)| name == plugin_name)
        {
            entry.1 = reg;
        } else {
            self.registrations.cli.push((plugin_name.to_string(), reg));
        }
        self
    }

    /// Replace the `DataRegistration` for `plugin_name` with `reg`.
    pub fn update_data_registration(mut self, plugin_name: &str, reg: DataRegistration) -> Self {
        if let Some(entry) = self
            .registrations
            .data
            .iter_mut()
            .find(|(name, _)| name == plugin_name)
        {
            entry.1 = reg;
        } else {
            self.registrations.data.push((plugin_name.to_string(), reg));
        }
        self
    }

    /// Replace the `BuildRegistration` for `plugin_name` with `reg`.
    pub fn update_build_registration(mut self, plugin_name: &str, reg: BuildRegistration) -> Self {
        if let Some(entry) = self
            .registrations
            .build
            .iter_mut()
            .find(|(name, _)| name == plugin_name)
        {
            entry.1 = reg;
        } else {
            self.registrations
                .build
                .push((plugin_name.to_string(), reg));
        }
        self
    }

    /// Build the immutable plugin registry
    ///
    /// This validates that there are no block type conflicts between plugins.
    ///
    /// # Returns
    /// * `Ok(PluginRegistry)` - The immutable registry
    /// * `Err(PluginError::RegistrationConflict)` - If plugins conflict
    ///
    /// # Example
    ///
    /// ```ignore
    /// use my_framework_plugins::MyPlugin;
    ///
    /// let registry = PluginRegistry::builder()
    ///     .add(MyPlugin::new())
    ///     .build()?;
    /// ```
    pub fn build(self) -> Result<PluginRegistry, PluginError> {
        let mut handlers = HashMap::new();
        let mut expression_handlers: HashMap<String, Arc<dyn FrameworkPlugin>> = HashMap::new();
        let mut registered_plugins = Vec::new();

        // Validate and register all plugins
        for plugin in self.plugins {
            let plugin_name = plugin.name().to_string();

            // Check for conflicts first
            for block_name in plugin.handles() {
                if let Some(existing) = handlers.get(*block_name) {
                    let existing_plugin: &Arc<dyn FrameworkPlugin> = existing;
                    return Err(PluginError::RegistrationConflict {
                        block_name: block_name.to_string(),
                        existing_plugin: existing_plugin.name().to_string(),
                        new_plugin: plugin_name,
                    });
                }
            }

            // Register all block handlers for this plugin
            for block_name in plugin.handles() {
                handlers.insert(block_name.to_string(), Arc::clone(&plugin));
            }

            // Register ORM expression patterns for this plugin
            for pattern in plugin.expression_patterns() {
                expression_handlers.insert(pattern.clone(), Arc::clone(&plugin));
            }

            registered_plugins.push(plugin_name);
        }

        // Plugin Contracts v2 — validate callback contracts.
        // For each [bridge.functions.callback] block:
        //   1. plugin_target must reference a loaded plugin (PLUGIN-CALLBACK-TARGET-MISSING).
        //   2. (bridge_function, purpose) must not collide across plugins (PLUGIN-CALLBACK-CONFLICT).
        // See foundation/spec/plugins/contracts/bridge-host-classes.md §4.
        //
        // "Loaded" includes plugins that supplied a manifest via `add_manifest`
        // even without a corresponding FrameworkPlugin object. The production
        // path through WasmPluginLoader populates both sets in lockstep, but
        // manifest-only setups (tests, lifecycle slot exploration) must still
        // pass validation.
        let loaded_names: std::collections::HashSet<&str> = registered_plugins
            .iter()
            .map(|s| s.as_str())
            .chain(self.manifests.keys().map(|s| s.as_str()))
            .collect();
        let mut seen_callbacks: HashMap<(String, String), String> = HashMap::new();
        for (declaring_plugin, manifest) in &self.manifests {
            for bridge in &manifest.bridge.functions {
                let Some(cb) = &bridge.callback else {
                    continue;
                };
                // §4 #1 — target plugin must be loaded.
                if !loaded_names.contains(cb.plugin_target.as_str()) {
                    return Err(PluginError::ValidationFailed {
                        plugin_name: declaring_plugin.clone(),
                        message: format!(
                            "PLUGIN-CALLBACK-TARGET-MISSING: bridge `{}` declares callback with plugin_target = `{}`, but that plugin is not loaded. Add it to plugins: in app.cln or remove the callback declaration.",
                            bridge.name, cb.plugin_target
                        ),
                        location: None,
                    });
                }
                // §4 #3 — no two callbacks may share (bridge, purpose).
                let key = (bridge.name.clone(), cb.purpose.clone());
                if let Some(prior) = seen_callbacks.get(&key) {
                    return Err(PluginError::ValidationFailed {
                        plugin_name: declaring_plugin.clone(),
                        message: format!(
                            "PLUGIN-CALLBACK-CONFLICT: bridge `{}` purpose `{}` is declared by both `{}` and `{}`. A given (bridge, purpose) pair may have at most one callback contract.",
                            bridge.name, cb.purpose, prior, declaring_plugin
                        ),
                        location: None,
                    });
                }
                seen_callbacks.insert(key, declaring_plugin.clone());
            }
        }

        // Registry conformance validation — gated per-plugin via the
        // CLEAN_PLUGIN_REGISTRY_VALIDATION env var. Default (unset) is `all`:
        // every plugin manifest must match foundation/platform-architecture/
        // function-registry.toml at WASM-shape level.
        //
        // Set explicitly to `"off"` for emergency triage when a brand-new
        // plugin/registry edit is mid-flight, or to a comma-separated allowlist
        // (e.g. `frame.data,frame.auth`) to narrow the scope. See
        // `registry_loader::ValidationPolicy` for the grammar.
        let policy = self
            .validation_policy
            .clone()
            .unwrap_or_else(crate::plugins::registry_loader::ValidationPolicy::from_env);
        if policy.is_active() {
            match crate::plugins::registry_loader::RegistryIndex::load() {
                Ok(idx) => {
                    let mut all_issues: Vec<String> = Vec::new();
                    let mut sorted_manifests: Vec<(&String, &PluginManifest)> =
                        self.manifests.iter().collect();
                    sorted_manifests.sort_by(|a, b| a.0.cmp(b.0));
                    for (plugin_name, manifest) in sorted_manifests {
                        if !policy.includes(plugin_name) {
                            continue;
                        }
                        for decl in &manifest.bridge.functions {
                            all_issues.extend(idx.check_bridge(plugin_name, decl));
                        }
                    }
                    if !all_issues.is_empty() {
                        return Err(PluginError::ValidationFailed {
                            plugin_name: "(registry)".to_string(),
                            message: format!(
                                "PLUGIN-REGISTRY-DRIFT: {} bridge function declaration(s) \
do not match foundation/platform-architecture/function-registry.toml. \
Fix the plugin.toml entries or update the registry (with developer approval). \
To exclude a plugin from this check temporarily, narrow the \
CLEAN_PLUGIN_REGISTRY_VALIDATION allowlist:\n{}",
                                all_issues.len(),
                                all_issues.join("\n"),
                            ),
                            location: None,
                        });
                    }
                }
                Err(e) => {
                    return Err(PluginError::ValidationFailed {
                        plugin_name: "(registry)".to_string(),
                        message: format!(
                            "failed to load function-registry.toml: {e}. \
This is a compiler build issue, not a plugin issue."
                        ),
                        location: None,
                    });
                }
            }
        }

        Ok(PluginRegistry {
            handlers,
            expression_handlers,
            registered_plugins,
            bridge_functions: self.bridge_functions,
            manifests: self.manifests,
            build_state: super::new_build_state(),
            registrations: self.registrations,
            plugin_permissions: self.plugin_permissions,
        })
    }
}

impl Default for PluginRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::PluginResult;

    struct TestPlugin {
        name: &'static str,
        handles: &'static [&'static str],
    }

    impl FrameworkPlugin for TestPlugin {
        fn name(&self) -> &'static str {
            self.name
        }

        fn handles(&self) -> &'static [&'static str] {
            self.handles
        }

        fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
            // Simple test: return empty expansion
            let _ = block;
            Ok(vec![])
        }
    }

    #[test]
    fn test_register_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(TestPlugin {
            name: "test.plugin",
            handles: &["test"],
        });

        assert!(registry.register(plugin).is_ok());
        assert!(registry.handles("test"));
        assert!(!registry.handles("unknown"));
    }

    #[test]
    fn test_registration_conflict() {
        let mut registry = PluginRegistry::new();

        let plugin1 = Arc::new(TestPlugin {
            name: "plugin1",
            handles: &["test"],
        });
        let plugin2 = Arc::new(TestPlugin {
            name: "plugin2",
            handles: &["test"],
        });

        assert!(registry.register(plugin1).is_ok());

        let result = registry.register(plugin2);
        assert!(matches!(
            result,
            Err(PluginError::RegistrationConflict { .. })
        ));
    }

    #[test]
    fn test_expand_unknown_block() {
        let registry = PluginRegistry::new();
        let block = FrameworkBlock {
            name: "unknown".to_string(),
            content: "content".to_string(),
            attributes: vec![],
            location: None,
        };

        let result = registry.expand(&block);
        assert!(matches!(result, Err(PluginError::UnknownBlockType { .. })));
    }

    #[test]
    fn test_bridge_function_registration() {
        use crate::plugins::plugin_abi::{BridgeFunction, PluginBridge};

        // Create bridge functions like they would come from plugin.toml
        let bridge = PluginBridge {
            functions: vec![
                BridgeFunction {
                    name: "_db_query".to_string(),
                    params: vec!["string".to_string(), "string".to_string()],
                    returns: "string".to_string(),
                    module: "env".to_string(),
                    description: Some("Execute SELECT query".to_string()),
                    expand_strings: true,
                    ..Default::default()
                },
                BridgeFunction {
                    name: "_db_execute".to_string(),
                    params: vec!["string".to_string(), "string".to_string()],
                    returns: "i32".to_string(),
                    module: "env".to_string(),
                    description: Some("Execute INSERT/UPDATE/DELETE".to_string()),
                    expand_strings: true,
                    ..Default::default()
                },
            ],
        };

        // Build registry with bridge functions
        let registry = PluginRegistryBuilder::new()
            .add_bridge_functions(&bridge)
            .build()
            .expect("Failed to build registry");

        // Verify bridge functions are registered
        assert_eq!(registry.bridge_functions().len(), 2);
        assert!(registry.is_bridge_function("_db_query"));
        assert!(registry.is_bridge_function("_db_execute"));
        assert!(!registry.is_bridge_function("_unknown"));

        // Verify the function properties
        let db_query = registry.get_bridge_function("_db_query").unwrap();
        assert_eq!(db_query.name, "_db_query");
        assert_eq!(db_query.params.len(), 2);
        assert_eq!(db_query.returns, "string");
        assert!(db_query.expand_strings);
    }

    #[test]
    fn test_language_to_bridge_map_convention() {
        use crate::plugins::plugin_abi::{
            BridgeFunction, PluginBridge, PluginCompatibility, PluginExports, PluginFunctionDef,
            PluginHandles, PluginInfo, PluginLanguage, PluginManifest,
        };

        // Build a manifest with bridge functions and matching language functions
        let manifest = PluginManifest {
            plugin: PluginInfo {
                name: "frame.data".to_string(),
                version: "1.0.0".to_string(),
                description: "Data plugin".to_string(),
                author: "Test".to_string(),
            },
            compatibility: PluginCompatibility::default(),
            handles: PluginHandles {
                blocks: vec!["data".to_string()],
                expressions: Vec::new(),
            },
            exports: PluginExports::default(),
            bridge: PluginBridge {
                functions: vec![
                    BridgeFunction {
                        name: "_db_query".to_string(),
                        params: vec!["string".to_string(), "string".to_string()],
                        returns: "string".to_string(),
                        module: "env".to_string(),
                        description: None,
                        expand_strings: true,
                        ..Default::default()
                    },
                    BridgeFunction {
                        name: "_db_execute".to_string(),
                        params: vec!["string".to_string(), "string".to_string()],
                        returns: "i32".to_string(),
                        module: "env".to_string(),
                        description: None,
                        expand_strings: true,
                        ..Default::default()
                    },
                ],
            },
            language: PluginLanguage {
                blocks: vec!["data".to_string()],
                keywords: vec![],
                types: vec![],
                functions: vec![
                    // Convention-based: "db.query" -> "_db_query"
                    PluginFunctionDef {
                        name: "db.query".to_string(),
                        signature: "db.query(sql, params) -> string".to_string(),
                        description: "Execute SELECT query".to_string(),
                        maps_to: None,
                        maps_to_helper: None,
                        params: None,
                        returns: None,
                        param_defaults: vec![],
                    },
                    // Explicit override: "db.run" -> "_db_execute"
                    PluginFunctionDef {
                        name: "db.run".to_string(),
                        signature: "db.run(sql, params) -> integer".to_string(),
                        description: "Execute INSERT/UPDATE/DELETE".to_string(),
                        maps_to: Some("_db_execute".to_string()),
                        maps_to_helper: None,
                        params: None,
                        returns: None,
                        param_defaults: vec![],
                    },
                    // No matching bridge: should not appear in the map
                    PluginFunctionDef {
                        name: "db.nonexistent".to_string(),
                        signature: "db.nonexistent() -> void".to_string(),
                        description: "No bridge counterpart".to_string(),
                        maps_to: None,
                        maps_to_helper: None,
                        params: None,
                        returns: None,
                        param_defaults: vec![],
                    },
                ],
                completions: vec![],
                owns_paths: vec![],
            },
            ai: Default::default(),
            paths: Default::default(),
            enforcement: Default::default(),
            memory: Default::default(),
            build: Default::default(),
            lifecycle: Default::default(),
            artifacts: Vec::new(),
            blocks: Default::default(),
        };

        let registry = PluginRegistryBuilder::new()
            .with_validation_policy(crate::plugins::registry_loader::ValidationPolicy::Off)
            .add_manifest("frame.data".to_string(), manifest)
            .build()
            .expect("Failed to build registry");

        let map = registry.language_to_bridge_map();

        // Convention-derived mapping (phase 1)
        assert_eq!(map.get("db.query"), Some(&"_db_query".to_string()));
        // Explicit override (phase 1)
        assert_eq!(map.get("db.run"), Some(&"_db_execute".to_string()));
        // Auto-derived from bridge "_db_execute" (phase 2)
        assert_eq!(map.get("db.execute"), Some(&"_db_execute".to_string()));
        // Convention name that isn't a real bridge — must NOT appear.
        // (Registry-level aliases from the canonical function-registry.toml
        // also contribute mappings via phase 3, so we no longer assert on
        // the total size — the phase 1/2 entries above are what this test
        // guards.)
        assert!(!map.contains_key("db.nonexistent"));
    }

    #[test]
    fn test_language_to_bridge_map_excludes_wasm_only_functions() {
        // GEN004: _json_encode and similar bridge functions must NOT auto-derive
        // to json.encode / json.dataToText, because those are pure WASM functions.
        // Mapping them to the bridge causes the bridge (which expects a string param)
        // to receive a boxed `any` object value, producing empty JSON output.
        use crate::plugins::plugin_abi::{
            BridgeFunction, PluginBridge, PluginCompatibility, PluginExports, PluginHandles,
            PluginInfo, PluginLanguage, PluginManifest,
        };

        let manifest = PluginManifest {
            plugin: PluginInfo {
                name: "frame.server".to_string(),
                version: "1.0.0".to_string(),
                description: "Server plugin".to_string(),
                author: "Test".to_string(),
            },
            compatibility: PluginCompatibility::default(),
            handles: PluginHandles {
                blocks: vec!["server".to_string()],
                expressions: Vec::new(),
            },
            exports: PluginExports::default(),
            bridge: PluginBridge {
                functions: vec![
                    BridgeFunction {
                        name: "_json_encode".to_string(),
                        params: vec!["string".to_string()],
                        returns: "string".to_string(),
                        module: "env".to_string(),
                        description: None,
                        expand_strings: true,
                        ..Default::default()
                    },
                    BridgeFunction {
                        name: "_req_body".to_string(),
                        params: vec![],
                        returns: "string".to_string(),
                        module: "env".to_string(),
                        description: None,
                        expand_strings: false,
                        ..Default::default()
                    },
                ],
            },
            language: PluginLanguage {
                blocks: vec!["server".to_string()],
                keywords: vec![],
                types: vec![],
                functions: vec![],
                completions: vec![],
                owns_paths: vec![],
            },
            ai: Default::default(),
            paths: Default::default(),
            enforcement: Default::default(),
            memory: Default::default(),
            build: Default::default(),
            lifecycle: Default::default(),
            artifacts: Vec::new(),
            blocks: Default::default(),
        };

        let registry = PluginRegistryBuilder::new()
            .add_manifest("frame.server".to_string(), manifest)
            .build()
            .expect("Failed to build registry");

        let map = registry.language_to_bridge_map();

        // _req_body auto-derives to req.body — this is fine
        assert_eq!(map.get("req.body"), Some(&"_req_body".to_string()));
        // _json_encode must NOT auto-derive to json.encode — pure WASM function (GEN004)
        assert!(
            !map.contains_key("json.encode"),
            "json.encode must not map to _json_encode bridge"
        );
        // _json_encode also must not derive to json.dataToText or json.prettyDataToText
        assert!(!map.contains_key("json.dataToText"));
        assert!(!map.contains_key("json.prettyDataToText"));
    }

    /// FRAME-AUTH-JWT-HELPERS-UNREACHABLE regression: a `[[language.functions]]`
    /// entry with `maps_to_helper` (and no `maps_to`) must appear in the
    /// language-to-helper map so the resolver and codegen can route calls to the
    /// plugin-emitted helper function.
    #[test]
    fn test_language_to_helper_map_registers_maps_to_helper_entries() {
        use crate::plugins::plugin_abi::{
            BridgeFunction, PluginBridge, PluginCompatibility, PluginExports, PluginFunctionDef,
            PluginHandles, PluginInfo, PluginLanguage, PluginManifest,
        };

        let manifest = PluginManifest {
            plugin: PluginInfo {
                name: "frame.auth".to_string(),
                version: "2.4.0".to_string(),
                description: "Auth plugin".to_string(),
                author: "Test".to_string(),
            },
            compatibility: PluginCompatibility::default(),
            handles: PluginHandles {
                blocks: vec!["auth".to_string()],
                expressions: Vec::new(),
            },
            exports: PluginExports::default(),
            bridge: PluginBridge {
                functions: vec![BridgeFunction {
                    name: "_jwt_sign".to_string(),
                    params: vec![
                        "string".to_string(),
                        "string".to_string(),
                        "string".to_string(),
                    ],
                    returns: "string".to_string(),
                    module: "env".to_string(),
                    description: None,
                    expand_strings: true,
                    ..Default::default()
                }],
            },
            language: PluginLanguage {
                blocks: vec!["auth".to_string()],
                keywords: vec![],
                types: vec![],
                functions: vec![
                    // auth.jwt.sign delegates to the plugin-emitted `jwt_sign`
                    // helper (added to program.functions during auth: expansion).
                    PluginFunctionDef {
                        name: "auth.jwt.sign".to_string(),
                        signature: "auth.jwt.sign(claims)".to_string(),
                        description: "Sign JWT with configured secret".to_string(),
                        maps_to: None,
                        maps_to_helper: Some("jwt_sign".to_string()),
                        params: None,
                        returns: None,
                        param_defaults: vec![],
                    },
                    // Explicit maps_to always wins over maps_to_helper.
                    PluginFunctionDef {
                        name: "jwt.sign".to_string(),
                        signature: "jwt.sign(payload, secret, algo)".to_string(),
                        description: "Raw JWT sign".to_string(),
                        maps_to: Some("_jwt_sign".to_string()),
                        maps_to_helper: Some("jwt_sign_should_be_ignored".to_string()),
                        params: None,
                        returns: None,
                        param_defaults: vec![],
                    },
                    // Entries with neither maps_to nor maps_to_helper stay LSP-only.
                    PluginFunctionDef {
                        name: "auth.jwt.verify".to_string(),
                        signature: "auth.jwt.verify(token)".to_string(),
                        description: "Verify JWT".to_string(),
                        maps_to: None,
                        maps_to_helper: None,
                        params: None,
                        returns: None,
                        param_defaults: vec![],
                    },
                ],
                completions: vec![],
                owns_paths: vec![],
            },
            ai: Default::default(),
            paths: Default::default(),
            enforcement: Default::default(),
            memory: Default::default(),
            build: Default::default(),
            lifecycle: Default::default(),
            artifacts: Vec::new(),
            blocks: Default::default(),
        };

        let registry = PluginRegistryBuilder::new()
            .with_validation_policy(crate::plugins::registry_loader::ValidationPolicy::Off)
            .add_manifest("frame.auth".to_string(), manifest)
            .build()
            .expect("Failed to build registry");

        let helper_map = registry.language_to_helper_map();
        assert_eq!(
            helper_map.get("auth.jwt.sign"),
            Some(&"jwt_sign".to_string()),
            "auth.jwt.sign must map to the plugin-emitted jwt_sign helper"
        );
        // Explicit maps_to takes precedence — the entry stays out of the helper map.
        assert!(
            !helper_map.contains_key("jwt.sign"),
            "explicit maps_to must win over maps_to_helper"
        );
        // No mapping declared → not in helper map (stays LSP-only).
        assert!(!helper_map.contains_key("auth.jwt.verify"));

        // The bridge alias for jwt.sign still resolves through the ordinary path.
        let bridge_map = registry.language_to_bridge_map();
        assert_eq!(bridge_map.get("jwt.sign"), Some(&"_jwt_sign".to_string()));
    }

    /// Helpers shared across the v2 callback validation tests below.
    mod v2_callback_helpers {
        use crate::plugins::plugin_abi::{
            BridgeCallback, BridgeFunction, PluginBridge, PluginCompatibility, PluginExports,
            PluginHandles, PluginInfo, PluginLanguage, PluginManifest,
        };

        pub(super) fn manifest_with_callback(
            plugin_name: &str,
            bridge_name: &str,
            callback: Option<BridgeCallback>,
        ) -> PluginManifest {
            PluginManifest {
                plugin: PluginInfo {
                    name: plugin_name.to_string(),
                    version: "1.0.0".to_string(),
                    description: String::new(),
                    author: String::new(),
                },
                compatibility: PluginCompatibility::default(),
                handles: PluginHandles {
                    blocks: vec![plugin_name.replace('.', "_")],
                    expressions: Vec::new(),
                },
                exports: PluginExports::default(),
                bridge: PluginBridge {
                    functions: vec![BridgeFunction {
                        name: bridge_name.to_string(),
                        params: vec!["string".to_string()],
                        returns: "string".to_string(),
                        module: "env".to_string(),
                        description: None,
                        expand_strings: true,
                        callback,
                        ..Default::default()
                    }],
                },
                language: PluginLanguage::default(),
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
    }

    #[test]
    fn test_v2_callback_validation_accepts_loaded_plugin_target() {
        use crate::plugins::plugin_abi::BridgeCallback;
        use v2_callback_helpers::manifest_with_callback;

        let frame_server = manifest_with_callback(
            "frame.server",
            "_ui_render_page",
            Some(BridgeCallback {
                purpose: "component_tag_render".to_string(),
                plugin_target: "frame.ui".to_string(),
                discovery: "exports_matching".to_string(),
                export_pattern: Some("{tagname}_render".to_string()),
                fallback: "passthrough".to_string(),
            }),
        );
        let frame_ui = manifest_with_callback("frame.ui", "_ui_event", None);

        let result = PluginRegistryBuilder::new()
            .with_validation_policy(crate::plugins::registry_loader::ValidationPolicy::Off)
            .add_manifest("frame.server".to_string(), frame_server)
            .add_manifest("frame.ui".to_string(), frame_ui)
            .build();

        assert!(result.is_ok(), "valid callback target must build cleanly");
    }

    #[test]
    fn test_v2_callback_validation_rejects_missing_plugin_target() {
        use crate::plugins::plugin_abi::BridgeCallback;
        use v2_callback_helpers::manifest_with_callback;

        let frame_server = manifest_with_callback(
            "frame.server",
            "_ui_render_page",
            Some(BridgeCallback {
                purpose: "component_tag_render".to_string(),
                plugin_target: "frame.does_not_exist".to_string(),
                discovery: "exports_matching".to_string(),
                export_pattern: Some("{tagname}_render".to_string()),
                fallback: "passthrough".to_string(),
            }),
        );

        let err = PluginRegistryBuilder::new()
            .add_manifest("frame.server".to_string(), frame_server)
            .build()
            .expect_err("missing plugin_target must fail to build");

        let msg = format!("{}", err);
        assert!(
            msg.contains("PLUGIN-CALLBACK-TARGET-MISSING"),
            "diagnostic must cite the error code; got: {}",
            msg
        );
        assert!(msg.contains("frame.does_not_exist"));
    }

    #[test]
    fn test_v2_callback_validation_rejects_conflict_across_plugins() {
        use crate::plugins::plugin_abi::BridgeCallback;
        use v2_callback_helpers::manifest_with_callback;

        let cb = BridgeCallback {
            purpose: "component_tag_render".to_string(),
            plugin_target: "frame.ui".to_string(),
            discovery: "exports_matching".to_string(),
            export_pattern: Some("{tagname}_render".to_string()),
            fallback: "passthrough".to_string(),
        };
        let plugin_a = manifest_with_callback("plugin.a", "_shared_bridge", Some(cb.clone()));
        let plugin_b = manifest_with_callback("plugin.b", "_shared_bridge", Some(cb));
        let frame_ui = manifest_with_callback("frame.ui", "_ui_event", None);

        let err = PluginRegistryBuilder::new()
            .add_manifest("plugin.a".to_string(), plugin_a)
            .add_manifest("plugin.b".to_string(), plugin_b)
            .add_manifest("frame.ui".to_string(), frame_ui)
            .build()
            .expect_err("conflicting (bridge, purpose) must fail to build");

        let msg = format!("{}", err);
        assert!(msg.contains("PLUGIN-CALLBACK-CONFLICT"));
        assert!(msg.contains("_shared_bridge"));
        assert!(msg.contains("component_tag_render"));
    }

    #[test]
    fn test_v2_callback_contracts_accessor_returns_resolved_set() {
        use crate::plugins::plugin_abi::BridgeCallback;
        use v2_callback_helpers::manifest_with_callback;

        let frame_server = manifest_with_callback(
            "frame.server",
            "_ui_render_page",
            Some(BridgeCallback {
                purpose: "component_tag_render".to_string(),
                plugin_target: "frame.ui".to_string(),
                discovery: "exports_matching".to_string(),
                export_pattern: Some("{tagname}_render".to_string()),
                fallback: "passthrough".to_string(),
            }),
        );
        let frame_ui = manifest_with_callback("frame.ui", "_ui_event", None);

        let registry = PluginRegistryBuilder::new()
            .with_validation_policy(crate::plugins::registry_loader::ValidationPolicy::Off)
            .add_manifest("frame.server".to_string(), frame_server)
            .add_manifest("frame.ui".to_string(), frame_ui)
            .build()
            .expect("build should succeed");

        let callbacks = registry.callback_contracts();
        assert_eq!(callbacks.len(), 1);
        assert_eq!(callbacks[0].bridge, "_ui_render_page");
        assert_eq!(callbacks[0].purpose, "component_tag_render");
        assert_eq!(callbacks[0].plugin_target, "frame.ui");
        assert_eq!(callbacks[0].declared_by_plugin, "frame.server");
        assert_eq!(callbacks[0].discovery, "exports_matching");
        assert_eq!(
            callbacks[0].export_pattern.as_deref(),
            Some("{tagname}_render")
        );
        assert_eq!(callbacks[0].fallback, "passthrough");
    }
}
