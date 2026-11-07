//! Multi-file Compiler
//!
//! Orchestrates the compilation of multi-file Clean Language programs.
//! This is the main entry point for building programs with imports.

use super::{CompilationModuleId, CompilationUnit, ModuleGraph, ModulePluginExpansion};
use crate::ast::SourceLocation;
use crate::error::{CompilerError, ErrorContext, ErrorType};
use crate::hir::hir_builder::HirBuilder;
use crate::hir::HirProgram;
use crate::lexer::specification_lexer::{SourceCode, SpecificationLexer};
use crate::parser::SpecificationParser;
use crate::plugins::{HtmlEntryOutput, PluginExpander, PluginRegistry};
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
    /// Where the import is written, for import-cycle and resolution errors
    location: SourceLocation,
}

/// Line and column (1-based, in characters) of the `import:` entry for
/// `name` in `source`; `(1, 1)` when it cannot be found. File imports match
/// the quoted path, module imports the module name before any `.member` or
/// ` as alias`.
fn import_position(source: &str, name: &str, is_file_import: bool) -> (usize, usize) {
    let mut in_import_block = false;
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        let entry = if let Some(rest) = trimmed.strip_prefix("import:") {
            in_import_block = true;
            rest.trim()
        } else if in_import_block && line.starts_with([' ', '\t']) {
            trimmed
        } else {
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                in_import_block = false;
            }
            continue;
        };
        if entry.is_empty() || entry.starts_with("//") {
            continue;
        }
        let entry_name = if is_file_import {
            entry.trim_matches('"').split('"').next().unwrap_or("")
        } else {
            entry.split([' ', '.']).next().unwrap_or("")
        };
        if entry_name == name {
            // `find` returns a char boundary, so the prefix slice is valid.
            let byte = line.find(entry).unwrap_or(0);
            return (index + 1, line[..byte].chars().count() + 1);
        }
    }
    (1, 1)
}

/// Metadata extracted from a project manifest (main.cln with package: block)
#[derive(Debug, Default)]
struct ManifestInfo {
    /// The declared entry file for the first target (e.g. app/web/pages/home.cln)
    entry_path: Option<PathBuf>,
    /// Folders declared in shared: [...] that should be compiled for every target
    shared_folders: Vec<PathBuf>,
    /// True when `frame.server` is listed in the manifest's plugins declaration
    has_frame_server: bool,
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

    /// Plugin registry for framework block expansion
    pub plugin_registry: Option<Arc<PluginRegistry>>,

    /// Release mode: strip `always:` invariant checks from the compiled output.
    /// When true, class `always:` block conditions are not injected before
    /// method returns, producing smaller and faster WASM.
    pub release_mode: bool,

    /// Plugin Contracts v2 — true when this compilation is the nested client
    /// (browser) build that produces `frontend.wasm`. Controls which lifecycle
    /// slots are dispatched by `PluginExpander` per contracts/lifecycle.md §3.
    /// See `MultiFileCompilerConfig::with_client_mode`.
    pub client_mode: bool,
}

impl std::fmt::Debug for MultiFileCompilerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiFileCompilerConfig")
            .field("search_paths", &self.search_paths)
            .field("opt_level", &self.opt_level)
            .field("debug", &self.debug)
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
            plugin_registry: None,
            release_mode: false,
            client_mode: false,
        }
    }
}

