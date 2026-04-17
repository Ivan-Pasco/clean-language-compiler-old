/*!
 * Language Registry - Aggregates language definitions from all loaded plugins
 *
 * This module provides a unified interface for accessing language definitions
 * from multiple plugins. It's used by the language server to provide:
 * - Context-aware completions
 * - Hover documentation
 * - Keyword highlighting
 * - Path-based plugin activation
 */

use std::collections::HashMap;

use super::plugin_abi::{
    PluginCompletionDef, PluginFunctionDef, PluginKeyword, PluginManifest, PluginTypeDef,
};

/// Information about a plugin block for LSP
#[derive(Debug, Clone)]
pub struct BlockInfo {
    /// Block name (e.g., "data", "endpoints")
    pub name: String,
    /// Plugin that defines this block
    pub plugin_name: String,
    /// Description from the plugin
    pub description: Option<String>,
}

/// Keyword information with its source plugin
#[derive(Debug, Clone)]
pub struct KeywordInfo {
    /// The keyword
    pub name: String,
    /// Description/documentation
    pub description: String,
    /// Context where this keyword is valid
    pub context: String,
    /// Plugin that defines this keyword
    pub plugin_name: String,
}

/// Type information with its source plugin
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// The type name
    pub name: String,
    /// Description/documentation
    pub description: String,
    /// Plugin that defines this type
    pub plugin_name: String,
}

/// Function information with its source plugin
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// The function name
    pub name: String,
    /// Function signature
    pub signature: String,
    /// Description/documentation
    pub description: String,
    /// Plugin that defines this function
    pub plugin_name: String,
}

/// Completion snippet with its source plugin
#[derive(Debug, Clone)]
pub struct CompletionSnippet {
    /// Trigger text
    pub trigger: String,
    /// Text to insert (with snippet syntax)
    pub insert: String,
    /// Optional description
    pub description: Option<String>,
    /// Plugin that defines this completion
    pub plugin_name: String,
}

/// Aggregated language definitions from all loaded plugins
///
/// This registry provides a unified view of all language definitions
/// from loaded plugins, enabling the language server to provide
/// context-aware IDE features without running WASM plugins.
///
/// # Example
///
/// ```ignore
/// let discovery = PluginDiscovery::new();
/// let manifests = discovery.discover_all()?;
/// let registry = LanguageRegistry::from_manifests(&manifests);
///
/// // Get keywords for a specific context
/// let block_keywords = registry.keywords_for_context("block");
///
/// // Get completions matching a prefix
/// let completions = registry.get_completions("dat");
///
/// // Check which plugin owns a file path
/// if let Some(plugin) = registry.plugin_for_path("app/data/User.cln") {
///     println!("File is owned by: {}", plugin);
/// }
/// ```
pub struct LanguageRegistry {
    /// Block definitions by name
    blocks: HashMap<String, BlockInfo>,
    /// Keywords grouped by context
    keywords_by_context: HashMap<String, Vec<KeywordInfo>>,
    /// All keywords (for quick lookup)
    all_keywords: HashMap<String, KeywordInfo>,
    /// Type definitions by name
    types: HashMap<String, TypeInfo>,
    /// Function definitions by name
    functions: HashMap<String, FunctionInfo>,
    /// All completion snippets
    completions: Vec<CompletionSnippet>,
    /// Path ownership patterns: (glob_pattern, plugin_name)
    path_ownership: Vec<(String, String)>,
}

impl LanguageRegistry {
    /// Create a new empty language registry
    pub fn new() -> Self {
        Self {
            blocks: HashMap::new(),
            keywords_by_context: HashMap::new(),
            all_keywords: HashMap::new(),
            types: HashMap::new(),
            functions: HashMap::new(),
            completions: Vec::new(),
            path_ownership: Vec::new(),
        }
    }

