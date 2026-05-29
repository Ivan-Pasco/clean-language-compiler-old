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
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("Default registry should build")
    }
}

impl PluginRegistry {
    /// Create a new empty plugin registry
    #[deprecated(note = "Use PluginRegistry::builder() instead")]
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            expression_handlers: HashMap::new(),
            registered_plugins: Vec::new(),
            bridge_functions: Vec::new(),
            manifests: HashMap::new(),
            registrations: PluginRegistrations::default(),
            plugin_permissions: HashMap::new(),
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

    /// Emit preamble code from every registered plugin.
    ///
    /// Each unique WASM plugin is called with `expand_block("__preamble", "", "")`.
    /// Plugins that do not handle `"__preamble"` return empty output — this is safe.
    /// Results are returned in registration order; the caller is responsible for
    /// deduplicating functions before merging into the AST.
    pub fn expand_preambles(&self) -> Vec<super::PluginExpansion> {
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();
        for plugin in self.handlers.values() {
            if seen.insert(plugin.name().to_string()) {
                let preamble_block = crate::plugins::FrameworkBlock {
                    name: "__preamble".to_string(),
                    content: String::new(),
                    attributes: Vec::new(),
                    location: None,
                };
                if let Ok(expansion) = plugin.expand_full(&preamble_block) {
                    results.push(expansion);
                }
            }
        }
        results
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

    /// Get all loaded plugin manifests
    ///
    /// Returns the full manifests for enforcement rules, path detection, etc.
    pub fn loaded_manifests(&self) -> &HashMap<String, PluginManifest> {
        &self.manifests
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
            // Skip if already mapped in phase 1.
            for bf in &manifest.bridge.functions {
                if let Some(stripped) = bf.name.strip_prefix('_') {
                    if let Some(underscore_pos) = stripped.find('_') {
                        let namespace = &stripped[..underscore_pos];
                        let method = &stripped[underscore_pos + 1..];
                        let dot_name = format!("{}.{}", namespace, method);
                        // Don't override explicit mappings from phase 1
                        map.entry(dot_name).or_insert_with(|| bf.name.clone());
                    }
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
        }
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

        Ok(PluginRegistry {
            handlers,
            expression_handlers,
            registered_plugins,
            bridge_functions: self.bridge_functions,
            manifests: self.manifests,
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
                },
                BridgeFunction {
                    name: "_db_execute".to_string(),
                    params: vec!["string".to_string(), "string".to_string()],
                    returns: "integer".to_string(),
                    module: "env".to_string(),
                    description: Some("Execute INSERT/UPDATE/DELETE".to_string()),
                    expand_strings: true,
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
                    },
                    BridgeFunction {
                        name: "_db_execute".to_string(),
                        params: vec!["string".to_string(), "string".to_string()],
                        returns: "integer".to_string(),
                        module: "env".to_string(),
                        description: None,
                        expand_strings: true,
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
        };

        let registry = PluginRegistryBuilder::new()
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
        // No matching bridge — must NOT appear
        assert!(!map.contains_key("db.nonexistent"));
        // Total: 2 from phase 1 + 1 auto-derived from phase 2
        assert_eq!(map.len(), 3);
    }
}