impl MultiFileCompilerConfig {
    /// Create a new config with custom search paths
    pub fn with_search_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.search_paths = paths;
        self
    }

    /// Set the optimization level
    pub fn with_opt_level(mut self, level: u8) -> Self {
        self.opt_level = level;
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

    /// Plugin Contracts v2 — declare this is the nested client (browser) build.
    /// Controls which lifecycle slots `PluginExpander` dispatches:
    /// `is_server_build = !client_mode`, `is_client_build = client_mode`.
    /// See foundation/spec/plugins/contracts/lifecycle.md §3.
    pub fn with_client_mode(mut self, client_mode: bool) -> Self {
        self.client_mode = client_mode;
        self
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

        // AST a typed-emission HTML processor emitted for an HTML entry; it is
        // attached to the entry module and merged during plugin expansion.
        let mut entry_plugin_expansion: Option<ModulePluginExpansion> = None;

        let (manifest_info, entry_source, canonical_path) =
            if let Some(ref manifest_dir) = manifest_root {
                let (info, mut manifest_errors) = Self::parse_manifest_info(
                    &manifest_source,
                    manifest_dir,
                    self.config.plugin_registry.as_ref(),
                );
                manifest_errors.extend(Self::code_after_manifest_error(
                    &manifest_source,
                    entry_path,
                ));

                // Migration diagnostics (e.g. BLD-LAYOUT) abort the build
                // before any file walking so the developer sees the layout
                // problem before any downstream parse/typecheck errors.
                if !manifest_errors.is_empty() {
                    return Err(manifest_errors);
                }

                // Resolve the actual entry file if declared
                if let Some(ref ep) = info.entry_path {
                    let is_html_entry = ep.extension().and_then(|e| e.to_str()) == Some("html");
                    if is_html_entry {
                        // HTML-first entry: ask a loaded plugin (e.g. frame.ui)
                        // to convert the .html page into Clean Language source
                        // via its `process_html` WASM export. The compiler
                        // itself owns no HTML logic — feeding raw HTML to the
                        // Clean parser would error with SYN001 on the leading
                        // `<`. See `[exports].process_html` in plugin.toml and
                        // `PluginRegistry::find_html_processor`.
                        let (plugin_name, output) =
                            self.process_html_entry_via_plugin(ep, manifest_dir)?;
                        let ep_canonical = ep.canonicalize().unwrap_or_else(|_| ep.clone());
                        let src = match output {
                            HtmlEntryOutput::Source(src) => src,
                            // A typed-emission plugin hands back the page as
                            // AST. The entry module starts from empty source
                            // and the expansion is merged into it during
                            // plugin expansion (parse_module_to_hir).
                            HtmlEntryOutput::Expansion(expansion) => {
                                entry_plugin_expansion = Some(ModulePluginExpansion {
                                    plugin_name,
                                    expansion,
                                });
                                String::new()
                            }
                        };
                        (Some(info), src, ep_canonical)
                    } else {
                        match fs::read_to_string(ep) {
                            Ok(src) => {
                                let ep_canonical = ep.canonicalize().unwrap_or_else(|_| ep.clone());
                                (Some(info), src, ep_canonical)
                            }
                            Err(e) => {
                                let msg = format!("Package entry {}: {}", ep.display(), e);
                                return Err(vec![CompilerError::io_error(msg, None, None)]);
                            }
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
        if let Some(module) = unit.get_module_mut(unit.entry_module) {
            module.plugin_expansion = entry_plugin_expansion;
        }

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
        //
        // Collect all shared files first, then run plugin-declared assemble hooks
        // for page-companion detection, function prefixing, and synthetic route
        // module generation. The compiler owns no assemble logic itself.
        if let (Some(ref info), Some(ref project_root), Some(ref manifest_dir)) =
            (&manifest_info, &manifest_root, &manifest_root)
        {
            // Pass 1: collect (file_path, shared_dir, raw_content) for every shared file.
            let shared_files = Self::collect_shared_files_deduped(&info.shared_folders, &unit);

            // Build the source-file list for assemble: every discovered .cln
            // module in the compilation unit, deduplicated by canonical path
            // against the shared-folder walk above.
            //
            // The AssembleInput contract (see
            // src/plugins/plugin_abi.rs::AssembleInput::source_files —
            // "All source files in the compilation unit") requires this to
            // be the full set. Restricting it to `shared_files` excluded the
            // entry file and any module discovered through Stage 2.5 expansion
            // — frame.ui's page-companion detection, custom routing plugins,
            // and any third-party file-pattern hook then could not see them.
            // Reported as COMPILER-ASSEMBLE-INPUT-OMITS-ENTRY-AND-NON-OWNED-FILES
            // (fp 38323eb59c33). Manifested as
            // PAGE-COMPANION-NO-ROUTE-GENERATED on the framework dashboard.
            let mut source_files: Vec<crate::plugins::AssembleSourceFile> = shared_files
                .iter()
                .map(|(p, _, c)| crate::plugins::AssembleSourceFile {
                    path: p.to_string_lossy().into_owned(),
                    content: c.clone(),
                })
                .collect();

            let mut shared_paths: HashSet<PathBuf> = shared_files
                .iter()
                .map(|(p, _, _)| p.canonicalize().unwrap_or_else(|_| p.clone()))
                .collect();

            // Every other file in a plugin-owned folder that matches the
            // owning plugin's `[paths].patterns` (plugin-contract.md §1.2) —
            // `.html` pages, for example — is passed to `assemble` with its
            // content. These files are inputs of the assemble step only; they
            // are never parsed as Clean modules.
            if let Some(ref registry) = self.config.plugin_registry {
                for (path, content) in
                    Self::collect_plugin_pattern_files(registry, manifest_dir, &info.shared_folders)
                {
                    let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if shared_paths.insert(canonical) {
                        source_files.push(crate::plugins::AssembleSourceFile {
                            path: path.to_string_lossy().into_owned(),
                            content,
                        });
                    }
                }
            }

            for module in unit.modules.values() {
                let module_canonical = module
                    .file_path
                    .canonicalize()
                    .unwrap_or_else(|_| module.file_path.clone());
                if shared_paths.contains(&module_canonical) {
                    continue;
                }
                source_files.push(crate::plugins::AssembleSourceFile {
                    path: module.file_path.to_string_lossy().into_owned(),
                    content: module.source.clone(),
                });
            }

            // Pass 2: run assemble hooks.
            //
            // Assembly is owned by whichever loaded plugin declares
            // `[exports].assemble` in its plugin.toml (e.g. frame.ui >= 2.6.11).
            // The compiler does not contain a Rust fallback — manifests that
            // load no assemble-capable plugin simply get an empty output here.
            let assemble_input = crate::plugins::AssembleInput {
                source_files,
                project_root: project_root.to_string_lossy().into_owned(),
                manifest_dir: manifest_dir.to_string_lossy().into_owned(),
                has_frame_server: info.has_frame_server,
            };

            let mut assemble_output = crate::plugins::plugin_abi::AssembleOutput::default();
            if let Some(ref registry) = self.config.plugin_registry {
                let (hook_output, hook_errors) = registry.run_assemble_hooks(&assemble_input);
                // Any plugin's `assemble` failure aborts the build with a
                // proper diagnostic rather than silently producing an empty
                // WASM. See COMPILER-ASSEMBLE-ERROR-SWALLOWED for the prior
                // bug where these errors were discarded.
                if !hook_errors.is_empty() {
                    return Err(hook_errors
                        .into_iter()
                        .map(|(plugin_name, err)| CompilerError::PluginError {
                            message: format!("plugin `{plugin_name}` assemble hook failed: {err}"),
                            location: None,
                        })
                        .collect());
                }
                assemble_output
                    .injected_sources
                    .extend(hook_output.injected_sources);
                assemble_output
                    .transformed_sources
                    .extend(hook_output.transformed_sources);
            }

            let transformed_map: HashMap<String, String> = assemble_output
                .transformed_sources
                .into_iter()
                .map(|t| (t.path, t.content))
                .collect();

            // Track which transformed_map entries we have already applied so
            // pass 3b can finish whatever pass 3a missed without
            // double-applying (canonicalized to match the lookup key used in
            // `module_id_for_path`).
            let mut applied_transforms: HashSet<PathBuf> = HashSet::new();

            // Pass 3a: add shared-folder files to the compilation unit with
            // transformations applied. Files inside the plugin's owned paths
            // flow through this branch.
            for (file_path, _shared_dir, raw_source) in shared_files {
                let path_str = file_path.to_string_lossy().into_owned();
                if let Some(existing_id) = unit.module_id_for_path(&file_path) {
                    // Already in unit (e.g. the entry file) — apply transformation if any.
                    if let Some(transformed) = transformed_map.get(&path_str) {
                        if let Some(module) = unit.get_module_mut(existing_id) {
                            module.source = transformed.clone();
                            applied_transforms.insert(
                                file_path
                                    .canonicalize()
                                    .unwrap_or_else(|_| file_path.clone()),
                            );
                        }
                    }
                } else {
                    let (name, source) = if let Some(transformed) = transformed_map.get(&path_str) {
                        let module_name =
                            crate::plugins::builtin_assemblers::derive_companion_module_name(
                                &file_path,
                            );
                        applied_transforms.insert(
                            file_path
                                .canonicalize()
                                .unwrap_or_else(|_| file_path.clone()),
                        );
                        (module_name, transformed.clone())
                    } else {
                        (Self::derive_module_name(&file_path), raw_source)
                    };
                    let id = unit.add_module(name, file_path, source);
                    graph.add_module(id);
                }
            }

            // Pass 3b: apply transformations for modules outside every
            // shared folder.
            //
            // The plugin sees these files in `source_files` thanks to
            // COMPILER-ASSEMBLE-INPUT-OMITS-ENTRY-AND-NON-OWNED-FILES — the
            // entry file and any import-discovered module now reach the
            // assemble hook regardless of `[paths].owns`. The previous
            // implementation only applied transformations during pass 3a,
            // which iterated `shared_files`, so any `transformed_sources`
            // entry the plugin emitted for the entry (e.g. frame.ui's
            // companion rename of `any load(Request)` to
            // `any pages_<name>_load_impl(any)` plus the
            // `pages_<name>_load` wrapper) was silently dropped. Downstream
            // the injected route module called the renamed name, the
            // entry still defined the original `load`, and the build
            // failed with `error[SEM007]: Function 'pages_<name>_load' not found`.
            // That is dashboard bug `d275648c7452` /
            // `CLN-0-30-345-PLUGIN-BUILD-EMITS-UNDEFINED-FN-REF-CAUSING-SEM007`,
            // which the dashboard reporter correctly diagnosed as
            // post-plugin-expansion symbol-table fallout — the actual
            // cause is here, in the apply-transformations pass that didn't
            // mirror the input-collection pass.
            for (path_str, content) in &transformed_map {
                let path = PathBuf::from(path_str);
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                if applied_transforms.contains(&canonical) {
                    continue;
                }
                if let Some(existing_id) = unit.module_id_for_path(&path) {
                    if let Some(module) = unit.get_module_mut(existing_id) {
                        module.source = content.clone();
                    }
                }
            }

            // Pass 4: inject synthetic sources produced by assemble hooks.
            for injected in assemble_output.injected_sources {
                let synthetic_path = PathBuf::from(&injected.virtual_path);
                let id = unit.add_module(
                    "__page_routes_generated".to_string(),
                    synthetic_path,
                    injected.content,
                );
                graph.add_module(id);
                tracing::debug!("Generated synthetic page route registrations (E-PGREG fix)");
            }
        }

        // Determine compilation order. Modules are named by name and path so
        // an import-cycle error reads `main (main.cln) -> utils (...)`.
        let module_ids: Vec<CompilationModuleId> = graph.modules().copied().collect();
        for id in module_ids {
            if let Some(module) = unit.get_module(id) {
                graph.set_module_name(
                    id,
                    format!("{} ({})", module.name, module.file_path.display()),
                );
            }
        }
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
            plugin_registry: self.config.plugin_registry.clone(),
            release_mode: self.config.release_mode,
            client_mode: self.config.client_mode,
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

    /// Convert an HTML entry into the entry module by delegating to a
    /// plugin's `process_html` WASM export.
    ///
    /// Architectural note: the compiler owns no HTML logic. It reads the HTML
    /// file off disk, hands the bytes to the plugin (typically frame.ui via
    /// `[exports].process_html`), and uses whatever the plugin returns — Clean
    /// source (v1 plugins) or emitted AST (typed-emission plugins) — as the
    /// entry module. Without a loaded HTML processor the manifest
    /// configuration is rejected — no fallback parsing of the raw HTML, no
    /// inline conversion in Rust. Returns the processing plugin's name with
    /// its output.
    fn process_html_entry_via_plugin(
        &self,
        entry_path: &Path,
        manifest_dir: &Path,
    ) -> Result<(String, HtmlEntryOutput), Vec<CompilerError>> {
        let registry = self.config.plugin_registry.as_ref().ok_or_else(|| {
            vec![CompilerError::PluginError {
                message: format!(
                    "HTML entry `{}` requires a plugin with `[exports].process_html` \
                     (e.g. frame.ui), but no plugins are loaded for this build.",
                    entry_path.display()
                ),
                location: None,
            }]
        })?;

        let processor = registry.find_html_processor().ok_or_else(|| {
            vec![CompilerError::PluginError {
                message: format!(
                    "HTML entry `{}` requires a plugin that declares \
                     `[exports].process_html` in its plugin.toml. Add frame.ui \
                     (or another HTML processor) to the manifest `plugins:` list.",
                    entry_path.display()
                ),
                location: None,
            }]
        })?;

        let html = fs::read_to_string(entry_path).map_err(|e| {
            vec![CompilerError::io_error(
                format!(
                    "Failed to read HTML entry `{}`: {}",
                    entry_path.display(),
                    e
                ),
                None,
                None,
            )]
        })?;

        // Path passed to the plugin is relative to the manifest directory so
        // that plugin-generated identifiers and route paths are stable across
        // build hosts.
        let relative_path = entry_path
            .strip_prefix(manifest_dir)
            .unwrap_or(entry_path)
            .to_string_lossy()
            .to_string();

        // Companion `.cln` next to the page — same convention as
        // `find_companion_file`. The plugin decides whether to honour it.
        let companion_json = {
            let companion_path = entry_path.with_extension("cln");
            if companion_path.exists() {
                let module_name = crate::plugins::builtin_assemblers::derive_companion_module_name(
                    &companion_path,
                );
                let companion_source = fs::read_to_string(&companion_path).unwrap_or_default();
                let has_guard =
                    companion_source.contains("any guard(") || companion_source.contains("guard()");
                let has_load =
                    companion_source.contains("any load(") || companion_source.contains("load()");
                format!(
                    "{{\"has_guard\":{},\"has_load\":{},\"module_name\":\"{}\"}}",
                    has_guard, has_load, module_name
                )
            } else {
                String::new()
            }
        };

        // The compiler does not synthesise a component registry — that is the
        // plugin's domain. Pass an empty JSON object; plugins that need a
        // registry collect it themselves through other hooks.
        let registry_json = "{}".to_string();

        let generated = processor
            .process_html(&html, &relative_path, &registry_json, &companion_json)
            .map_err(|e| {
                vec![CompilerError::PluginError {
                    message: format!(
                        "Plugin `{}` failed to process HTML entry `{}`: {}",
                        processor.name(),
                        entry_path.display(),
                        e
                    ),
                    location: None,
                }]
            })?
            .ok_or_else(|| {
                vec![CompilerError::PluginError {
                    message: format!(
                        "Plugin `{}` returned no output for HTML entry `{}`",
                        processor.name(),
                        entry_path.display()
                    ),
                    location: None,
                }]
            })?;

        Ok((processor.name().to_string(), generated))
    }

    // =========================================================================
    // Project Manifest Support
    // =========================================================================

    /// Returns true if the source file is a project manifest: its first line
    /// of code (after blank lines and `//` comments) is `package:`.
    pub(crate) fn is_manifest_file(source: &str) -> bool {
        source
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("//"))
            .is_some_and(|line| line.starts_with("package:"))
    }

    /// BLD-MANIFEST: a `package:` manifest holds build configuration only;
    /// its settings are indented under the `package:` line. A second
    /// top-level line (`start:`, `functions:`, ...) is Clean code the build
    /// would never compile, so it is reported instead of silently dropped.
    fn code_after_manifest_error(source: &str, manifest_path: &Path) -> Option<CompilerError> {
        let (line_index, line) = source
            .lines()
            .enumerate()
            .filter(|(_, line)| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !trimmed.starts_with("//")
            })
            .filter(|(_, line)| !line.starts_with([' ', '\t']))
            .nth(1)?;
        let message = format!(
            "`{}` follows the `package:` manifest. A manifest file holds only build \
             configuration; code after it is never compiled.",
            line.trim()
        );
        let location = SourceLocation {
            line: line_index + 1,
            column: 1,
            file: manifest_path.display().to_string(),
            ..Default::default()
        };
        Some(CompilerError::Validation {
            context: Box::new(
                ErrorContext::new(message, None, ErrorType::Validation, Some(location))
                    .with_error_code("BLD-MANIFEST")
                    .with_suggestion(
                        "Move the code into the module named by `entry:` (or another module \
                         of the project).",
                    ),
            ),
        })
    }

    /// Extract entry: and shared: declarations from a manifest file, then
    /// expand `shared_folders` with every directory that the active plugins
    /// declare ownership of via `[paths].owns` in their plugin.toml.
    ///
    /// The compiler does NOT know any folder names. Ownership of `app/ui/`,
    /// `app/server/`, etc. lives entirely in plugin manifests — this method
    /// reads them off the loaded `PluginRegistry`. The only special folder
    /// path that appears in compiler source is the legacy `app/web/` migration
    /// detector below, which emits a BLD-LAYOUT diagnostic when a project is
    /// still on the pre-frame.ui-2.7 layout.
    fn parse_manifest_info(
        source: &str,
        manifest_dir: &Path,
        plugin_registry: Option<&Arc<PluginRegistry>>,
    ) -> (ManifestInfo, Vec<CompilerError>) {
        let mut info = ManifestInfo::default();
        let mut errors: Vec<CompilerError> = Vec::new();

        for line in source.lines() {
            let trimmed = line.trim();

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

        // Manifest-driven folder discovery (spec §plugin-contract §1.2).
        //
        // Each plugin loaded into the registry has already been selected by
        // the build pipeline based on the manifest's `plugins:` list. Every
        // directory it declares via `[paths].owns` becomes a shared folder
        // for this build, provided the directory actually exists.
        //
        // No folder name is hardcoded here. Plugin authors are the single
        // source of truth for what folders the compiler walks — adding a new
        // owned folder requires only updating the plugin's plugin.toml.
        //
        // Scope-narrowing rule (BRIDGE-HOST-MISMATCH fingerprint fbceaf3d51dd):
        // When the manifest source explicitly declares `shared: [...]`, that
        // list is authoritative. Plugin-owned folders are only added if they
        // are inside (or equal to) at least one manifest-declared entry —
        // sibling folders that a plugin claims via `[paths].owns` but the
        // developer excluded from `shared:` are treated as belonging to a
        // different target (e.g. a browser build) and are NOT compiled into
        // the current target. Without an explicit `shared:` list, plugin-owned
        // auto-scan behaves unchanged (backwards-compatible for projects that
        // rely on the plugin registry as the sole ownership source).
        let manifest_shared_scope: Vec<PathBuf> = info.shared_folders.clone();
        let mut declared_owned: HashSet<String> = HashSet::new();
        if let Some(registry) = plugin_registry {
            // `has_frame_server` is consumed by the assemble hook input
            // (see build_from_file) to flag whether route-registration code
            // should be generated. Derive it from registry state, not by
            // string-matching the manifest source.
            info.has_frame_server = registry.loaded_manifests().contains_key("frame.server");

            for manifest in registry.loaded_manifests().values() {
                for owned in &manifest.paths.owns {
                    declared_owned.insert(owned.trim_end_matches('/').to_string());
                    let dir = manifest_dir.join(owned);
                    if !dir.exists() || info.shared_folders.contains(&dir) {
                        continue;
                    }
                    if !manifest_shared_scope.is_empty() {
                        // Developer narrowed the scope — only accept a
                        // plugin-owned folder if it is inside (or equal to)
                        // one of the manifest's declared shared paths.
                        let in_scope = manifest_shared_scope
                            .iter()
                            .any(|scope| dir == *scope || dir.starts_with(scope));
                        if !in_scope {
                            continue;
                        }
                    }
                    info.shared_folders.push(dir);
                }
            }
        }

        // Migration diagnostic (BLD-LAYOUT).
        //
        // The pre-frame.ui-2.7 layout placed pages under `app/web/pages/`.
        // The new layout (declared by an upgraded frame.ui manifest) nests
        // every render target under `app/ui/`, so the same pages live at
        // `app/ui/web/pages/`. When a project still has files in the legacy
        // location AND the active plugins describe the new layout, emit a
        // hard error directing the developer to reorganize the tree.
        //
        // This is the only place in the compiler where the literal
        // string "app/web" appears: it is the migration anchor, not a
        // discovery rule.
        // BLD-LAYOUT-MIGRATION-ANCHOR: legacy layout detection.
        let legacy_pages = manifest_dir.join("app").join("web").join("pages"); // BLD-LAYOUT-MIGRATION-ANCHOR
        let new_pages = manifest_dir
            .join("app")
            .join("ui")
            .join("web")
            .join("pages"); // BLD-LAYOUT-MIGRATION-ANCHOR
        if legacy_pages.exists() && !new_pages.exists() {
            let plugins_use_new_layout = declared_owned
                .iter()
                .any(|p| p == "app/ui/web" || p.starts_with("app/ui/web/"));
            if plugins_use_new_layout {
                let message = "Folder layout out of date: `app/web/` is no longer recognized.\n\
                     Move:\n  \
                       `app/web/`                              → `app/ui/web/`\n  \
                       `app/ui/` (any non-target content)      → `app/ui/shared/`\n\
                     Update `entry:` in main.cln accordingly. \
                     See clean-framework PROJECT_STRUCTURE.md for the new layout."
                    .to_string();
                errors.push(CompilerError::Validation {
                    context: Box::new(
                        ErrorContext::new(message, None, ErrorType::Validation, None)
                            .with_error_code("BLD-LAYOUT")
                            .with_suggestion("Run `mv app/web app/ui/web` from the project root."),
                    ),
                });
            }
        }

        (info, errors)
    }

    /// Walk every entry in `shared_folders`, gathering `(file_path, shared_dir,
    /// content)` for each `.cln` file, deduplicated by canonical file path.
    ///
    /// `shared_folders` frequently contains overlapping entries because
    /// plugin-declared `[paths].owns` subdirectories (e.g. frame.ui's `app/ui`,
    /// `app/ui/web`, `app/ui/web/pages`) all live under a manifest-declared
    /// `shared:` scope like `[app/ui/]`. Without dedup a single file gets
    /// walked once per ancestor shared_dir and shipped to
    /// `AssembleInput.source_files` N times — reported as
    /// COMPILER-ASSEMBLE-INPUT-DUPLICATE-SOURCE-FILES (fp b2357941).
    ///
    /// First-wins on `shared_dir`: the entry retained for a duplicated file
    /// is the one seen first in `shared_folders` iteration order. That order
    /// puts manifest-declared `shared: [...]` scopes ahead of plugin-owned
    /// subdirs (parents before children within a single scope), so pass 3a's
    /// `derive_companion_module_name` — which uses `shared_dir` to strip the
    /// base prefix — still picks the outermost shared root.
    ///
    /// Content lookup mirrors the pre-dedup Pass 1: prefer the module
    /// already loaded into `unit` (so any downstream edits stay visible)
    /// and only fall back to the on-disk source when the file has not
    /// been discovered as a module yet.
    fn collect_shared_files_deduped(
        shared_folders: &[PathBuf],
        unit: &CompilationUnit,
    ) -> Vec<(PathBuf, PathBuf, String)> {
        let mut shared_files: Vec<(PathBuf, PathBuf, String)> = Vec::new();
        let mut seen_shared: HashSet<PathBuf> = HashSet::new();
        for shared_dir in shared_folders {
            for file_path in Self::collect_cln_files(shared_dir) {
                let canonical = file_path
                    .canonicalize()
                    .unwrap_or_else(|_| file_path.clone());
                if !seen_shared.insert(canonical) {
                    continue;
                }
                let content = if let Some(id) = unit.module_id_for_path(&file_path) {
                    unit.get_module(id).map(|m| m.source.clone())
                } else {
                    fs::read_to_string(&file_path).ok()
                };
                if let Some(content) = content {
                    shared_files.push((file_path, shared_dir.clone(), content));
                }
            }
        }
        shared_files
    }

    /// Non-`.cln` files the loaded plugins declare through `[paths]`: every
    /// file under one of a plugin's `owns` folders (when that folder is part
    /// of this build, i.e. in `shared_folders`) whose name matches one of the
    /// plugin's `patterns`. `.cln` files are already collected as modules.
    /// Sorted by path so the assemble input is deterministic.
    fn collect_plugin_pattern_files(
        registry: &PluginRegistry,
        manifest_dir: &Path,
        shared_folders: &[PathBuf],
    ) -> Vec<(PathBuf, String)> {
        let mut files: Vec<(PathBuf, String)> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut manifests: Vec<_> = registry.loaded_manifests().iter().collect();
        manifests.sort_by(|a, b| a.0.cmp(b.0));
        for (_, manifest) in manifests {
            let patterns: Vec<&str> = manifest
                .paths
                .patterns
                .iter()
                .map(String::as_str)
                .filter(|p| !Self::file_name_matches(p, "x.cln"))
                .collect();
            if patterns.is_empty() {
                continue;
            }
            for owned in &manifest.paths.owns {
                let dir = manifest_dir.join(owned.trim_end_matches('/'));
                if !shared_folders.contains(&dir) {
                    continue;
                }
                for path in Self::collect_files(&dir) {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.ends_with(".cln")
                        || !patterns.iter().any(|p| Self::file_name_matches(p, name))
                    {
                        continue;
                    }
                    let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if !seen.insert(canonical) {
                        continue;
                    }
                    if let Ok(content) = fs::read_to_string(&path) {
                        files.push((path, content));
                    }
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }

    /// Whether `name` matches a `[paths].patterns` entry: a file-name glob in
    /// which `*` stands for any run of characters (`*.html`, `index.*`).
    fn file_name_matches(pattern: &str, name: &str) -> bool {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 1 {
            return pattern == name;
        }
        let (first, last) = (parts[0], parts[parts.len() - 1]);
        if !name.starts_with(first)
            || !name.ends_with(last)
            || name.len() < first.len() + last.len()
        {
            return false;
        }
        let Some(mut rest) = name.get(first.len()..name.len() - last.len()) else {
            return false;
        };
        for middle in &parts[1..parts.len() - 1] {
            match rest.find(middle) {
                Some(at) => rest = &rest[at + middle.len()..],
                None => return false,
            }
        }
        true
    }

    /// Recursively collect every file under a directory.
    fn collect_files(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    files.extend(Self::collect_files(&path));
                } else {
                    files.push(path);
                }
            }
        }
        files
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
                                graph.add_dependency(current_id, dep_id, import.location.clone());

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
        let tokens = lexer.tokenize().map_err(CompilerError::from)?;

        // Stage 2: Parse to AST (use plugin keywords if available)
        let plugin_keywords = if let Some(ref registry) = self.config.plugin_registry {
            registry.get_all_block_keywords()
        } else {
            Vec::new()
        };
        let mut parser = SpecificationParser::with_plugin_keywords(tokens, plugin_keywords);
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
                let (line, column) = import_position(source, &key, import.is_file_import);
                imports.push(ExtractedImport {
                    location: SourceLocation {
                        file: file_path.to_string_lossy().to_string(),
                        line,
                        column,
                        byte_start: None,
                        byte_end: None,
                    },
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

        // Parse in compilation order (dependencies first), with the entry
        // module last. Modules with no import edge between them (component
        // files, page companions) come out of the topological sort in hash
        // order, and the entry's lifecycle slots (client_init, server_init)
        // read the build_state those modules write during their own
        // expansion — so every other module must be expanded first.
        // Nothing imports the entry, so moving it last keeps the order valid.
        let mut order = unit.compilation_order.clone();
        order.sort_by_key(|id| unit.get_module(*id).is_some_and(|m| m.is_entry));
        for module_id in order {
            if let Some(module) = unit.get_module_mut(module_id) {
                let is_entry = module.is_entry;
                match self.parse_module_to_hir(
                    &module.source,
                    &module.file_path,
                    is_entry,
                    module.plugin_expansion.clone(),
                ) {
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
        plugin_expansion: Option<ModulePluginExpansion>,
    ) -> Result<HirProgram, CompilerError> {
        // Stage 1: Tokenize
        let source_code =
            SourceCode::new(source.to_string(), file_path.to_string_lossy().to_string());
        let mut lexer = SpecificationLexer::new(&source_code);
        let tokens = lexer.tokenize().map_err(CompilerError::from)?;

        // Stage 2: Parse to AST
        // Get plugin keywords so the parser recognizes plugin-defined syntax
        let plugin_keywords = if let Some(ref registry) = self.config.plugin_registry {
            registry.get_all_block_keywords()
        } else {
            Vec::new()
        };
        let mut parser = SpecificationParser::with_plugin_keywords(tokens, plugin_keywords);
        let mut parsed_ast = parser.parse_program()?;

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
        let mut ast = if let Some(ref registry) = self.config.plugin_registry {
            tracing::debug!(
                file = %file_path.display(),
                is_entry = is_entry,
                "Starting Stage 2.5: Plugin Expansion for module"
            );
            // Plugin Contracts v2 — pass build-target flags to the expander so
            // it dispatches only the lifecycle slots that match the build
            // shape. See contracts/lifecycle.md §3.3, §3.4.
            let mut expander = PluginExpander::new(registry.as_ref())
                .with_build_target(!self.config.client_mode, self.config.client_mode)
                .with_build_context(
                    crate::plugins::BuildContext::new()
                        .with_entry(file_path.to_string_lossy().to_string()),
                );
            // AST the plugin emitted for the module as a whole (an HTML page
            // entry processed by a typed-emission plugin) is folded in like a
            // top-level framework block before the program is expanded.
            if let Some(module_expansion) = plugin_expansion {
                expander.absorb_entry_expansion(
                    &mut parsed_ast,
                    &module_expansion.plugin_name,
                    module_expansion.expansion,
                );
            }
            let expand_result = if is_entry {
                expander.expand_program(parsed_ast)
            } else {
                expander.expand_program_without_preambles(parsed_ast)
            };
            let expanded = expand_result.map_err(|e| {
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
            })?;
            expander.check_permissions()?;
            expanded
        } else {
            parsed_ast
        };

        // HYDRATE_AUTO Gap 2 — emit bare-named top-level dispatch shims for
        // any class method declared inside an `events:` block. Runs whether
        // or not plugins are active because `events:` is now a first-class
        // class section (foundation/spec/plugins/frame-ui.ebnf §events_section)
        // and a class with event handlers needs its shims regardless of
        // whether the class came from user code or plugin expansion. When
        // the expander already ran, this is the second invocation and is a
        // no-op (shims already emitted are detected by name collision and
        // skipped).
        crate::plugins::expander::emit_event_handler_shims(&mut ast).map_err(|e| {
            CompilerError::syntax_error(
                e.to_string(),
                Some("Event handler export conflict".to_string()),
                Some(SourceLocation {
                    file: file_path.to_string_lossy().to_string(),
                    line: 1,
                    column: 1,
                    byte_start: None,
                    byte_end: None,
                }),
            )
        })?;

        // Stage 3: Build HIR
        let generic_instances = self
            .config
            .plugin_registry
            .as_ref()
            .map(|r| r.generic_instance_classes())
            .unwrap_or_default();
        let mut hir_builder = HirBuilder::new().with_generic_instance_classes(generic_instances);
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
    use crate::plugins::builtin_assemblers::derive_companion_module_name;

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

    #[test]
    fn test_derive_companion_module_name() {
        // The canonical anchor is the `pages/` segment in the path. Every result
        // is prefixed `pages_` to match frame.ui's `derive_module_name` so
        // the compiler's call sites (which feed the result into
        // `process_html`'s `companion_json`) agree with what frame.ui's
        // `assemble` later names the renamed function. See
        // SEM-PAGE-COMPANION-NAMING-DRIFT for what happens when they don't.
        assert_eq!(
            derive_companion_module_name(Path::new("app/pages/dashboard.cln")),
            "pages_dashboard"
        );
        assert_eq!(
            derive_companion_module_name(Path::new("app/pages/blog/post.cln")),
            "pages_blog_post"
        );
        assert_eq!(
            derive_companion_module_name(Path::new("app/pages/blog/[slug].cln")),
            "pages_blog_slug"
        );
        // The real-world layout the page-project hang surfaced: the
        // historical name was `app_ui_web_pages_home`, which the plugin
        // never defined, so the synthesized call to `<name>_load_impl`
        // SEM007'd. The new shape is `pages_home`, matching frame.ui.
        assert_eq!(
            derive_companion_module_name(Path::new("app/ui/web/pages/home.cln")),
            "pages_home"
        );
        assert_eq!(
            derive_companion_module_name(Path::new("app/ui/web/pages/blog/index.cln")),
            "pages_blog_index"
        );
        // Identifier-invalid characters in the page name (frame.ui sanitises
        // these in its `derive_module_name` so the rewritten function name
        // is a valid Clean identifier; we must do the same).
        assert_eq!(
            derive_companion_module_name(Path::new("app/pages/my-page.cln")),
            "pages_my_page"
        );
    }

    // =========================================================================
    // Manifest-driven folder discovery tests
    //
    // These tests assert that `parse_manifest_info` reads every active plugin's
    // `[paths].owns` list out of the registry and includes the directories it
    // points to. The compiler hardcodes NO folder names — the tests therefore
    // build synthetic plugin manifests in-memory and verify that whatever those
    // manifests declare ends up in `shared_folders`.
    // =========================================================================

    /// Build a synthetic plugin registry with the given (plugin_name, owns_paths)
    /// pairs. Each manifest is otherwise minimal — only `plugin.paths.owns`
    /// matters for folder discovery.
    fn registry_with_owns(entries: &[(&str, &[&str])]) -> Arc<crate::plugins::PluginRegistry> {
        let with_patterns: Vec<(&str, &[&str], &[&str])> = entries
            .iter()
            .map(|(name, owns)| (*name, *owns, &[][..]))
            .collect();
        registry_with_paths(&with_patterns)
    }

    /// Like [`registry_with_owns`], with each plugin's `[paths].patterns`.
    fn registry_with_paths(
        entries: &[(&str, &[&str], &[&str])],
    ) -> Arc<crate::plugins::PluginRegistry> {
        use crate::plugins::plugin_abi::{
            PluginCompatibility, PluginHandles, PluginInfo, PluginManifest, PluginPaths,
        };

        let mut builder = crate::plugins::PluginRegistry::builder()
            .with_validation_policy(crate::plugins::registry_loader::ValidationPolicy::Off);
        for (name, owns, patterns) in entries {
            let manifest = PluginManifest {
                plugin: PluginInfo {
                    name: (*name).to_string(),
                    version: "1.0.0".to_string(),
                    description: "test".to_string(),
                    author: "test".to_string(),
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
                paths: PluginPaths {
                    owns: owns.iter().map(|s| (*s).to_string()).collect(),
                    patterns: patterns.iter().map(|s| (*s).to_string()).collect(),
                },
                enforcement: Default::default(),
                memory: Default::default(),
                build: Default::default(),
                lifecycle: Default::default(),
                artifacts: Vec::new(),
                blocks: Default::default(),
            };
            builder = builder.add_manifest((*name).to_string(), manifest);
        }
        Arc::new(builder.build().expect("build registry"))
    }

    #[test]
    fn test_parse_manifest_info_uses_plugin_paths_owns() {
        // A plugin that declares ownership of app/ui/web should cause that
        // directory to appear in shared_folders without any folder name being
        // hardcoded in the compiler.
        let manifest = "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui]\n\t\tentry: app/ui/web/pages/index.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();

        let owned = manifest_dir.join("app").join("ui").join("web");
        std::fs::create_dir_all(owned.join("pages")).unwrap();

        let registry = registry_with_owns(&[("frame.ui", &["app/ui/web"])]);
        let (info, errors) =
            MultiFileCompiler::parse_manifest_info(manifest, manifest_dir, Some(&registry));

        assert!(errors.is_empty(), "no migration diagnostics expected");
        assert!(
            info.shared_folders.contains(&owned),
            "directory declared in [paths].owns should be added to shared_folders"
        );
    }

    #[test]
    fn test_parse_manifest_info_skips_undeclared_directories() {
        // A directory that no loaded plugin declares ownership of must NOT be
        // auto-included, even if it exists on disk. This is the spec-parity
        // check: discovery is manifest-driven, not filesystem-driven.
        let manifest = "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui]\n\t\tentry: app/ui/web/pages/index.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();

        std::fs::create_dir_all(
            manifest_dir
                .join("app")
                .join("ui")
                .join("web")
                .join("pages"),
        )
        .unwrap();
        // Create an unrelated directory the plugin does NOT own.
        let unowned = manifest_dir.join("app").join("server");
        std::fs::create_dir_all(&unowned).unwrap();

        let registry = registry_with_owns(&[("frame.ui", &["app/ui/web"])]);
        let (info, errors) =
            MultiFileCompiler::parse_manifest_info(manifest, manifest_dir, Some(&registry));

        assert!(errors.is_empty());
        assert!(
            !info.shared_folders.contains(&unowned),
            "directories not declared in any plugin's [paths].owns must not be auto-included"
        );
    }

    #[test]
    fn test_parse_manifest_info_no_registry_no_auto_shared() {
        // Without a plugin registry there is no source of folder ownership,
        // so nothing is auto-discovered beyond explicit `shared:` declarations.
        let manifest = "package: Test\n\tentry: src/main.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();
        std::fs::create_dir_all(manifest_dir.join("src")).unwrap();

        let (info, errors) = MultiFileCompiler::parse_manifest_info(manifest, manifest_dir, None);

        assert!(errors.is_empty());
        assert!(
            info.shared_folders.is_empty(),
            "without plugins, no auto-shared folder"
        );
    }

    #[test]
    fn test_parse_manifest_info_has_frame_server_flag_tracks_registry() {
        // The `has_frame_server` flag (consumed by the assemble hook) must be
        // true iff the registry has loaded `frame.server` — derived from
        // registry state, not from string-matching the manifest text.
        let manifest = "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui, frame.server]\n\t\tentry: app/ui/web/pages/index.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();
        std::fs::create_dir_all(
            manifest_dir
                .join("app")
                .join("ui")
                .join("web")
                .join("pages"),
        )
        .unwrap();
        std::fs::create_dir_all(manifest_dir.join("app").join("server")).unwrap();

        let registry_with = registry_with_owns(&[
            ("frame.ui", &["app/ui/web"]),
            ("frame.server", &["app/server"]),
        ]);
        let (info_with, _) =
            MultiFileCompiler::parse_manifest_info(manifest, manifest_dir, Some(&registry_with));
        assert!(info_with.has_frame_server);

        let registry_without = registry_with_owns(&[("frame.ui", &["app/ui/web"])]);
        let (info_without, _) =
            MultiFileCompiler::parse_manifest_info(manifest, manifest_dir, Some(&registry_without));
        assert!(!info_without.has_frame_server);
    }

    #[test]
    fn test_parse_manifest_info_bld_layout_fires_on_legacy_tree() {
        // Project still has app/web/pages/ on disk; active plugins describe
        // the new app/ui/web/ layout. Expected: BLD-LAYOUT migration error.
        let manifest = "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui]\n\t\tentry: app/web/pages/index.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();
        std::fs::create_dir_all(manifest_dir.join("app").join("web").join("pages")).unwrap();

        let registry = registry_with_owns(&[("frame.ui", &["app/ui/web", "app/ui/web/pages"])]);
        let (_info, errors) =
            MultiFileCompiler::parse_manifest_info(manifest, manifest_dir, Some(&registry));

        let has_bld_layout = errors.iter().any(|e| match e {
            CompilerError::Validation { context } => {
                context.error_code.as_deref() == Some("BLD-LAYOUT")
            }
            _ => false,
        });
        assert!(
            has_bld_layout,
            "expected BLD-LAYOUT diagnostic when project is on legacy layout but plugins use new layout"
        );
    }

    #[test]
    fn test_parse_manifest_info_bld_layout_silent_on_current_layout() {
        // Project on legacy layout, plugins also on legacy layout → no migration.
        // This is the today-case: shipping the manifest-driven discovery without
        // breaking projects that haven't updated yet.
        let manifest = "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui]\n\t\tentry: app/web/pages/index.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();
        std::fs::create_dir_all(manifest_dir.join("app").join("web").join("pages")).unwrap();

        let registry = registry_with_owns(&[("frame.ui", &["app/ui", "app/web", "app/web/pages"])]);
        let (_info, errors) =
            MultiFileCompiler::parse_manifest_info(manifest, manifest_dir, Some(&registry));

        let has_bld_layout = errors.iter().any(|e| match e {
            CompilerError::Validation { context } => {
                context.error_code.as_deref() == Some("BLD-LAYOUT")
            }
            _ => false,
        });
        assert!(
            !has_bld_layout,
            "BLD-LAYOUT must not fire when active plugins still claim ownership of the legacy paths"
        );
    }

    #[test]
    fn test_parse_manifest_info_second_plugin_picked_up_without_compiler_change() {
        // Regression guard for §D in the cross-component prompt:
        // a never-before-seen folder declared by a plugin's [paths].owns
        // must be discovered with zero compiler changes.
        let manifest = "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui]\n\t\tentry: app/ui/web/pages/index.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();

        let fictional = manifest_dir.join("app").join("ui").join("desktop");
        std::fs::create_dir_all(&fictional).unwrap();
        std::fs::create_dir_all(
            manifest_dir
                .join("app")
                .join("ui")
                .join("web")
                .join("pages"),
        )
        .unwrap();

        let registry = registry_with_owns(&[("frame.ui", &["app/ui/web", "app/ui/desktop"])]);
        let (info, errors) =
            MultiFileCompiler::parse_manifest_info(manifest, manifest_dir, Some(&registry));

        assert!(errors.is_empty());
        assert!(
            info.shared_folders.contains(&fictional),
            "a folder declared by a plugin's manifest must be auto-discovered without compiler changes"
        );
    }

    #[test]
    fn test_manifest_shared_narrows_plugin_owned_scope() {
        // BRIDGE-HOST-MISMATCH fingerprint fbceaf3d51dd:
        // When the manifest explicitly declares `shared: [<path>]`, plugin-owned
        // folders that live OUTSIDE that declared scope must not be pulled into
        // the compilation unit — even if a loaded plugin declares them via
        // `[paths].owns`. The developer's `shared:` list is authoritative when
        // present; without it, plugin-owned auto-discovery still runs unchanged.
        //
        // Repro: server target with `shared: [app/ui/web/]` — files under
        // `app/ui/components/` (which is a descendant of frame.ui's owned
        // `app/ui` but NOT inside the narrower `app/ui/web/`) must not be
        // compiled into the server WASM, so their browser-only bridge calls
        // (e.g. `_ui_set_timeout`) don't trip host-class validation.
        let manifest = "package: Test\n\tshared: [app/ui/web]\n\ttarget: server\n\t\tplugins: [frame.ui, frame.server]\n\t\tentry: app/server/main.cln\n";
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();

        std::fs::create_dir_all(manifest_dir.join("app").join("server")).unwrap();
        let inside_scope = manifest_dir.join("app").join("ui").join("web");
        std::fs::create_dir_all(inside_scope.join("pages")).unwrap();
        let outside_scope = manifest_dir.join("app").join("ui").join("components");
        std::fs::create_dir_all(&outside_scope).unwrap();

        // frame.ui declares ownership of the whole `app/ui` subtree. Without
        // narrowing, that would sweep `app/ui/components/` into the build.
        let registry = registry_with_owns(&[
            (
                "frame.ui",
                &[
                    "app/ui",
                    "app/ui/shared",
                    "app/ui/web",
                    "app/ui/web/pages",
                    "app/ui/web/components",
                    "app/ui/web/layouts",
                ],
            ),
            ("frame.server", &["app/server"]),
        ]);
        let (info, errors) =
            MultiFileCompiler::parse_manifest_info(manifest, manifest_dir, Some(&registry));

        assert!(errors.is_empty(), "no migration diagnostics expected");
        assert!(
            info.shared_folders.contains(&inside_scope)
                || info
                    .shared_folders
                    .iter()
                    .any(|p| inside_scope.starts_with(p)),
            "the manifest's declared shared path must remain in scope"
        );
        assert!(
            !info.shared_folders.iter().any(|p| p == &outside_scope
                || outside_scope.starts_with(p) && *p != manifest_dir.to_path_buf()),
            "a plugin-owned folder outside the manifest's declared `shared:` scope \
             (here: app/ui/components/, inside frame.ui's owned `app/ui`) must not \
             be added when the manifest explicitly narrows the scope"
        );
    }

    #[test]
    fn test_collect_shared_files_deduped_drops_ancestor_walk_duplicates() {
        // Regression: COMPILER-ASSEMBLE-INPUT-DUPLICATE-SOURCE-FILES
        // (fingerprint b2357941). When `shared_folders` contains overlapping
        // entries — the manifest's `shared: [app/ui/]` scope AND the plugin's
        // owned `app/ui/web`, `app/ui/web/pages` subdirs (frame.ui does
        // exactly this) — a single .cln file used to appear in `shared_files`
        // once per ancestor shared_dir. `AssembleInput.source_files` then
        // shipped duplicates to plugin assemble hooks; a 4-file project
        // reported 3 copies of one page file.
        //
        // The fix (`collect_shared_files_deduped`) dedupes by canonical path
        // and keeps the first (outermost) `shared_dir` binding so pass 3a's
        // `derive_companion_module_name` still strips the manifest-declared
        // base prefix, not a plugin-owned inner subdir.
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path();
        let pages = manifest_dir
            .join("app")
            .join("ui")
            .join("web")
            .join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("index.cln"),
            "functions:\n\tany load(Request r)\n\t\treturn { title: \"Home\" }\n",
        )
        .unwrap();

        // Simulate the shared_folders vector that `parse_manifest_info`
        // builds when frame.ui's `[paths].owns` intersects manifest
        // `shared: [app/ui/]` — the manifest scope plus every owned
        // subdirectory that lives inside it. Nesting order matches
        // production ordering (parent first, then children).
        let shared_folders = vec![
            manifest_dir.join("app").join("ui"),
            manifest_dir.join("app").join("ui").join("web"),
            manifest_dir
                .join("app")
                .join("ui")
                .join("web")
                .join("pages"),
        ];

        // A compilation unit is required for the module-vs-disk content
        // preference. An entry file that does not overlap the walk is
        // sufficient — the shared walk falls back to `fs::read_to_string`
        // for files it does not find as loaded modules.
        let unit = CompilationUnit::new(
            "entry".to_string(),
            manifest_dir.join("entry.cln"),
            "start:\n\tprint(1)\n".to_string(),
        );

        let shared_files = MultiFileCompiler::collect_shared_files_deduped(&shared_folders, &unit);

        assert_eq!(
            shared_files.len(),
            1,
            "duplicate walks over the same file across overlapping shared_folders \
             must dedup by canonical path (found {} entries: {:?})",
            shared_files.len(),
            shared_files
                .iter()
                .map(|(p, _, _)| p.display().to_string())
                .collect::<Vec<_>>()
        );

        // First-wins policy: outermost shared_dir (`app/ui`) is retained,
        // not the innermost (`app/ui/web/pages`). This preserves the
        // module-name derivation contract for `derive_companion_module_name`.
        let (_, shared_dir, _) = &shared_files[0];
        assert_eq!(
            shared_dir,
            &manifest_dir.join("app").join("ui"),
            "first-seen shared_dir must be retained on dedup (got {:?})",
            shared_dir
        );
    }

    /// Architecture-check regression test (cross-component prompt §D).
    ///
    /// The compiler's file-discovery code path must not contain literal folder
    /// names — those belong in plugin manifests. The only allowed string
    /// literals in `multi_file_compiler.rs` related to the legacy layout are
    /// the BLD-LAYOUT migration anchor strings (the literal `"app/web"`
    /// directory path used to detect the legacy layout), which are explicitly
    /// scoped to the migration diagnostic.
    /// Every file a plugin's `[paths].patterns` match in a folder it owns —
    /// here `.html` pages — reaches `assemble`; `.cln` files are modules and
    /// folders outside the build are skipped.
    #[test]
    fn test_assemble_input_includes_plugin_pattern_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pages = root.join("app/ui/web/pages");
        std::fs::create_dir_all(pages.join("about")).unwrap();
        std::fs::write(pages.join("about/index.html"), "<p>about</p>").unwrap();
        std::fs::write(pages.join("home.cln"), "functions:\n").unwrap();
        std::fs::write(pages.join("notes.txt"), "x").unwrap();
        let other = root.join("app/other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("skip.html"), "<p>skip</p>").unwrap();
        let registry = registry_with_paths(&[(
            "frame.test",
            &["app/ui/web/pages", "app/other"],
            &["*.cln", "*.html"],
        )]);
        let files = MultiFileCompiler::collect_plugin_pattern_files(
            &registry,
            root,
            std::slice::from_ref(&pages),
        );
        assert_eq!(
            files.len(),
            1,
            "only the page under the built folder: {:?}",
            files
        );
        assert!(files[0].0.ends_with("about/index.html"));
        assert_eq!(files[0].1, "<p>about</p>");
    }

    #[test]
    fn test_plugin_file_patterns_match_file_names() {
        assert!(MultiFileCompiler::file_name_matches("*.html", "index.html"));
        assert!(MultiFileCompiler::file_name_matches("*.html", "a.b.html"));
        assert!(!MultiFileCompiler::file_name_matches("*.html", "index.cln"));
        assert!(!MultiFileCompiler::file_name_matches("*.html", "html"));
        assert!(MultiFileCompiler::file_name_matches("index.*", "index.css"));
        assert!(MultiFileCompiler::file_name_matches("a*b*c", "axxbyyc"));
        assert!(!MultiFileCompiler::file_name_matches("a*b*c", "axxc"));
        assert!(MultiFileCompiler::file_name_matches(
            "theme.css",
            "theme.css"
        ));
    }

    /// BLD-MANIFEST: code written after a `package:` manifest was dropped
    /// without a diagnostic.
    #[test]
    fn test_code_after_manifest_is_reported() {
        let manifest = "// project\npackage: T1\n\tversion: \"1.0.0\"\n\n\ttarget: browser\n\t\tentry: index.html\n\nstart:\n\tprintl(\"hello\")\n";
        let error = MultiFileCompiler::code_after_manifest_error(manifest, Path::new("main.cln"))
            .expect("code after the manifest must be reported");
        let text = error.to_string();
        assert!(text.contains("start:"), "message names the line: {}", text);
        let location = error
            .context()
            .and_then(|c| c.location.clone())
            .expect("the error carries a location");
        assert_eq!(location.line, 8);

        let plain =
            "package: T1\n\tversion: \"1.0.0\"\n\t// note\n\ttarget: server\n\t\tentry: app.cln\n";
        assert!(
            MultiFileCompiler::code_after_manifest_error(plain, Path::new("main.cln")).is_none(),
            "a manifest with only indented settings is valid"
        );
    }

    #[test]
    fn test_no_hardcoded_folder_names_in_discovery() {
        // Forbidden literal segments. These names should ONLY come from plugin
        // manifests, never from compiler source.
        const FORBIDDEN: &[&str] = &[
            "\"pages\"",
            "\"server\"",
            "\"api\"",
            "\"models\"",
            "\"middleware\"",
            "\"layouts\"",
            "\"components\"",
            "\"migrations\"",
            "\"seeds\"",
        ];

        let src = include_str!("multi_file_compiler.rs");

        // Strip the #[cfg(test)] module — fixtures and assertions legitimately
        // contain these literals and would create false positives. The scan
        // targets only the production discovery code.
        let production_src = src.split("#[cfg(test)]").next().unwrap_or(src);

        // Lines tagged `BLD-LAYOUT-MIGRATION-ANCHOR` are the deliberately-scoped
        // legacy path strings used by the migration diagnostic per prompt §B —
        // the only allowed appearance of folder names in production code.
        let scanned: String = production_src
            .lines()
            .filter(|line| !line.contains("BLD-LAYOUT-MIGRATION-ANCHOR"))
            .collect::<Vec<_>>()
            .join("\n");

        for needle in FORBIDDEN {
            assert!(
                !scanned.contains(needle),
                "compiler production code contains hardcoded folder name `{}`; \
                 folder ownership must come from plugin manifests ([paths].owns), \
                 not compiler source",
                needle
            );
        }
    }

    // Deleted: test_page_companion_load_functions_are_prefixed_to_avoid_collision
    // — depended on the in-tree PageCompanionAssembler Rust shim. Page companion
    // prefixing now lives in frame.ui's WASM `assemble` export; the equivalent
    // regression coverage belongs in clean-framework/plugins/frame.ui/tests.

    // Deleted: test_build_from_file_generates_route_module_for_page_companions
    // — asserted the shim emitted `_http_route` calls in a synthetic module.
    // Route registration now flows through frame.server's WASM assemble path.
    // Equivalent end-to-end coverage belongs in the framework's integration
    // tests, not in compiler unit tests.

    #[test]
    fn test_build_from_file_no_route_module_without_frame_server() {
        // When frame.server is NOT declared, no synthetic route module should be generated.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let manifest_source = concat!(
            "package: TestApp\n",
            "\tversion: \"1.0.0\"\n",
            "\ttarget: web\n",
            "\t\tplugins: [frame.ui]\n",
            "\t\tentry: app/web/pages/home.cln\n",
        );
        std::fs::write(root.join("main.cln"), manifest_source).unwrap();

        let pages_dir = root.join("app").join("web").join("pages");
        std::fs::create_dir_all(&pages_dir).unwrap();

        std::fs::write(
            pages_dir.join("home.cln"),
            "functions:\n\tany load(string req)\n\t\treturn \"Home\"\n",
        )
        .unwrap();

        let compiler = MultiFileCompiler::new();
        let unit = compiler
            .build_from_file(root.join("main.cln"))
            .expect("build_from_file should succeed without frame.server");

        let has_synthetic = unit.modules.values().any(|m| {
            m.file_path
                .to_string_lossy()
                .contains("__page_routes_generated")
        });

        assert!(
            !has_synthetic,
            "should NOT generate __page_routes_generated module when frame.server is absent"
        );
    }
}