    /// Build a language registry from a collection of plugin manifests
    ///
    /// # Arguments
    /// * `manifests` - Map of plugin name to manifest
    ///
    /// # Returns
    /// A populated LanguageRegistry
    pub fn from_manifests(manifests: &HashMap<String, PluginManifest>) -> Self {
        let mut registry = Self::new();

        for (name, manifest) in manifests {
            registry.add_manifest(name, manifest);
        }

        registry
    }

    /// Build from a vec of (name, manifest) pairs
    pub fn from_manifest_vec(manifests: &[(String, PluginManifest)]) -> Self {
        let mut registry = Self::new();

        for (name, manifest) in manifests {
            registry.add_manifest(name, manifest);
        }

        registry
    }

    /// Add a plugin manifest to the registry
    pub fn add_manifest(&mut self, plugin_name: &str, manifest: &PluginManifest) {
        let language = &manifest.language;

        // Add blocks
        for block_name in &language.blocks {
            self.blocks.insert(
                block_name.clone(),
                BlockInfo {
                    name: block_name.clone(),
                    plugin_name: plugin_name.to_string(),
                    description: Some(manifest.plugin.description.clone()),
                },
            );
        }

        // Also add handled blocks (even without explicit language.blocks)
        for block_name in &manifest.handles.blocks {
            if !self.blocks.contains_key(block_name) {
                self.blocks.insert(
                    block_name.clone(),
                    BlockInfo {
                        name: block_name.clone(),
                        plugin_name: plugin_name.to_string(),
                        description: Some(manifest.plugin.description.clone()),
                    },
                );
            }
        }

        // Add keywords
        for keyword in &language.keywords {
            self.add_keyword(plugin_name, keyword);
        }

        // Add types
        for type_def in &language.types {
            self.add_type(plugin_name, type_def);
        }

        // Add functions
        for func_def in &language.functions {
            self.add_function(plugin_name, func_def);
        }

        // Add completions
        for completion in &language.completions {
            self.add_completion(plugin_name, completion);
        }

        // Add path ownership
        for path_pattern in &language.owns_paths {
            self.path_ownership
                .push((path_pattern.clone(), plugin_name.to_string()));
        }
    }

    fn add_keyword(&mut self, plugin_name: &str, keyword: &PluginKeyword) {
        let info = KeywordInfo {
            name: keyword.name.clone(),
            description: keyword.description.clone(),
            context: keyword.context.clone(),
            plugin_name: plugin_name.to_string(),
        };

        // Add to context-specific map
        self.keywords_by_context
            .entry(keyword.context.clone())
            .or_default()
            .push(info.clone());

        // Also add to "any" context if not already there
        if keyword.context != "any" {
            self.keywords_by_context
                .entry("any".to_string())
                .or_default()
                .push(info.clone());
        }

        // Add to all keywords map
        self.all_keywords.insert(keyword.name.clone(), info);
    }

    fn add_type(&mut self, plugin_name: &str, type_def: &PluginTypeDef) {
        self.types.insert(
            type_def.name.clone(),
            TypeInfo {
                name: type_def.name.clone(),
                description: type_def.description.clone(),
                plugin_name: plugin_name.to_string(),
            },
        );
    }

    fn add_function(&mut self, plugin_name: &str, func_def: &PluginFunctionDef) {
        self.functions.insert(
            func_def.name.clone(),
            FunctionInfo {
                name: func_def.name.clone(),
                signature: func_def.signature.clone(),
                description: func_def.description.clone(),
                plugin_name: plugin_name.to_string(),
            },
        );
    }

    fn add_completion(&mut self, plugin_name: &str, completion: &PluginCompletionDef) {
        self.completions.push(CompletionSnippet {
            trigger: completion.trigger.clone(),
            insert: completion.insert.clone(),
            description: completion.description.clone(),
            plugin_name: plugin_name.to_string(),
        });
    }

    // ========================================================================
    // Query Methods
    // ========================================================================

