//! Module Resolution for Clean Language
//!
//! This module handles the resolution of import statements and module dependencies.
//! It provides functionality to discover, load, and resolve symbols from imported modules.

use super::*;
use crate::error::CompilerError;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Module resolver for handling imports and module dependencies
pub struct ModuleResolver {
    /// Loaded modules keyed by module name
    modules: HashMap<String, LoadedModule>,

    /// Module search paths
    search_paths: Vec<PathBuf>,

    /// Current module being processed
    current_module: Option<String>,

    /// Module dependency graph for cycle detection
    dependencies: HashMap<String, HashSet<String>>,
}

/// Information about a loaded module
#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub name: String,
    pub file_path: PathBuf,
    pub hir: Option<HirProgram>, // None if not yet loaded
    pub exports: HashMap<String, SymbolId>,
    pub dependencies: Vec<String>,
}

/// Module resolution result
#[derive(Debug)]
pub struct ModuleResolution {
    pub resolved_modules: HashMap<String, LoadedModule>,
    pub dependency_order: Vec<String>,
    pub errors: Vec<CompilerError>,
}

impl ModuleResolver {
    /// Create a new module resolver
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            search_paths: vec![
                PathBuf::from("./"),
                PathBuf::from("./lib/"),
                PathBuf::from("./stdlib/"),
            ],
            current_module: None,
            dependencies: HashMap::new(),
        }
    }

    /// Add a search path for modules
    pub fn add_search_path<P: AsRef<Path>>(&mut self, path: P) {
        self.search_paths.push(path.as_ref().to_path_buf());
    }

    /// Set the current module being processed
    pub fn set_current_module(&mut self, module_name: String) {
        self.current_module = Some(module_name);
    }

    /// Resolve all imports for a given HIR program
    pub fn resolve_imports(
        &mut self,
        hir: &HirProgram,
    ) -> Result<ModuleResolution, Vec<CompilerError>> {
        let mut errors = Vec::new();

        // Collect all import statements
        let imports = &hir.imports;

        // Resolve each import
        for import in imports {
            if let Err(mut import_errors) = self.resolve_import(import) {
                errors.append(&mut import_errors);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        // Check for circular dependencies
        if let Err(cycle_error) = self.check_circular_dependencies() {
            errors.push(cycle_error);
            return Err(errors);
        }

        // Determine dependency order
        let dependency_order = self.topological_sort()?;

        Ok(ModuleResolution {
            resolved_modules: self.modules.clone(),
            dependency_order,
            errors,
        })
    }

    /// Resolve a single import statement
    fn resolve_import(&mut self, import: &HirImport) -> Result<(), Vec<CompilerError>> {
        let module_name = &import.module_name;

        // Check if module is already loaded
        if self.modules.contains_key(module_name) {
            return Ok(());
        }

        // Find module file
        let module_path = self.find_module_file(module_name).ok_or_else(|| {
            vec![CompilerError::validation_error(
                &format!("Module '{}' not found in search paths", module_name),
                import.location.clone(),
            )]
        })?;

        // Create loaded module entry
        let loaded_module = LoadedModule {
            name: module_name.clone(),
            file_path: module_path,
            hir: None,                // Will be loaded when needed
            exports: HashMap::new(),  // Will be populated when HIR is loaded
            dependencies: Vec::new(), // Will be populated when HIR is loaded
        };

        self.modules.insert(module_name.clone(), loaded_module);

        // Add dependency relationship
        if let Some(current) = &self.current_module {
            self.dependencies
                .entry(current.clone())
                .or_insert_with(HashSet::new)
                .insert(module_name.clone());
        }

        Ok(())
    }

    /// Find a module file in the search paths
    fn find_module_file(&self, module_name: &str) -> Option<PathBuf> {
        let possible_filenames = vec![
            format!("{}.cln", module_name),
            format!("{}/mod.cln", module_name),
            format!("{}/index.cln", module_name),
        ];

        for search_path in &self.search_paths {
            for filename in &possible_filenames {
                let full_path = search_path.join(filename);
                if full_path.exists() {
                    return Some(full_path);
                }
            }
        }

        None
    }

    /// Check for circular dependencies in the module graph
    fn check_circular_dependencies(&self) -> Result<(), CompilerError> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for module in self.dependencies.keys() {
            if !visited.contains(module) {
                if self.has_cycle(module, &mut visited, &mut rec_stack) {
                    return Err(CompilerError::validation_error(
                        &format!("Circular dependency detected involving module '{}'", module),
                        crate::ast::SourceLocation {
                            file: "<module_resolver>".to_string(),
                            line: 0,
                            column: 0,
                        },
                    ));
                }
            }
        }

        Ok(())
    }

    /// DFS helper for cycle detection
    fn has_cycle(
        &self,
        module: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> bool {
        visited.insert(module.to_string());
        rec_stack.insert(module.to_string());

        if let Some(deps) = self.dependencies.get(module) {
            for dep in deps {
                if !visited.contains(dep) {
                    if self.has_cycle(dep, visited, rec_stack) {
                        return true;
                    }
                } else if rec_stack.contains(dep) {
                    return true;
                }
            }
        }

        rec_stack.remove(module);
        false
    }

    /// Perform topological sort to determine module loading order
    fn topological_sort(&self) -> Result<Vec<String>, Vec<CompilerError>> {
        let mut visited = HashSet::new();
        let mut temp_visited = HashSet::new();
        let mut result = Vec::new();

        for module in self.modules.keys() {
            if !visited.contains(module) {
                if let Err(error) = self.topological_sort_visit(
                    module,
                    &mut visited,
                    &mut temp_visited,
                    &mut result,
                ) {
                    return Err(vec![error]);
                }
            }
        }

        // Result is already in correct order: dependencies come before dependents
        Ok(result)
    }

    /// DFS helper for topological sort
    fn topological_sort_visit(
        &self,
        module: &str,
        visited: &mut HashSet<String>,
        temp_visited: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) -> Result<(), CompilerError> {
        if temp_visited.contains(module) {
            return Err(CompilerError::validation_error(
                &format!("Circular dependency detected at module '{}'", module),
                crate::ast::SourceLocation {
                    file: "<module_resolver>".to_string(),
                    line: 0,
                    column: 0,
                },
            ));
        }

        if visited.contains(module) {
            return Ok(());
        }

        temp_visited.insert(module.to_string());

        if let Some(deps) = self.dependencies.get(module) {
            for dep in deps {
                self.topological_sort_visit(dep, visited, temp_visited, result)?;
            }
        }

        temp_visited.remove(module);
        visited.insert(module.to_string());
        result.push(module.to_string());

        Ok(())
    }

    /// Load HIR for a specific module (placeholder - would read and parse file)
    pub fn load_module_hir(&mut self, module_name: &str) -> Result<&HirProgram, CompilerError> {
        if let Some(module) = self.modules.get_mut(module_name) {
            if module.hir.is_none() {
                // In a real implementation, this would:
                // 1. Read the file from module.file_path
                // 2. Parse it using the specification parser
                // 3. Build HIR using the HIR builder
                // 4. Store the result in module.hir

                // For now, return an error indicating this is not implemented
                return Err(CompilerError::validation_error(
                    &format!("Module loading not yet implemented for '{}'", module_name),
                    crate::ast::SourceLocation {
                        file: module.file_path.to_string_lossy().to_string(),
                        line: 0,
                        column: 0,
                    },
                ));
            }

            Ok(module.hir.as_ref().unwrap())
        } else {
            Err(CompilerError::validation_error(
                &format!("Module '{}' not found", module_name),
                crate::ast::SourceLocation {
                    file: "<module_resolver>".to_string(),
                    line: 0,
                    column: 0,
                },
            ))
        }
    }

    /// Get exports from a module
    pub fn get_module_exports(&self, module_name: &str) -> Option<&HashMap<String, SymbolId>> {
        self.modules.get(module_name).map(|m| &m.exports)
    }

    /// Check if a module exists
    pub fn has_module(&self, module_name: &str) -> bool {
        self.modules.contains_key(module_name)
    }

    /// Get all loaded modules
    pub fn get_loaded_modules(&self) -> &HashMap<String, LoadedModule> {
        &self.modules
    }
}

impl Default for ModuleResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::SourceLocation;

    #[allow(dead_code)]
    fn test_location() -> SourceLocation {
        SourceLocation {
            file: "test.cln".to_string(),
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_module_resolver_creation() {
        let resolver = ModuleResolver::new();
        assert!(resolver.modules.is_empty());
        assert!(!resolver.search_paths.is_empty());
    }

    #[test]
    fn test_search_path_management() {
        let mut resolver = ModuleResolver::new();
        let initial_count = resolver.search_paths.len();

        resolver.add_search_path("./custom");
        assert_eq!(resolver.search_paths.len(), initial_count + 1);
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut resolver = ModuleResolver::new();

        // Create circular dependency: A -> B -> A
        resolver
            .dependencies
            .insert("A".to_string(), ["B".to_string()].into_iter().collect());
        resolver
            .dependencies
            .insert("B".to_string(), ["A".to_string()].into_iter().collect());

        assert!(resolver.check_circular_dependencies().is_err());
    }

    #[test]
    fn test_topological_sort() {
        let mut resolver = ModuleResolver::new();

        // Create dependency chain: A -> B -> C
        resolver.modules.insert(
            "A".to_string(),
            LoadedModule {
                name: "A".to_string(),
                file_path: PathBuf::from("A.cln"),
                hir: None,
                exports: HashMap::new(),
                dependencies: vec!["B".to_string()],
            },
        );

        resolver.modules.insert(
            "B".to_string(),
            LoadedModule {
                name: "B".to_string(),
                file_path: PathBuf::from("B.cln"),
                hir: None,
                exports: HashMap::new(),
                dependencies: vec!["C".to_string()],
            },
        );

        resolver.modules.insert(
            "C".to_string(),
            LoadedModule {
                name: "C".to_string(),
                file_path: PathBuf::from("C.cln"),
                hir: None,
                exports: HashMap::new(),
                dependencies: vec![],
            },
        );

        resolver
            .dependencies
            .insert("A".to_string(), ["B".to_string()].into_iter().collect());
        resolver
            .dependencies
            .insert("B".to_string(), ["C".to_string()].into_iter().collect());

        let result = resolver.topological_sort().unwrap();

        // C should come before B, and B should come before A
        let c_pos = result.iter().position(|x| x == "C").unwrap();
        let b_pos = result.iter().position(|x| x == "B").unwrap();
        let a_pos = result.iter().position(|x| x == "A").unwrap();

        assert!(c_pos < b_pos, "C should come before B");
        assert!(b_pos < a_pos, "B should come before A");
    }
}
