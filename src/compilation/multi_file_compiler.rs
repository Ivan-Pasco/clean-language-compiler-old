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
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration for the multi-file compiler
#[derive(Debug, Clone)]
pub struct MultiFileCompilerConfig {
    /// Search paths for finding imported modules
    pub search_paths: Vec<PathBuf>,

    /// Optimization level (0-3)
    pub opt_level: u8,

    /// Whether to include debug information
    pub debug: bool,
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
        };

        let compiler = MultiFileCompiler::with_config(config);
        let result = compiler.build_from_file(entry_path);

        // Clean up temp directory after use
        let _ = fs::remove_dir_all(&temp_dir);

        result
    }

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
                    // Update the module with its imports
                    if let Some(m) = unit.get_module_mut(current_id) {
                        m.set_imports(imports.clone());
                    }

                    // Process each import
                    for import_name in imports {
                        // Skip standard library modules (they're built-in)
                        if Self::is_builtin_module(&import_name) {
                            continue;
                        }

                        // Find the module file
                        match self.find_module_file(&import_name, search_paths) {
                            Ok((module_path, module_source)) => {
                                // Add the module if not already present
                                let dep_id = if !unit.has_module(&import_name) {
                                    unit.add_module(
                                        import_name.clone(),
                                        module_path.clone(),
                                        module_source,
                                    )
                                } else {
                                    *unit.module_by_name.get(&import_name).unwrap()
                                };

                                // Add to graph
                                graph.add_dependency(
                                    current_id,
                                    dep_id,
                                    SourceLocation {
                                        file: file_path.to_string_lossy().to_string(),
                                        line: 1,
                                        column: 1,
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

    /// Extract import statements from source code
    ///
    /// This does a quick parse to find import declarations without full AST building
    fn extract_imports(
        &self,
        source: &str,
        file_path: &Path,
    ) -> Result<Vec<String>, CompilerError> {
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
                }),
            )
        })?;

        // Stage 2: Parse to AST
        let mut parser = SpecificationParser::new(tokens, file_path.to_string_lossy().to_string());
        let ast = parser.parse_program()?;

        // Extract import names from AST
        let imports: Vec<String> = ast
            .imports
            .iter()
            .map(|import| {
                // Handle qualified imports like "Math.sqrt" - extract the module name
                let name = &import.name;
                if let Some(dot_pos) = name.find('.') {
                    name[..dot_pos].to_string()
                } else {
                    name.clone()
                }
            })
            .collect::<HashSet<_>>() // Deduplicate
            .into_iter()
            .collect();

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

    /// Parse a single module through Stages 1-3
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
        let mut parser = SpecificationParser::new(tokens, file_path.to_string_lossy().to_string());
        let ast = parser.parse_program()?;

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
            r#"start()
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

start()
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

start()
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
}
