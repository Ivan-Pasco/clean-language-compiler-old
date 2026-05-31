//! Multi-file Compiler
//!
//! Orchestrates the compilation of multi-file Clean Language programs.
//! This is the main entry point for building programs with imports.

use super::{CompilationModuleId, CompilationUnit, ModuleGraph};
use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::hir::hir_builder::HirBuilder;
use crate::hir::HirProgram;
use crate::lexer::specification_lexer::{SourceCode, SpecificationLexer};
use crate::parser::SpecificationParser;
use crate::plugins::{PluginExpander, PluginRegistry};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Derive a unique module name from a companion file path relative to the pages directory.
///
/// Examples:
/// - `app/pages/dashboard.cln` → `pages_dashboard`
/// - `app/pages/blog/[slug].cln` → `pages_blog_slug`
fn derive_companion_module_name(path: &Path, base_dir: &Path) -> String {
    let relative = path.strip_prefix(base_dir).unwrap_or(path);
    relative
        .to_str()
        .unwrap_or("")
        .trim_end_matches(".cln")
        .replace('/', "_")
        .replace(['[', ']'], "")
}

/// Prefix companion functions with the module name to avoid conflicts.
///
/// Transforms `any guard()` → `any {module}_guard()` and `any load()` → `any {module}_load()`.
fn prefix_companion_functions(source: &str, module_name: &str) -> String {
    source
        .replace("any guard(", &format!("any {}_guard(", module_name))
        .replace("any load(", &format!("any {}_load(", module_name))
        .replace("guard()", &format!("{}_guard()", module_name))
        .replace("load()", &format!("{}_load()", module_name))
}

/// Represents an extracted import with additional metadata
#[derive(Debug, Clone)]
struct ExtractedImport {
    /// Module name or file path
    name: String,
    /// True if this is a file path import
    is_file_import: bool,
}

/// Metadata extracted from a project manifest (main.cln with package: block)
#[derive(Debug, Default)]
struct ManifestInfo {
    /// The declared entry file for the first target (e.g. app/web/pages/home.cln)
    entry_path: Option<PathBuf>,
    /// Folders declared in shared: [...] that should be compiled for every target
    shared_folders: Vec<PathBuf>,
}

/// Configuration for the multi-file compiler
#[derive(Clone)]
pub struct MultiFileCompilerConfig {
    /// Search paths for finding imported modules
    pub search_paths: Vec<PathBuf>,

    /// Optimization level (0-3)
    pub opt_level: u8,

    /// Whether to include debug information
    pub debug: bool,

    /// Enable HTML-first page processing (Frame UI)
    pub html_pages_enabled: bool,

    /// Directory containing HTML pages (relative to project root)
    pub html_pages_dir: Option<PathBuf>,

    /// Directory containing Clean components for HTML pages
    pub html_components_dir: Option<PathBuf>,

    /// Plugin registry for framework block expansion
    pub plugin_registry: Option<Arc<PluginRegistry>>,

    /// Release mode: strip `always:` invariant checks from the compiled output.
    /// When true, class `always:` block conditions are not injected before
    /// method returns, producing smaller and faster WASM.
    pub release_mode: bool,
}

impl std::fmt::Debug for MultiFileCompilerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiFileCompilerConfig")
            .field("search_paths", &self.search_paths)
            .field("opt_level", &self.opt_level)
            .field("debug", &self.debug)
            .field("html_pages_enabled", &self.html_pages_enabled)
            .field("html_pages_dir", &self.html_pages_dir)
            .field("html_components_dir", &self.html_components_dir)
            .field(
                "plugin_registry",
                &self.plugin_registry.as_ref().map(|_| "..."),
            )
            .field("release_mode", &self.release_mode)
            .finish()
    }
}

impl Default for MultiFileCompilerConfig {
    fn default() -> Self {
        Self {
            search_paths: vec![
                PathBuf::from("./"),
                PathBuf::from("./lib/"),
                PathBuf::from("./modules/"),
                PathBuf::from("./src/"),
            ],
            opt_level: 2,
            debug: false,
            html_pages_enabled: false,
            html_pages_dir: None,
            html_components_dir: None,
            plugin_registry: None,
            release_mode: false,
        }
    }
}

impl MultiFileCompilerConfig {
    /// Create a new config with custom search paths
    pub fn with_search_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.search_paths = paths;
        self
    }

    /// Add a search path
    pub fn add_search_path<P: AsRef<Path>>(&mut self, path: P) {
        self.search_paths.push(path.as_ref().to_path_buf());
    }

    /// Set the optimization level
    pub fn with_opt_level(mut self, level: u8) -> Self {
        self.opt_level = level;
        self
    }

    /// Enable debug mode
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Enable HTML-first page processing
    pub fn with_html_pages(mut self, enabled: bool) -> Self {
        self.html_pages_enabled = enabled;
        self
    }

    /// Set the HTML pages directory
    pub fn with_html_pages_dir<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.html_pages_dir = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set the HTML components directory
    pub fn with_html_components_dir<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.html_components_dir = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set the plugin registry for framework block expansion
    pub fn with_plugin_registry(mut self, registry: Arc<PluginRegistry>) -> Self {
        self.plugin_registry = Some(registry);
        self
    }

    /// Enable release mode (strips `always:` invariant checks).
    pub fn with_release_mode(mut self, release: bool) -> Self {
        self.release_mode = release;
        self
    }
}

/// Represents an HTML page discovered during compilation
///
/// Pages are `.html` files optionally paired with a companion `.cln` file
/// that provides server-side logic (guard, load functions).
#[derive(Debug, Clone)]
pub struct HtmlPage {
    /// File path to the HTML page (.html file)
    pub file_path: PathBuf,
    /// Route path derived from file path (e.g., "/blog/[slug].html" -> "/blog/:slug")
    pub route_path: String,
    /// Raw HTML content
    pub html_content: String,
    /// Page metadata (title, layout, etc.)
    pub metadata: HtmlPageMetadata,
    /// Optional companion .cln file with guard/load functions
    pub companion: Option<CompanionFile>,
}

/// Companion .cln file paired with an HTML page
///
/// Provides server-side logic: guard() for auth checks and load() for data fetching.
#[derive(Debug, Clone)]
pub struct CompanionFile {
    /// Path to the companion .cln file
    pub file_path: PathBuf,
    /// Raw source code of the companion file
    pub source: String,
    /// Whether the companion defines a guard() function
    pub has_guard: bool,
    /// Whether the companion defines a load() function
    pub has_load: bool,
    /// Unique module name derived from path (e.g., "pages_dashboard")
    pub module_name: String,
}

/// Metadata extracted from HTML page
#[derive(Debug, Clone, Default)]
pub struct HtmlPageMetadata {
    /// Page title from <title> or page attribute
    pub title: Option<String>,
    /// Layout to use (default: "main")
    pub layout: Option<String>,
    /// Data block content if present
    pub data_block: Option<String>,
    /// Custom tags used in the page
    pub custom_tags: Vec<String>,
}

/// Component registry mapping custom tags to Clean classes
#[derive(Debug, Clone, Default)]
pub struct ComponentRegistry {
    /// Map from tag name to component info
    pub components: HashMap<String, ComponentInfo>,
}

/// Information about a registered component
#[derive(Debug, Clone)]
pub struct ComponentInfo {
    /// Class name in Clean code
    pub class_name: String,
    /// Source file path
    pub file_path: PathBuf,
    /// Props/attributes the component accepts
    pub props: Vec<String>,
    /// Whether the component supports client hydration
    pub has_client_hydration: bool,
}

