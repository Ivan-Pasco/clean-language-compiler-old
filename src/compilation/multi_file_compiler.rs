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

/// Represents an extracted import with additional metadata
#[derive(Debug, Clone)]
struct ExtractedImport {
    /// Module name or file path
    name: String,
    /// True if this is a file path import
    is_file_import: bool,
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
}

/// Represents an HTML page discovered during compilation
///
/// Pages use the `.html.cln` extension to indicate they should be processed
/// by the Clean Language compiler. Regular `.html` files are static and
/// served as-is without processing.
#[derive(Debug, Clone)]
pub struct HtmlPage {
    /// File path to the HTML page (.html.cln file)
    pub file_path: PathBuf,
    /// Route path derived from file path (e.g., "/blog/[slug].html.cln" -> "/blog/:slug")
    pub route_path: String,
    /// Raw HTML content
    pub html_content: String,
    /// Page metadata (title, layout, etc.)
    pub metadata: HtmlPageMetadata,
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
        let entry_source = fs::read_to_string(entry_path).map_err(|e| {
            vec![CompilerError::io_error(
                format!("Failed to read entry file {}: {}", entry_path.display(), e),
                None,
                None,
            )]
        })?;

        // Get the canonical path for the entry file
        let canonical_path = entry_path
            .canonicalize()
            .unwrap_or_else(|_| entry_path.to_path_buf());

        // Derive module name from file path
        let entry_name = Self::derive_module_name(&canonical_path);

        // Create compilation unit
        let mut unit = CompilationUnit::new(entry_name, canonical_path.clone(), entry_source);

        // Build the module graph by discovering all imports
        let mut graph = ModuleGraph::new();
        graph.add_module(unit.entry_module);

        // Add the entry file's directory to search paths
        let mut search_paths = self.config.search_paths.clone();
        if let Some(parent) = canonical_path.parent() {
            search_paths.insert(0, parent.to_path_buf());
        }

        // Discover and load all modules
        self.discover_modules(&mut unit, &mut graph, &search_paths)?;

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
            } else if self.is_html_cln_file(&path) {
                // Process .html.cln file (pages that need Clean Language processing)
                // Regular .html files are static and not processed here
                match self.process_html_file(&path, base_dir) {
                    Ok(page) => pages.push(page),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to process .html.cln file {}: {:?}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }

        Ok(pages)
    }

    /// Check if a file has the .html.cln extension
    ///
    /// Only .html.cln files are processed by the compiler.
    /// Regular .html files are static and served as-is.
    fn is_html_cln_file(&self, path: &Path) -> bool {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        file_name.ends_with(".html.cln")
    }

    /// Process a single .html.cln file into an HtmlPage
    fn process_html_file(
        &self,
        file_path: &Path,
        base_dir: &Path,
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
        })
    }

    /// Convert file path to route path
    /// e.g., /pages/blog/[slug].html.cln -> /blog/:slug
    fn file_path_to_route(&self, file_path: &Path, base_dir: &Path) -> String {
        let relative = file_path.strip_prefix(base_dir).unwrap_or(file_path);

        let mut route = String::from("/");

        for component in relative.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();

                // Skip index files at the route level
                if name_str == "index.html.cln" {
                    continue;
                }

                // Remove .html.cln extension (processed pages)
                let name_str = name_str.trim_end_matches(".html.cln");

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
            if path.extension().map_or(false, |ext| ext == "cln") {
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
                            // File import: resolve relative to the importing file's directory
                            self.find_file_import(&import.name, &file_path)
                        } else {
                            // Module import: search in standard paths
                            self.find_module_file(&import.name, search_paths)
                        };

                        match find_result {
                            Ok((module_path, module_source)) => {
                                // Derive module name from file path for file imports
                                let module_name = if import.is_file_import {
                                    Self::derive_module_name(&module_path)
                                } else {
                                    import.name.clone()
                                };

                                // Add the module if not already present
                                let dep_id = if !unit.has_module(&module_name) {
                                    unit.add_module(
                                        module_name.clone(),
                                        module_path.clone(),
                                        module_source,
                                    )
                                } else {
                                    *unit.module_by_name.get(&module_name).unwrap()
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

    /// Find a file import by resolving the path relative to the importing file's directory
    fn find_file_import(
        &self,
        import_path: &str,
        importing_file: &Path,
    ) -> Result<(PathBuf, String), CompilerError> {
        // Get the directory of the importing file
        let base_dir = importing_file.parent().unwrap_or(Path::new("."));

        // Resolve the import path relative to the base directory
        let full_path = base_dir.join(import_path);

        // Check if the file exists
        if full_path.exists() {
            let source = fs::read_to_string(&full_path).map_err(|e| {
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
            return Ok((full_path, source));
        }

        // File not found - build helpful error message
        Err(CompilerError::module_error(
            format!("Imported file '{}' not found", import_path),
            Some(format!(
                "Resolved path: {} (relative to {})",
                full_path.display(),
                base_dir.display()
            )),
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
                match self.parse_module_to_hir(&module.source, &module.file_path) {
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
    fn parse_module_to_hir(
        &self,
        source: &str,
        file_path: &Path,
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

        // Stage 2.5: Plugin Expansion - transform framework blocks into Clean AST
        let ast = if let Some(ref registry) = self.config.plugin_registry {
            tracing::debug!(
                file = %file_path.display(),
                "Starting Stage 2.5: Plugin Expansion for module"
            );
            let mut expander = PluginExpander::new(registry.as_ref());
            expander.expand_program(parsed_ast).map_err(|e| {
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

        // Basic routes (using .html.cln extension for processed pages)
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/index.html.cln"), base),
            "/"
        );
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/about.html.cln"), base),
            "/about"
        );
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/blog/index.html.cln"), base),
            "/blog"
        );

        // Nested routes
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/blog/post.html.cln"), base),
            "/blog/post"
        );

        // Dynamic routes
        assert_eq!(
            compiler.file_path_to_route(Path::new("/project/pages/blog/[slug].html.cln"), base),
            "/blog/:slug"
        );
        assert_eq!(
            compiler.file_path_to_route(
                Path::new("/project/pages/users/[id]/profile.html.cln"),
                base
            ),
            "/users/:id/profile"
        );
    }

    #[test]
    fn test_is_html_cln_file() {
        let compiler = MultiFileCompiler::new();

        // Should match .html.cln files
        assert!(compiler.is_html_cln_file(Path::new("/project/pages/index.html.cln")));
        assert!(compiler.is_html_cln_file(Path::new("about.html.cln")));
        assert!(compiler.is_html_cln_file(Path::new("/blog/[slug].html.cln")));

        // Should NOT match regular .html files (static, not processed)
        assert!(!compiler.is_html_cln_file(Path::new("/project/pages/index.html")));
        assert!(!compiler.is_html_cln_file(Path::new("static.html")));

        // Should NOT match other extensions
        assert!(!compiler.is_html_cln_file(Path::new("main.cln")));
        assert!(!compiler.is_html_cln_file(Path::new("style.css")));
        assert!(!compiler.is_html_cln_file(Path::new("app.js")));
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
}