    /// Get all keywords valid in a specific context
    ///
    /// # Arguments
    /// * `context` - The context to filter by ("expression", "block", "any", etc.)
    ///
    /// # Returns
    /// Keywords valid in the specified context, plus keywords with "any" context
    pub fn keywords_for_context(&self, context: &str) -> Vec<&KeywordInfo> {
        let mut result = Vec::new();

        // Get context-specific keywords
        if let Some(keywords) = self.keywords_by_context.get(context) {
            result.extend(keywords.iter());
        }

        // Also get "any" context keywords if not already getting them
        if context != "any" {
            if let Some(any_keywords) = self.keywords_by_context.get("any") {
                // Deduplicate
                for kw in any_keywords {
                    if !result.iter().any(|k| k.name == kw.name) {
                        result.push(kw);
                    }
                }
            }
        }

        result
    }

    /// Get information about a specific block
    pub fn get_block(&self, name: &str) -> Option<&BlockInfo> {
        self.blocks.get(name)
    }

    /// Check if a block name is recognized
    pub fn has_block(&self, name: &str) -> bool {
        self.blocks.contains_key(name)
    }

    /// Get information about a specific type
    pub fn get_type(&self, name: &str) -> Option<&TypeInfo> {
        self.types.get(name)
    }

    /// Get information about a specific function
    pub fn get_function(&self, name: &str) -> Option<&FunctionInfo> {
        self.functions.get(name)
    }

    /// Get information about a keyword
    pub fn get_keyword(&self, name: &str) -> Option<&KeywordInfo> {
        self.all_keywords.get(name)
    }

    /// Get completions matching a prefix
    ///
    /// # Arguments
    /// * `prefix` - The text prefix to match triggers against
    ///
    /// # Returns
    /// Completion snippets whose triggers start with the prefix
    pub fn get_completions(&self, prefix: &str) -> Vec<&CompletionSnippet> {
        self.completions
            .iter()
            .filter(|c| c.trigger.starts_with(prefix) || prefix.starts_with(&c.trigger))
            .collect()
    }

    /// Get all completions
    pub fn all_completions(&self) -> &[CompletionSnippet] {
        &self.completions
    }

    /// Find the plugin that "owns" a file path
    ///
    /// # Arguments
    /// * `path` - The file path to check
    ///
    /// # Returns
    /// The name of the plugin that owns this path, or None
    pub fn plugin_for_path(&self, path: &str) -> Option<&str> {
        for (pattern, plugin) in &self.path_ownership {
            // Simple prefix matching (could be extended to proper glob matching)
            if path.starts_with(pattern) || path.contains(pattern) {
                return Some(plugin.as_str());
            }
        }
        None
    }

    /// Get all keyword names (for syntax highlighting)
    pub fn all_keywords(&self) -> Vec<&str> {
        self.all_keywords.keys().map(|s| s.as_str()).collect()
    }

    /// Get all block names
    pub fn all_blocks(&self) -> Vec<&str> {
        self.blocks.keys().map(|s| s.as_str()).collect()
    }

    /// Get all type names
    pub fn all_types(&self) -> Vec<&str> {
        self.types.keys().map(|s| s.as_str()).collect()
    }