impl ComponentRegistry {
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
        }
    }

    /// Register a component
    pub fn register(&mut self, tag_name: String, info: ComponentInfo) {
        self.components.insert(tag_name, info);
    }

    /// Check if a tag is a registered component
    pub fn has_component(&self, tag_name: &str) -> bool {
        self.components.contains_key(tag_name)
    }

    /// Get component info by tag name
    pub fn get(&self, tag_name: &str) -> Option<&ComponentInfo> {
        self.components.get(tag_name)
    }

    /// Convert to JSON for plugin consumption
    pub fn to_json(&self) -> String {
        let mut json = String::from("{");
        let mut first = true;
        for (tag, info) in &self.components {
            if !first {
                json.push(',');
            }
            first = false;
            json.push_str(&format!(
                "\"{}\":{{\"class\":\"{}\",\"props\":[{}]}}",
                tag,
                info.class_name,
                info.props
                    .iter()
                    .map(|p| format!("\"{}\"", p))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        json.push('}');
        json
    }
}

/// Multi-file compiler for Clean Language programs
pub struct MultiFileCompiler {
    config: MultiFileCompilerConfig,
}

impl MultiFileCompiler {
    /// Create a new multi-file compiler with default configuration
    pub fn new() -> Self {
        Self {
            config: MultiFileCompilerConfig::default(),
        }
    }

    /// Create a new multi-file compiler with custom configuration
    pub fn with_config(config: MultiFileCompilerConfig) -> Self {
        Self { config }
    }

    /// Build a program from an entry file
    ///
    /// This is the main entry point for multi-file compilation:
    /// 1. Reads the entry file
    /// 2. Discovers all imported modules
    /// 3. Builds the dependency graph
    /// 4. Parses all modules to HIR
    /// 5. Returns a CompilationUnit ready for resolution and codegen
    pub fn build_from_file<P: AsRef<Path>>(
        &self,
        entry_path: P,
    ) -> Result<CompilationUnit, Vec<CompilerError>> {
        let entry_path = entry_path.as_ref();

        // Validate entry file exists
        if !entry_path.exists() {
            return Err(vec![CompilerError::io_error(
                format!("Entry file not found: {}", entry_path.display()),
                None,
                None,
            )]);
        }

        // Read entry file
        let manifest_source = fs::read_to_string(entry_path).map_err(|e| {
            vec![CompilerError::io_error(
                format!("Failed to read entry file {}: {}", entry_path.display(), e),
                None,
                None,
            )]
        })?;

        // Get the canonical path for the entry file
        let manifest_canonical = entry_path
            .canonicalize()
            .unwrap_or_else(|_| entry_path.to_path_buf());

        // If this is a project manifest (package: block), extract entry: and shared: info
        // and redirect to the declared entry file.
        let manifest_root: Option<PathBuf> = if Self::is_manifest_file(&manifest_source) {
            Some(
                manifest_canonical
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_path_buf(),
            )
        } else {
            None
        };

        let (manifest_info, entry_source, canonical_path) =
            if let Some(ref manifest_dir) = manifest_root {
                let info = Self::parse_manifest_info(&manifest_source, manifest_dir);

                // Resolve the actual entry file if declared
                if let Some(ref ep) = info.entry_path {
                    match fs::read_to_string(ep) {
                        Ok(src) => {
                            let ep_canonical = ep.canonicalize().unwrap_or_else(|_| ep.clone());
                            (Some(info), src, ep_canonical)
                        }
                        Err(_) => {
                            // Entry file not found — fall back to compiling the manifest itself
                            (Some(info), manifest_source, manifest_canonical)
                        }
                    }
                } else {
                    (Some(info), manifest_source, manifest_canonical.clone())
                }
            } else {
                (None, manifest_source, manifest_canonical.clone())
            };

        // Derive module name from file path
        let entry_name = Self::derive_module_name(&canonical_path);

        // Create compilation unit
        let mut unit = CompilationUnit::new(entry_name, canonical_path.clone(), entry_source);

        // Build the module graph by discovering all imports
        let mut graph = ModuleGraph::new();
        graph.add_module(unit.entry_module);

        // Add the entry file's directory to search paths.
        // When a manifest is present, also add the manifest's directory (project root)
        // so that imports like `import "app/logic/utils"` resolve from the root.
        let mut search_paths = self.config.search_paths.clone();
        if let Some(parent) = canonical_path.parent() {
            search_paths.insert(0, parent.to_path_buf());
        }
        if let (Some(ref info), Some(ref manifest_dir)) = (&manifest_info, &manifest_root) {
            // Add project root (manifest dir) so all app/ paths resolve
            if !search_paths.contains(manifest_dir) {
                search_paths.push(manifest_dir.clone());
            }
            // Also add each shared folder itself as a search root for bare-name imports
            for shared in &info.shared_folders {
                if !search_paths.contains(shared) {
                    search_paths.push(shared.clone());
                }
            }
        }

        // Discover and load all modules via explicit import: statements
        self.discover_modules(&mut unit, &mut graph, &search_paths)?;

        // If a manifest declared shared: folders, scan them and add any .cln files
        // that were not already discovered via imports.  These files compile as part of
        // every target even when no other module explicitly imports them.
        if let Some(ref info) = manifest_info {
            for shared_dir in &info.shared_folders {
                for file_path in Self::collect_cln_files(shared_dir) {
                    if unit.module_id_for_path(&file_path).is_none() {
                        if let Ok(source) = fs::read_to_string(&file_path) {
                            let name = Self::derive_module_name(&file_path);
                            let id = unit.add_module(name, file_path, source);
                            // Add as a standalone node; the topological sort includes all nodes
                            graph.add_module(id);
                        }
                    }
                }
            }
        }

        // Determine compilation order
        let order = graph
            .topological_sort(unit.entry_module)
            .map_err(|e| vec![e])?;
        unit.set_compilation_order(order);

        // Parse all modules to HIR
        self.parse_all_modules(&mut unit)?;

        tracing::info!(
            modules = unit.module_count(),
            "Multi-file discovery complete"
        );

        Ok(unit)
    }

    /// Build a program from source code (for testing)
    ///
    /// Takes a map of module name -> source code
    pub fn build_from_sources(
        &self,
        entry_name: &str,
        sources: &[(String, String)],
    ) -> Result<CompilationUnit, Vec<CompilerError>> {
        // Create a unique temporary directory for this call to avoid race conditions
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("clean_compiler_test_{}", unique_id));

        // Clean up any existing directory and create fresh
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).map_err(|e| {
            vec![CompilerError::io_error(
                format!("Failed to create temp directory: {}", e),
                None,
                None,
            )]
        })?;

        // Write all sources to temp files
        let mut entry_path = None;
        for (name, source) in sources {
            let file_path = temp_dir.join(format!("{}.cln", name));
            fs::write(&file_path, source).map_err(|e| {
                vec![CompilerError::io_error(
                    format!("Failed to write temp file: {}", e),
                    None,
                    None,
                )]
            })?;
            if name == entry_name {
                entry_path = Some(file_path);
            }
        }

        let entry_path = entry_path.ok_or_else(|| {
            vec![CompilerError::codegen_error(
                format!("Entry module '{}' not found in sources", entry_name),
                None,
                None,
            )]
        })?;

        // Create config with temp dir in search path
        let config = MultiFileCompilerConfig {
            search_paths: vec![temp_dir.clone()],
            opt_level: self.config.opt_level,
            debug: self.config.debug,
            html_pages_enabled: self.config.html_pages_enabled,
            html_pages_dir: self.config.html_pages_dir.clone(),
            html_components_dir: self.config.html_components_dir.clone(),
            plugin_registry: self.config.plugin_registry.clone(),
            release_mode: self.config.release_mode,
        };

        let compiler = MultiFileCompiler::with_config(config);
        let result = compiler.build_from_file(entry_path);

        // Clean up temp directory after use
        let _ = fs::remove_dir_all(&temp_dir);

        result
    }

    // =========================================================================
    // HTML-First Page Processing Methods
    // =========================================================================

    /// Discover HTML pages in the pages directory
    pub fn discover_html_pages<P: AsRef<Path>>(
        &self,
        pages_dir: P,
    ) -> Result<Vec<HtmlPage>, Vec<CompilerError>> {
        let pages_dir = pages_dir.as_ref();
        let mut pages = Vec::new();
        let mut errors = Vec::new();

        if !pages_dir.exists() {
            return Ok(pages); // No pages directory, return empty
        }

        // Recursively scan for HTML files
        match self.scan_html_files(pages_dir, pages_dir) {
            Ok(found_pages) => pages = found_pages,
            Err(e) => errors.push(e),
        }

        if errors.is_empty() {
            Ok(pages)
        } else {
            Err(errors)
        }
    }

    /// Recursively scan directory for HTML files
    fn scan_html_files(&self, dir: &Path, base_dir: &Path) -> Result<Vec<HtmlPage>, CompilerError> {
        let mut pages = Vec::new();

        let entries = fs::read_dir(dir).map_err(|e| {
            CompilerError::io_error(
                format!("Failed to read directory {}: {}", dir.display(), e),
                None,
                None,
            )
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                CompilerError::io_error(format!("Failed to read entry: {}", e), None, None)
            })?;

            let path = entry.path();

            if path.is_dir() {
                // Recurse into subdirectories
                let mut sub_pages = self.scan_html_files(&path, base_dir)?;
                pages.append(&mut sub_pages);
            } else if self.is_html_page_file(&path) {
                // Process .html file — check for companion .cln file
                let companion = self.find_companion_file(&path, base_dir);
                match self.process_html_file(&path, base_dir, companion) {
                    Ok(page) => pages.push(page),
                    Err(e) => {
                        tracing::warn!("Failed to process HTML page {}: {:?}", path.display(), e);
                    }
                }
            }
        }

        Ok(pages)
    }

    /// Check if a file is an HTML page (.html extension)
    fn is_html_page_file(&self, path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "html")
    }

    /// Look for a companion .cln file for the given .html file
    fn find_companion_file(&self, html_path: &Path, base_dir: &Path) -> Option<CompanionFile> {
        let companion_path = html_path.with_extension("cln");
        if !companion_path.exists() {
            return None;
        }

        let source = match fs::read_to_string(&companion_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "Failed to read companion file {}: {}",
                    companion_path.display(),
                    e
                );
                return None;
            }
        };

        let has_guard = source.contains("any guard(") || source.contains("guard()");
        let has_load = source.contains("any load(") || source.contains("load()");
        let module_name = derive_companion_module_name(&companion_path, base_dir);

        Some(CompanionFile {
            file_path: companion_path,
            source,
            has_guard,
            has_load,
            module_name,
        })
    }

    /// Process a single .html file into an HtmlPage
    fn process_html_file(
        &self,
        file_path: &Path,
        base_dir: &Path,
        companion: Option<CompanionFile>,
    ) -> Result<HtmlPage, CompilerError> {
        let html_content = fs::read_to_string(file_path).map_err(|e| {
            CompilerError::io_error(
                format!("Failed to read HTML file {}: {}", file_path.display(), e),
                None,
                None,
            )
        })?;

        // Derive route path from file path
        let route_path = self.file_path_to_route(file_path, base_dir);

        // Extract metadata from HTML content
        let metadata = self.extract_html_metadata(&html_content);

        Ok(HtmlPage {
            file_path: file_path.to_path_buf(),
            route_path,
            html_content,
            metadata,
            companion,
        })
    }

    /// Convert file path to route path
    /// e.g., /pages/blog/[slug].html -> /blog/:slug
    fn file_path_to_route(&self, file_path: &Path, base_dir: &Path) -> String {
        let relative = file_path.strip_prefix(base_dir).unwrap_or(file_path);

        let mut route = String::from("/");

        for component in relative.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();

                // Skip index files at the route level
                if name_str == "index.html" {
                    continue;
                }

                // Remove .html extension
                let name_str = name_str.trim_end_matches(".html");

                // Convert [param] to :param
                let route_segment = if name_str.starts_with('[') && name_str.ends_with(']') {
                    format!(":{}", &name_str[1..name_str.len() - 1])
                } else {
                    name_str.to_string()
                };

                if !route.ends_with('/') {
                    route.push('/');
                }
                route.push_str(&route_segment);
            }
        }

        // Normalize empty route to "/"
        if route.is_empty() || route == "/" {
            "/".to_string()
        } else {
            route
        }
    }

    /// Extract metadata from HTML content
    fn extract_html_metadata(&self, html: &str) -> HtmlPageMetadata {
        let mut metadata = HtmlPageMetadata::default();

        // Extract title from <title> tag
        if let Some(start) = html.find("<title>") {
            if let Some(end) = html[start..].find("</title>") {
                let title_start = start + 7; // "<title>".len()
                let title_end = start + end;
                metadata.title = Some(html[title_start..title_end].trim().to_string());
            }
        }

        // Extract page attributes from <page> tag if present
        if let Some(start) = html.find("<page") {
            if let Some(end) = html[start..].find('>') {
                let tag_content = &html[start..start + end + 1];

                // Extract layout attribute
                if let Some(layout) = self.extract_attr_value(tag_content, "layout") {
                    metadata.layout = Some(layout);
                }

                // Extract title attribute (overrides <title> tag)
                if let Some(title) = self.extract_attr_value(tag_content, "title") {
                    metadata.title = Some(title);
                }
            }
        }

        // Extract <data> block if present
        if let Some(start) = html.find("<data>") {
            if let Some(end) = html.find("</data>") {
                let data_start = start + 6; // "<data>".len()
                metadata.data_block = Some(html[data_start..end].trim().to_string());
            }
        }

        // Find custom tags (tags with hyphens, following Web Components convention)
        let mut custom_tags = HashSet::new();
        let mut pos = 0;
        while let Some(tag_start) = html[pos..].find('<') {
            let tag_start = pos + tag_start;
            if tag_start + 1 >= html.len() {
                break;
            }

            // Skip closing tags, comments, doctype
            let next_char = html.chars().nth(tag_start + 1).unwrap_or(' ');
            if next_char == '/' || next_char == '!' || next_char == '?' {
                pos = tag_start + 1;
                continue;
            }

            // Find end of tag name
            let tag_name_end = html[tag_start + 1..]
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .map(|i| tag_start + 1 + i)
                .unwrap_or(html.len());

            let tag_name = &html[tag_start + 1..tag_name_end];

            // Check if it's a custom tag (contains hyphen)
            if tag_name.contains('-') && !tag_name.is_empty() {
                custom_tags.insert(tag_name.to_lowercase());
            }

            pos = tag_name_end;
        }

        metadata.custom_tags = custom_tags.into_iter().collect();

        metadata
    }

    /// Extract attribute value from an HTML tag string
    fn extract_attr_value(&self, tag: &str, attr_name: &str) -> Option<String> {
        let patterns = [format!("{}=\"", attr_name), format!("{}='", attr_name)];

        for pattern in &patterns {
            if let Some(start) = tag.find(pattern) {
                let value_start = start + pattern.len();
                let quote_char = tag.chars().nth(start + attr_name.len() + 1)?;
                if let Some(end) = tag[value_start..].find(quote_char) {
                    return Some(tag[value_start..value_start + end].to_string());
                }
            }
        }

        None
    }

    /// Build component registry from Clean component files
    pub fn build_component_registry<P: AsRef<Path>>(
        &self,
        components_dir: P,
    ) -> Result<ComponentRegistry, Vec<CompilerError>> {
        let components_dir = components_dir.as_ref();
        let mut registry = ComponentRegistry::new();
        let mut errors = Vec::new();

        if !components_dir.exists() {
            return Ok(registry); // No components directory
        }

        // Scan for .cln files
        let entries = fs::read_dir(components_dir).map_err(|e| {
            vec![CompilerError::io_error(
                format!("Failed to read components directory: {}", e),
                None,
                None,
            )]
        })?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(CompilerError::io_error(
                        format!("Failed to read entry: {}", e),
                        None,
                        None,
                    ));
                    continue;
                }
            };

            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "cln") {
                match self.extract_component_info(&path) {
                    Ok(Some(info)) => {
                        // Derive tag name from class name (PascalCase -> kebab-case)
                        let tag_name = self.class_name_to_tag(&info.class_name);
                        registry.register(tag_name, info);
                    }
                    Ok(None) => {
                        // File doesn't define a component
                    }
                    Err(e) => {
                        errors.push(e);
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(registry)
        } else {
            Err(errors)
        }
    }

    /// Extract component information from a Clean file
    fn extract_component_info(
        &self,
        file_path: &Path,
    ) -> Result<Option<ComponentInfo>, CompilerError> {
        let source = fs::read_to_string(file_path).map_err(|e| {
            CompilerError::io_error(
                format!(
                    "Failed to read component file {}: {}",
                    file_path.display(),
                    e
                ),
                None,
                None,
            )
        })?;

        // Look for class definition with _tag property (indicates a component)
        // Pattern: class ClassName
        let class_start = match source.find("class ") {
            Some(pos) => pos,
            None => return Ok(None),
        };

        // Extract class name
        let after_class = &source[class_start + 6..];
        let class_name_end = after_class
            .find(|c: char| c.is_whitespace() || c == ':' || c == '\n')
            .unwrap_or(after_class.len());
        let class_name = after_class[..class_name_end].trim().to_string();

        if class_name.is_empty() {
            return Ok(None);
        }

        // Check if it has _tag property (indicates UI component)
        let has_tag = source.contains("_tag =") || source.contains("_tag=");

        // Check for client hydration support
        let has_client_hydration =
            source.contains("_hydration_mode") && !source.contains("_hydration_mode = \"off\"");

        // Extract props (properties that don't start with _)
        let mut props = Vec::new();
        for line in source.lines() {
            let trimmed = line.trim();
            // Match property declarations: type name = value
            if !trimmed.starts_with("//") && !trimmed.starts_with("_") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    let first = parts[0];
                    // Check if it's a type declaration
                    if matches!(first, "string" | "integer" | "number" | "boolean" | "any") {
                        let prop_name = parts[1].trim_end_matches('=').trim();
                        if !prop_name.starts_with('_') && !prop_name.is_empty() {
                            props.push(prop_name.to_string());
                        }
                    }
                }
            }
        }

        // Only return if it looks like a UI component
        if has_tag {
            Ok(Some(ComponentInfo {
                class_name,
                file_path: file_path.to_path_buf(),
                props,
                has_client_hydration,
            }))
        } else {
            Ok(None)
        }
    }

    /// Convert PascalCase class name to kebab-case tag name
    fn class_name_to_tag(&self, class_name: &str) -> String {
        let mut result = String::new();
        for (i, c) in class_name.chars().enumerate() {
            if c.is_uppercase() {
                if i > 0 {
                    result.push('-');
                }
                result.push(c.to_ascii_lowercase());
            } else {
                result.push(c);
            }
        }
        result
    }

    /// Generate Clean code from HTML page using plugin
    /// This method would call the frame.ui plugin's process_html function
    pub fn generate_clean_from_html(
        &self,
        page: &HtmlPage,
        registry: &ComponentRegistry,
    ) -> Result<String, CompilerError> {
        // Generate the class name from route path
        let class_name = self.route_to_class_name(&page.route_path);

        // Generate Clean code for the page
        let mut code = String::new();

        // Note: No import statements - generated code is self-contained

        // Generate page class
        code.push_str(&format!("class {}\n", class_name));
        code.push_str(&format!("\tstring _path = \"{}\"\n", page.route_path));

        if let Some(layout) = &page.metadata.layout {
            code.push_str(&format!("\tstring _layout = \"{}\"\n", layout));
        }

        code.push_str("\n\tfunctions:\n");
        code.push_str("\t\tstring render()\n");

        // Convert HTML to render method body
        let render_body = self.html_to_render_body(&page.html_content, registry);
        code.push_str(&render_body);

        // If companion file exists, include prefixed functions after the class
        if let Some(ref companion) = page.companion {
            code.push_str("\n\n");
            let prefixed_source =
                prefix_companion_functions(&companion.source, &companion.module_name);
            code.push_str(&prefixed_source);
        }

        Ok(code)
    }

    /// Convert route path to class name
    fn route_to_class_name(&self, route: &str) -> String {
        let mut name = String::new();
        let mut capitalize_next = true;

        for c in route.chars() {
            if c == '/' || c == '-' || c == '_' {
                capitalize_next = true;
            } else if c == ':' {
                // Parameter - skip the colon, capitalize the param name
                capitalize_next = true;
            } else if capitalize_next {
                name.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                name.push(c);
            }
        }

        if name.is_empty() {
            "IndexPage".to_string()
        } else {
            format!("{}Page", name)
        }
    }

    /// Convert HTML content to render method body
    fn html_to_render_body(&self, html: &str, _registry: &ComponentRegistry) -> String {
        let mut body = String::new();
        body.push_str("\t\t\tstring html = \"\"\n");

        // Simple HTML to string concatenation conversion
        // Strip <page>, <data>, and doctype/html/head/body if present
        let content = self.extract_page_body(html);

        // Escape and add as string literals
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Escape quotes
            let escaped = trimmed.replace('\\', "\\\\").replace('"', "\\\"");
            body.push_str(&format!("\t\t\thtml = html + \"{}\"\n", escaped));
        }

        body.push_str("\t\t\treturn html\n");
        body
    }

    /// Extract just the body content from HTML
    fn extract_page_body(&self, html: &str) -> String {
        // Try to find <body> content
        if let Some(body_start) = html.find("<body") {
            if let Some(body_tag_end) = html[body_start..].find('>') {
                let content_start = body_start + body_tag_end + 1;
                if let Some(body_end) = html.find("</body>") {
                    return html[content_start..body_end].to_string();
                }
            }
        }

        // If no body tag, look for main content
        if let Some(main_start) = html.find("<main") {
            if let Some(main_tag_end) = html[main_start..].find('>') {
                let content_start = main_start + main_tag_end + 1;
                if let Some(main_end) = html.find("</main>") {
                    return html[content_start..main_end].to_string();
                }
            }
        }

        // Return as-is, stripping page/data blocks
        let mut result = html.to_string();

        // Remove <page ...> tags
        while let Some(start) = result.find("<page") {
            if let Some(end) = result[start..].find('>') {
                result = format!("{}{}", &result[..start], &result[start + end + 1..]);
            } else {
                break;
            }
        }

        // Remove <data>...</data> blocks
        while let Some(start) = result.find("<data>") {
            if let Some(end) = result.find("</data>") {
                result = format!("{}{}", &result[..start], &result[end + 7..]);
            } else {
                break;
            }
        }

        result
    }

    /// Process HTML pages and generate combined Clean source
    pub fn process_html_pages_to_clean(
        &self,
        pages: &[HtmlPage],
        registry: &ComponentRegistry,
    ) -> Result<String, Vec<CompilerError>> {
        let mut combined = String::new();
        let mut errors = Vec::new();

        // Note: We don't add import statements here since frame.web is a plugin
        // that gets expanded at compile time, not a module that gets imported.
        // The generated code is self-contained.

        // Generate page classes
        for page in pages {
            match self.generate_clean_from_html(page, registry) {
                Ok(code) => {
                    // Skip any import section
                    let code_without_import = code
                        .lines()
                        .skip_while(|line| {
                            line.starts_with("import")
                                || (line.starts_with('\t')
                                    && !line.contains("class")
                                    && !line.contains("functions:"))
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    combined.push_str(&code_without_import);
                    combined.push_str("\n\n");
                }
                Err(e) => errors.push(e),
            }
        }

        // Generate route handlers
        combined.push_str("// Route handlers\n");
        combined.push_str("functions:\n");

        for (i, page) in pages.iter().enumerate() {
            let class_name = self.route_to_class_name(&page.route_path);
            combined.push_str(&format!("\tstring __route_handler_{}()\n", i));

            // If companion has guard, call it before rendering
            if let Some(ref companion) = page.companion {
                if companion.has_guard {
                    combined.push_str(&format!(
                        "\t\tany guard_result = {}_guard()\n",
                        companion.module_name
                    ));
                    combined.push_str("\t\tif guard_result != null\n");
                    combined.push_str("\t\t\treturn \"\"\n");
                }
                if companion.has_load {
                    combined.push_str(&format!(
                        "\t\tany data = {}_load()\n",
                        companion.module_name
                    ));
                }
            }

            combined.push_str(&format!("\t\t{} page = {}()\n", class_name, class_name));
            combined.push_str("\t\treturn page.render()\n\n");
        }

        // Generate start: block with route registration
        combined.push_str("start:\n");
        combined.push_str("\tprintln(\"Starting HTML-first server...\")\n");
        combined.push_str("\tinteger status = 0\n");

        for (i, page) in pages.iter().enumerate() {
            combined.push_str(&format!(
                "\tstatus = _http_route(\"GET\", \"{}\", {})\n",
                page.route_path, i
            ));
        }

        combined.push_str("\tprintln(\"Routes registered\")\n");

        if errors.is_empty() {
            Ok(combined)
        } else {
            Err(errors)
        }
    }

    // =========================================================================
    // Project Manifest Support
    // =========================================================================

    /// Returns true if the source file is a project manifest (starts with "package:")
    fn is_manifest_file(source: &str) -> bool {
        source.trim_start().starts_with("package:")
    }

    /// Extract entry: and shared: declarations from a manifest file.
    ///
    /// This is a line-level parser — it does not tokenize the full file. It only
    /// extracts the two fields needed to drive multi-file discovery.
    fn parse_manifest_info(source: &str, manifest_dir: &Path) -> ManifestInfo {
        let mut info = ManifestInfo::default();
        let mut has_web_target = false;

        for line in source.lines() {
            let trimmed = line.trim();

            // Detect `target: web` so we can auto-add the web root as a shared folder
            if trimmed == "target: web" || trimmed == "target:web" {
                has_web_target = true;
            }

            // shared: [path1, path2, ...]
            if let Some(rest) = trimmed.strip_prefix("shared:") {
                let rest = rest.trim();
                if rest.starts_with('[') {
                    let end = rest.find(']').unwrap_or(rest.len());
                    let inner = &rest[1..end];
                    for path_str in inner.split(',') {
                        let p = path_str.trim().trim_matches('"');
                        if !p.is_empty() {
                            info.shared_folders.push(manifest_dir.join(p));
                        }
                    }
                }
            }

            // entry: path  (first occurrence wins)
            if info.entry_path.is_none() {
                if let Some(rest) = trimmed.strip_prefix("entry:") {
                    let p = rest.trim().trim_matches('"');
                    if !p.is_empty() {
                        info.entry_path = Some(manifest_dir.join(p));
                    }
                }
            }
        }

        // When `target: web` is declared, auto-add the web root as a shared folder
        // so companion files like routes.cln are discovered without requiring an
        // explicit shared: declaration.  Convention: entry lives in <web_root>/pages/,
        // so routes.cln and other companions sit at <web_root>/ (one level above pages/).
        if has_web_target {
            if let Some(ref ep) = info.entry_path {
                if let Some(pages_dir) = ep.parent() {
                    if pages_dir.file_name().and_then(|n| n.to_str()) == Some("pages") {
                        if let Some(web_root) = pages_dir.parent() {
                            let web_root = web_root.to_path_buf();
                            if web_root.exists() && !info.shared_folders.contains(&web_root) {
                                info.shared_folders.push(web_root);
                            }
                        }
                    }
                }
            }
        }

        info
    }

    /// Recursively collect all .cln files under a directory.
    fn collect_cln_files(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    files.extend(Self::collect_cln_files(&path));
                } else if path.extension().and_then(|e| e.to_str()) == Some("cln") {
                    let canonical = path.canonicalize().unwrap_or(path);
                    files.push(canonical);
                }
            }
        }
        files
    }

    // =========================================================================
    // Original Module Discovery Methods
    // =========================================================================

    /// Discover all imported modules starting from the entry module
    fn discover_modules(
        &self,
        unit: &mut CompilationUnit,
        graph: &mut ModuleGraph,
        search_paths: &[PathBuf],
    ) -> Result<(), Vec<CompilerError>> {
        let mut errors = Vec::new();
        let mut to_process: Vec<CompilationModuleId> = vec![unit.entry_module];
        let mut processed: HashSet<CompilationModuleId> = HashSet::new();

        while let Some(current_id) = to_process.pop() {
            if processed.contains(&current_id) {
                continue;
            }
            processed.insert(current_id);

            // Get the module's source
            let module = unit.get_module(current_id).unwrap();
            let source = module.source.clone();
            let file_path = module.file_path.clone();

            // Quick parse to extract imports (only Stages 1-2, minimal parsing)
            match self.extract_imports(&source, &file_path) {
                Ok(imports) => {
                    // Convert ExtractedImport to String for set_imports
                    let import_names: Vec<String> =
                        imports.iter().map(|i| i.name.clone()).collect();

                    // Update the module with its imports
                    if let Some(m) = unit.get_module_mut(current_id) {
                        m.set_imports(import_names);
                    }

                    // Process each import
                    for import in imports {
                        // Skip standard library modules (they're built-in)
                        if !import.is_file_import && Self::is_builtin_module(&import.name) {
                            continue;
                        }

                        // Find the module file based on import type
                        let find_result = if import.is_file_import {
                            // File import: resolve relative to the importing file's directory,
                            // with fallback to search_paths (project root, lib/, etc.)
                            self.find_file_import(&import.name, &file_path, search_paths)
                        } else {
                            // Module import: search in standard paths
                            self.find_module_file(&import.name, search_paths)
                        };

                        match find_result {
                            Ok((module_path, module_source)) => {
                                // Derive a human-readable module name (basename stem).
                                // add_module uses the canonical path as the true dedup
                                // key and disambiguates the name when two different
                                // files share the same stem.
                                let module_name = if import.is_file_import {
                                    Self::derive_module_name(&module_path)
                                } else {
                                    import.name.clone()
                                };

                                // Dedup by canonical path first; fall through to
                                // add_module only for genuinely new files.
                                let dep_id =
                                    if let Some(existing) = unit.module_id_for_path(&module_path) {
                                        existing
                                    } else {
                                        unit.add_module(
                                            module_name.clone(),
                                            module_path.clone(),
                                            module_source,
                                        )
                                    };

                                // Add to graph
                                graph.add_dependency(
                                    current_id,
                                    dep_id,
                                    SourceLocation {
                                        file: file_path.to_string_lossy().to_string(),
                                        line: 1,
                                        column: 1,
                                        byte_start: None,
                                        byte_end: None,
                                    },
                                );

                                // Queue for processing
                                if !processed.contains(&dep_id) {
                                    to_process.push(dep_id);
                                }
                            }
                            Err(e) => {
                                errors.push(e);
                            }
                        }
                    }
                }
                Err(e) => {
                    errors.push(e);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Find a file import by resolving the path relative to the importing file's directory,
    /// with a fallback to each search path (project root, lib/, etc.)
    fn find_file_import(
        &self,
        import_path: &str,
        importing_file: &Path,
        search_paths: &[PathBuf],
    ) -> Result<(PathBuf, String), CompilerError> {
        let base_dir = importing_file.parent().unwrap_or(Path::new("."));

        // Build candidate paths: importing file's directory first, then each search path
        let mut candidates: Vec<PathBuf> = Vec::with_capacity(search_paths.len() + 1);
        candidates.push(base_dir.join(import_path));
        for sp in search_paths {
            let candidate = sp.join(import_path);
            // Avoid duplicating the first candidate
            if candidate != candidates[0] {
                candidates.push(candidate);
            }
        }

        for full_path in &candidates {
            if full_path.exists() {
                let source = fs::read_to_string(full_path).map_err(|e| {
                    CompilerError::io_error(
                        format!(
                            "Failed to read imported file '{}': {}",
                            full_path.display(),
                            e
                        ),
                        None,
                        Some(SourceLocation {
                            file: importing_file.to_string_lossy().to_string(),
                            line: 1,
                            column: 1,
                            byte_start: None,
                            byte_end: None,
                        }),
                    )
                })?;
                return Ok((full_path.clone(), source));
            }
        }

        let searched = candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");

        Err(CompilerError::module_error(
            format!("Imported file '{}' not found", import_path),
            Some(format!("Searched: {}", searched)),
            Some(SourceLocation {
                file: importing_file.to_string_lossy().to_string(),
                line: 1,
                column: 1,
                byte_start: None,
                byte_end: None,
            }),
        ))
    }

    /// Extract import statements from source code
    ///
    /// This does a quick parse to find import declarations without full AST building
    fn extract_imports(
        &self,
        source: &str,
        file_path: &Path,
    ) -> Result<Vec<ExtractedImport>, CompilerError> {
        // Stage 1: Tokenize
        let source_code =
            SourceCode::new(source.to_string(), file_path.to_string_lossy().to_string());
        let mut lexer = SpecificationLexer::new(&source_code);
        let tokens = lexer.tokenize().map_err(|e| {
            CompilerError::syntax_error(
                format!("Lexer error: {}", e),
                None,
                Some(SourceLocation {
                    file: file_path.to_string_lossy().to_string(),
                    line: 1,
                    column: 1,
                    byte_start: None,
                    byte_end: None,
                }),
            )
        })?;

        // Stage 2: Parse to AST (use plugin keywords if available)
        let plugin_keywords = if let Some(ref registry) = self.config.plugin_registry {
            registry.get_all_block_keywords()
        } else {
            Vec::new()
        };
        let mut parser = SpecificationParser::with_plugin_keywords(
            tokens,
            file_path.to_string_lossy().to_string(),
            plugin_keywords,
        );
        let ast = parser.parse_program()?;

        // Extract imports from AST, preserving file import flag
        let mut seen = HashSet::new();
        let mut imports = Vec::new();

        for import in &ast.imports {
            let key = if import.is_file_import {
                // For file imports, use the full path as key
                import.name.clone()
            } else {
                // For module imports, extract the module name (before the dot)
                if let Some(dot_pos) = import.name.find('.') {
                    import.name[..dot_pos].to_string()
                } else {
                    import.name.clone()
                }
            };

            if !seen.contains(&key) {
                seen.insert(key.clone());
                imports.push(ExtractedImport {
                    name: key,
                    is_file_import: import.is_file_import,
                });
            }
        }

        Ok(imports)
    }

    /// Find a module file in the search paths
    fn find_module_file(
        &self,
        module_name: &str,
        search_paths: &[PathBuf],
    ) -> Result<(PathBuf, String), CompilerError> {
        // Handle nested module names (e.g., "foo.bar" -> "foo/bar")
        let path_name = module_name.replace('.', "/");

        let possible_filenames = [
            format!("{}.cln", path_name),
            format!("{}/mod.cln", path_name),
            format!("{}/index.cln", path_name),
            format!("{}.clean", path_name), // Legacy extension
        ];

        for search_path in search_paths {
            for filename in &possible_filenames {
                let full_path = search_path.join(filename);
                if full_path.exists() {
                    let source = fs::read_to_string(&full_path).map_err(|e| {
                        CompilerError::io_error(
                            format!(
                                "Failed to read module '{}' at {}: {}",
                                module_name,
                                full_path.display(),
                                e
                            ),
                            None,
                            Some(SourceLocation {
                                file: full_path.to_string_lossy().to_string(),
                                line: 1,
                                column: 1,
                                byte_start: None,
                                byte_end: None,
                            }),
                        )
                    })?;
                    return Ok((full_path, source));
                }
            }
        }

        // Module not found
        let search_str = search_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");

        Err(CompilerError::module_error(
            format!("Module '{}' not found", module_name),
            Some(format!("Searched in: {}", search_str)),
            None,
        ))
    }

    /// Parse all modules in the compilation unit to HIR
    fn parse_all_modules(&self, unit: &mut CompilationUnit) -> Result<(), Vec<CompilerError>> {
        let mut errors = Vec::new();

        // Parse in compilation order (dependencies first)
        let order = unit.compilation_order.clone();
        for module_id in order {
            if let Some(module) = unit.get_module_mut(module_id) {
                let is_entry = module.is_entry;
                match self.parse_module_to_hir(&module.source, &module.file_path, is_entry) {
                    Ok(hir) => {
                        module.set_hir(hir);
                    }
                    Err(e) => {
                        errors.push(e);
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Parse a single module through Stages 1-3 (with optional plugin expansion at Stage 2.5)
    ///
    /// `is_entry` controls preamble injection: only the entry module receives
    /// plugin preamble helpers (e.g. `redirect`).  Shared modules only get their
    /// own framework blocks expanded so the merged HIR never contains duplicates.
    fn parse_module_to_hir(
        &self,
        source: &str,
        file_path: &Path,
        is_entry: bool,
    ) -> Result<HirProgram, CompilerError> {
        // Stage 1: Tokenize
        let source_code =
            SourceCode::new(source.to_string(), file_path.to_string_lossy().to_string());
        let mut lexer = SpecificationLexer::new(&source_code);
        let tokens = lexer.tokenize().map_err(CompilerError::LexError)?;

        // Stage 2: Parse to AST
        // Get plugin keywords so the parser recognizes plugin-defined syntax
        let plugin_keywords = if let Some(ref registry) = self.config.plugin_registry {
            registry.get_all_block_keywords()
        } else {
            Vec::new()
        };
        let mut parser = SpecificationParser::with_plugin_keywords(
            tokens,
            file_path.to_string_lossy().to_string(),
            plugin_keywords,
        );
        let parsed_ast = parser.parse_program()?;

        // Stage 2.5a: Plugin Enforcement
        if let Some(ref registry) = self.config.plugin_registry {
            let enforcement_rules: Vec<(String, crate::plugins::plugin_abi::PluginEnforcement)> =
                registry
                    .loaded_manifests()
                    .iter()
                    .filter(|(_, m)| {
                        !m.enforcement.restricted_functions.is_empty()
                            || !m.enforcement.required_blocks.is_empty()
                            || !m.enforcement.block_folder_rules.is_empty()
                    })
                    .map(|(name, m)| (name.clone(), m.enforcement.clone()))
                    .collect();

            if !enforcement_rules.is_empty() {
                let file_str = file_path.to_string_lossy();
                let result = crate::plugins::enforcement::enforce_rules(
                    &parsed_ast,
                    &file_str,
                    &enforcement_rules,
                );
                for warning in &result.warnings {
                    eprintln!(
                        "warning[{}]: {} ({})",
                        warning.plugin, warning.message, warning.suggestion
                    );
                }
                if !result.errors.is_empty() {
                    return Err(CompilerError::PluginError {
                        message: result
                            .errors
                            .iter()
                            .map(|e| format!("{} ({})", e.message, e.suggestion))
                            .collect::<Vec<_>>()
                            .join("; "),
                        location: Some(SourceLocation {
                            file: file_path.to_string_lossy().to_string(),
                            line: 1,
                            column: 1,
                            byte_start: None,
                            byte_end: None,
                        }),
                    });
                }
            }
        }

        // Stage 2.5b: Plugin Expansion - transform framework blocks into Clean AST
        // Preamble helpers (e.g. redirect, json, error from frame.server) are only
        // injected for the entry module.  Shared modules get their own framework
        // blocks expanded but no preambles, preventing duplicate symbols in the
        // merged HIR (E001).
        let ast = if let Some(ref registry) = self.config.plugin_registry {
            tracing::debug!(
                file = %file_path.display(),
                is_entry = is_entry,
                "Starting Stage 2.5: Plugin Expansion for module"
            );
            let mut expander = PluginExpander::new(registry.as_ref());
            let expand_result = if is_entry {
                expander.expand_program(parsed_ast)
            } else {
                expander.expand_program_without_preambles(parsed_ast)
            };
            expand_result.map_err(|e| {
                CompilerError::syntax_error(
                    e.to_string(),
                    Some("Plugin expansion failed".to_string()),
                    Some(SourceLocation {
                        file: file_path.to_string_lossy().to_string(),
                        line: 1,
                        column: 1,
                        byte_start: None,
                        byte_end: None,
                    }),
                )
            })?
        } else {
            parsed_ast
        };

        // Stage 3: Build HIR
        let mut hir_builder = HirBuilder::new();
        let hir_result = hir_builder.build_hir(ast)?;

        Ok(hir_result.hir)
    }

    /// Check if a module name refers to a built-in module
    fn is_builtin_module(name: &str) -> bool {
        matches!(
            name.to_lowercase().as_str(),
            "math" | "string" | "list" | "file" | "http" | "json" | "console"
        )
    }

    /// Derive a module name from a file path
    fn derive_module_name(path: &Path) -> String {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "main".to_string())
    }
}

impl Default for MultiFileCompiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_file_compilation() {
        let sources = vec![(
            "main".to_string(),
            r#"start:
	print(42)
"#
            .to_string(),
        )];

        let compiler = MultiFileCompiler::new();
        let result = compiler.build_from_sources("main", &sources);

        assert!(
            result.is_ok(),
            "Single file should compile: {:?}",
            result.err()
        );
        let unit = result.unwrap();
        assert_eq!(unit.module_count(), 1);
        assert!(unit.all_modules_have_hir());
    }

    #[test]
    fn test_two_file_compilation() {
        let sources = vec![
            (
                "main".to_string(),
                r#"import:
	utils

start:
	integer x = 42
	print(x)
"#
                .to_string(),
            ),
            (
                "utils".to_string(),
                r#"functions:
	integer add(integer a, integer b)
		return a + b
"#
                .to_string(),
            ),
        ];

        let compiler = MultiFileCompiler::new();
        let result = compiler.build_from_sources("main", &sources);

        assert!(
            result.is_ok(),
            "Two files should compile: {:?}",
            result.err()
        );
        let unit = result.unwrap();
        assert_eq!(unit.module_count(), 2);
        assert!(unit.all_modules_have_hir());

        // Check compilation order - utils should come before main
        let order = &unit.compilation_order;
        let utils_id = *unit.module_by_name.get("utils").unwrap();
        let main_id = *unit.module_by_name.get("main").unwrap();

        let utils_pos = order.iter().position(|&x| x == utils_id);
        let main_pos = order.iter().position(|&x| x == main_id);

        assert!(
            utils_pos < main_pos,
            "utils should come before main in compilation order"
        );
    }

    #[test]
    fn test_builtin_module_skipped() {
        let sources = vec![(
            "main".to_string(),
            r#"import:
	math

start:
	integer x = abs(-5)
	print(x)
"#
            .to_string(),
        )];

        let compiler = MultiFileCompiler::new();
        let result = compiler.build_from_sources("main", &sources);

        // Should succeed - math is a builtin and should be skipped
        assert!(
            result.is_ok(),
            "Builtin imports should be skipped: {:?}",
            result.err()
        );
        let unit = result.unwrap();
        assert_eq!(unit.module_count(), 1); // Only main, not math
    }

    #[test]
    fn test_find_file_import_search_path_fallback() {
        // E013 regression: find_file_import should search search_paths when the file is not
        // adjacent to the importing file (e.g. importing file is in a subdirectory but
        // helpers.cln lives at the project root).
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let project_root = dir.path();

        // Create project structure:
        //   project_root/helpers.cln
        //   project_root/sub/main.cln   (imports "helpers.cln")
        let sub_dir = project_root.join("sub");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let helpers_path = project_root.join("helpers.cln");
        std::fs::write(
            &helpers_path,
            "functions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b\n",
        )
        .unwrap();

        let importing_file = sub_dir.join("main.cln");
        std::fs::write(&importing_file, "").unwrap();

        let compiler = MultiFileCompiler::new();

        // search_paths includes project_root (mimics build_from_file inserting entry dir)
        let search_paths = vec![project_root.to_path_buf()];

        // Should find helpers.cln via search_paths fallback (not in sub/ dir)
        let result = compiler.find_file_import("helpers.cln", &importing_file, &search_paths);
        assert!(
            result.is_ok(),
            "Should find helpers.cln via search_path fallback: {:?}",
            result.err()
        );
        let (found_path, _) = result.unwrap();
        assert_eq!(
            found_path.canonicalize().unwrap(),
            helpers_path.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_derive_module_name() {
        assert_eq!(
            MultiFileCompiler::derive_module_name(Path::new("/path/to/utils.cln")),
            "utils"
        );
        assert_eq!(
            MultiFileCompiler::derive_module_name(Path::new("main.cln")),
            "main"
        );
        assert_eq!(
            MultiFileCompiler::derive_module_name(Path::new("/complex/path/MyModule.cln")),
            "MyModule"
        );
    }

    // =========================================================================
    // HTML-First Page Processing Tests
    // =========================================================================

    #[test]
    fn test_file_path_to_route() {
        let compiler = MultiFileCompiler::new();
        let base = Path::new("/project/pages");

        // Basic routes (using .html extension for pages)
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/index.html"), base),
            "/"
        );
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/about.html"), base),
            "/about"
        );
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/blog/index.html"), base),
            "/blog"
        );

        // Nested routes
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/blog/post.html"), base),
            "/blog/post"
        );

        // Dynamic routes
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/blog/[slug].html"), base),
            "/blog/:slug"
        );
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/users/[id]/profile.html"), base),
            "/users/:id/profile"
        );
    }

    #[test]
    fn test_is_html_page_file() {
        let compiler = MultiFileCompiler::new();

        // Should match .html files
        assert!(compiler.is_html_page_file(Path::new("/project/pages/index.html")));
        assert!(compiler.is_html_page_file(Path::new("about.html")));
        assert!(compiler.is_html_page_file(Path::new("/blog/[slug].html")));

        // Should NOT match other extensions
        assert!(!compiler.is_html_page_file(Path::new("main.cln")));
        assert!(!compiler.is_html_page_file(Path::new("style.css")));
        assert!(!compiler.is_html_page_file(Path::new("app.js")));
    }

    #[test]
    fn test_derive_companion_module_name() {
        let base = Path::new("app/pages");

        assert_eq!(
            derive_companion_module_name(Path::new("app/pages/dashboard.cln"), base),
            "dashboard"
        );
        assert_eq!(
            derive_companion_module_name(Path::new("app/pages/blog/post.cln"), base),
            "blog_post"
        );
        assert_eq!(
            derive_companion_module_name(Path::new("app/pages/blog/[slug].cln"), base),
            "blog_slug"
        );
    }

    #[test]
    fn test_route_to_class_name() {
        let compiler = MultiFileCompiler::new();

        assert_eq!(compiler.route_to_class_name("/"), "IndexPage");
        assert_eq!(compiler.route_to_class_name("/about"), "AboutPage");
        assert_eq!(compiler.route_to_class_name("/blog"), "BlogPage");
        assert_eq!(compiler.route_to_class_name("/blog/posts"), "BlogPostsPage");
        assert_eq!(compiler.route_to_class_name("/blog/:slug"), "BlogSlugPage");
        assert_eq!(
            compiler.route_to_class_name("/users/:id/profile"),
            "UsersIdProfilePage"
        );
    }

    #[test]
    fn test_class_name_to_tag() {
        let compiler = MultiFileCompiler::new();

        assert_eq!(compiler.class_name_to_tag("StatCard"), "stat-card");
        assert_eq!(compiler.class_name_to_tag("UserBadge"), "user-badge");
        assert_eq!(compiler.class_name_to_tag("AppHeader"), "app-header");
        assert_eq!(
            compiler.class_name_to_tag("NewsletterForm"),
            "newsletter-form"
        );
    }

    #[test]
    fn test_extract_html_metadata() {
        let compiler = MultiFileCompiler::new();

        // Basic metadata extraction
        let html = r#"<page layout="main" title="Test Page"></page>
<section>Content</section>
<stat-card></stat-card>
<user-badge></user-badge>"#;

        let metadata = compiler.extract_html_metadata(html);
        assert_eq!(metadata.layout, Some("main".to_string()));
        assert_eq!(metadata.title, Some("Test Page".to_string()));
        assert!(metadata.custom_tags.contains(&"stat-card".to_string()));
        assert!(metadata.custom_tags.contains(&"user-badge".to_string()));

        // Title from <title> tag
        let html2 = r#"<html><head><title>Page Title</title></head><body></body></html>"#;
        let metadata2 = compiler.extract_html_metadata(html2);
        assert_eq!(metadata2.title, Some("Page Title".to_string()));

        // Data block extraction
        let html3 = r#"<data>
posts = Post.findAll()
</data>
<section></section>"#;
        let metadata3 = compiler.extract_html_metadata(html3);
        assert!(metadata3.data_block.is_some());
        assert!(metadata3.data_block.unwrap().contains("posts"));
    }

    #[test]
    fn test_extract_attr_value() {
        let compiler = MultiFileCompiler::new();

        let tag = r#"<page layout="main" title="Test">"#;
        assert_eq!(
            compiler.extract_attr_value(tag, "layout"),
            Some("main".to_string())
        );
        assert_eq!(
            compiler.extract_attr_value(tag, "title"),
            Some("Test".to_string())
        );
        assert_eq!(compiler.extract_attr_value(tag, "missing"), None);

        // Single quotes
        let tag2 = r#"<page layout='sidebar'>"#;
        assert_eq!(
            compiler.extract_attr_value(tag2, "layout"),
            Some("sidebar".to_string())
        );
    }

    #[test]
    fn test_component_registry() {
        let mut registry = ComponentRegistry::new();

        registry.register(
            "stat-card".to_string(),
            ComponentInfo {
                class_name: "StatCard".to_string(),
                file_path: PathBuf::from("/components/StatCard.cln"),
                props: vec!["label".to_string(), "value".to_string()],
                has_client_hydration: false,
            },
        );

        assert!(registry.has_component("stat-card"));
        assert!(!registry.has_component("unknown-tag"));

        let info = registry.get("stat-card").unwrap();
        assert_eq!(info.class_name, "StatCard");
        assert!(info.props.contains(&"label".to_string()));

        // Test JSON serialization
        let json = registry.to_json();
        assert!(json.contains("stat-card"));
        assert!(json.contains("StatCard"));
    }

    #[test]
    fn test_html_to_render_body() {
        let compiler = MultiFileCompiler::new();
        let registry = ComponentRegistry::new();

        let html = r#"<html>
<body>
<h1>Hello World</h1>
<p>Test content</p>
</body>
</html>"#;

        let body = compiler.html_to_render_body(html, &registry);

        // Should generate string concatenation code
        assert!(body.contains("string html = \"\""));
        assert!(body.contains("return html"));
        assert!(body.contains("<h1>Hello World</h1>"));
    }

    #[test]
    fn test_parse_manifest_info_web_target_adds_web_root_as_shared() {
        let manifest = "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui, frame.server]\n\t\tentry: app/web/pages/index.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();

        // Create the pages directory so web_root.exists() returns true
        let web_root = manifest_dir.join("app").join("web");
        std::fs::create_dir_all(web_root.join("pages")).unwrap();

        let info = MultiFileCompiler::parse_manifest_info(manifest, manifest_dir);

        assert!(
            info.shared_folders.contains(&web_root),
            "web root app/web/ should be auto-added as shared folder for target: web"
        );
    }

    #[test]
    fn test_parse_manifest_info_no_web_target_no_auto_shared() {
        let manifest = "package: Test\n\tentry: src/main.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();
        std::fs::create_dir_all(manifest_dir.join("src")).unwrap();

        let info = MultiFileCompiler::parse_manifest_info(manifest, manifest_dir);

        assert!(
            info.shared_folders.is_empty(),
            "no target: web means no auto-shared folder"
        );
    }
}