    /// Get all function names
    pub fn all_functions(&self) -> Vec<&str> {
        self.functions.keys().map(|s| s.as_str()).collect()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
            && self.all_keywords.is_empty()
            && self.types.is_empty()
            && self.functions.is_empty()
            && self.completions.is_empty()
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::plugin_abi::{
        PluginCompatibility, PluginExports, PluginHandles, PluginInfo, PluginLanguage,
    };

    fn create_test_manifest() -> PluginManifest {
        PluginManifest {
            plugin: PluginInfo {
                name: "test.plugin".to_string(),
                version: "1.0.0".to_string(),
                description: "Test plugin".to_string(),
                author: "Test".to_string(),
            },
            compatibility: PluginCompatibility::default(),
            handles: PluginHandles {
                blocks: vec!["testblock".to_string()],
            },
            exports: PluginExports::default(),
            bridge: Default::default(),
            language: PluginLanguage {
                blocks: vec!["testblock".to_string()],
                keywords: vec![
                    PluginKeyword {
                        name: "find".to_string(),
                        description: "Find records".to_string(),
                        context: "expression".to_string(),
                    },
                    PluginKeyword {
                        name: "where".to_string(),
                        description: "Filter condition".to_string(),
                        context: "block".to_string(),
                    },
                ],
                types: vec![PluginTypeDef {
                    name: "Model".to_string(),
                    description: "Data model type".to_string(),
                }],
                functions: vec![PluginFunctionDef {
                    name: "Data.tx".to_string(),
                    signature: "Data.tx: block -> Result".to_string(),
                    description: "Execute in transaction".to_string(),
                    maps_to: None,
                }],
                completions: vec![PluginCompletionDef {
                    trigger: "data ".to_string(),
                    insert: "data ${1:Name}:\n\t${2:field}: ${3:type}".to_string(),
                    description: Some("Create data model".to_string()),
                }],
                owns_paths: vec!["app/data/".to_string()],
            },
            ai: Default::default(),
            paths: Default::default(),
            enforcement: Default::default(),
            memory: Default::default(),
        }
    }

    #[test]
    fn test_build_from_manifest() {
        let manifest = create_test_manifest();
        let mut manifests = HashMap::new();
        manifests.insert("test.plugin".to_string(), manifest);

        let registry = LanguageRegistry::from_manifests(&manifests);

        assert!(!registry.is_empty());
        assert!(registry.has_block("testblock"));
    }

    #[test]
    fn test_keywords_by_context() {
        let manifest = create_test_manifest();
        let mut manifests = HashMap::new();
        manifests.insert("test.plugin".to_string(), manifest);

        let registry = LanguageRegistry::from_manifests(&manifests);

        // Get expression keywords
        let expr_keywords = registry.keywords_for_context("expression");
        assert!(expr_keywords.iter().any(|k| k.name == "find"));

        // Get block keywords
        let block_keywords = registry.keywords_for_context("block");
        assert!(block_keywords.iter().any(|k| k.name == "where"));
    }

    #[test]
    fn test_get_type() {
        let manifest = create_test_manifest();
        let mut manifests = HashMap::new();
        manifests.insert("test.plugin".to_string(), manifest);

        let registry = LanguageRegistry::from_manifests(&manifests);

        let model_type = registry.get_type("Model").unwrap();
        assert_eq!(model_type.name, "Model");
        assert!(model_type.description.contains("Data model"));
    }

    #[test]
    fn test_get_function() {
        let manifest = create_test_manifest();
        let mut manifests = HashMap::new();
        manifests.insert("test.plugin".to_string(), manifest);

        let registry = LanguageRegistry::from_manifests(&manifests);

        let func = registry.get_function("Data.tx").unwrap();
        assert_eq!(func.name, "Data.tx");
        assert!(func.signature.contains("block"));
    }

    #[test]
    fn test_completions() {
        let manifest = create_test_manifest();
        let mut manifests = HashMap::new();
        manifests.insert("test.plugin".to_string(), manifest);

        let registry = LanguageRegistry::from_manifests(&manifests);

        let completions = registry.get_completions("data");
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].trigger, "data ");
    }

    #[test]
    fn test_path_ownership() {
        let manifest = create_test_manifest();
        let mut manifests = HashMap::new();
        manifests.insert("test.plugin".to_string(), manifest);

        let registry = LanguageRegistry::from_manifests(&manifests);

        // File in owned path
        assert_eq!(
            registry.plugin_for_path("app/data/User.cln"),
            Some("test.plugin")
        );

        // File not in owned path
        assert_eq!(registry.plugin_for_path("app/other/File.cln"), None);
    }

    #[test]
    fn test_all_keywords() {
        let manifest = create_test_manifest();
        let mut manifests = HashMap::new();
        manifests.insert("test.plugin".to_string(), manifest);

        let registry = LanguageRegistry::from_manifests(&manifests);

        let keywords = registry.all_keywords();
        assert!(keywords.contains(&"find"));
        assert!(keywords.contains(&"where"));
    }
}
