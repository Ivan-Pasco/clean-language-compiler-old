//! Compilation Unit - Represents an entire Clean Language program
//!
//! A CompilationUnit encapsulates all the modules that make up a Clean program,
//! from the entry point file to all transitively imported modules.

use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::hir::HirProgram;
use crate::resolver::{GlobalSymbolTable, ResolvedHirProgram, SymbolId};
use std::collections::HashMap;
use std::path::PathBuf;

/// Unique identifier for a module within a compilation unit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CompilationModuleId(pub usize);

/// Represents a single module (source file) in the compilation unit
#[derive(Debug, Clone)]
pub struct CompiledModule {
    /// Unique identifier for this module
    pub id: CompilationModuleId,

    /// Module name (derived from file path or explicit module declaration)
    pub name: String,

    /// Absolute path to the source file
    pub file_path: PathBuf,

    /// Source code content
    pub source: String,

    /// HIR representation (populated after Stage 3)
    pub hir: Option<HirProgram>,

    /// List of module names this module imports
    pub imports: Vec<String>,

    /// Exported symbols from this module (populated during resolution)
    pub exports: HashMap<String, SymbolId>,

    /// Whether this is the entry module
    pub is_entry: bool,

    /// Source location for error reporting
    pub location: SourceLocation,
}

impl CompiledModule {
    /// Create a new module from a file path and source code
    pub fn new(
        id: CompilationModuleId,
        name: String,
        file_path: PathBuf,
        source: String,
        is_entry: bool,
    ) -> Self {
        Self {
            id,
            name,
            file_path: file_path.clone(),
            source,
            hir: None,
            imports: Vec::new(),
            exports: HashMap::new(),
            is_entry,
            location: SourceLocation {
                file: file_path.to_string_lossy().to_string(),
                line: 1,
                column: 1,
                byte_start: None,
                byte_end: None,
            },
        }
    }

    /// Update the module with parsed imports
    pub fn set_imports(&mut self, imports: Vec<String>) {
        self.imports = imports;
    }

    /// Update the module with its HIR representation
    pub fn set_hir(&mut self, hir: HirProgram) {
        self.hir = Some(hir);
    }
}

/// Compilation Unit - represents the complete program being compiled
///
/// This is the top-level data structure for multi-file compilation.
/// It contains all modules, their dependency relationships, and
/// the unified symbol table after resolution.
#[derive(Debug)]
pub struct CompilationUnit {
    /// All modules in the compilation unit, indexed by their ID
    pub modules: HashMap<CompilationModuleId, CompiledModule>,

    /// Module name to ID mapping for quick lookup
    pub module_by_name: HashMap<String, CompilationModuleId>,

    /// The entry module ID (the main file being compiled)
    pub entry_module: CompilationModuleId,

    /// Modules in topological order (dependencies before dependents)
    pub compilation_order: Vec<CompilationModuleId>,

    /// Global symbol table (populated after resolution across all modules)
    pub global_symbols: Option<GlobalSymbolTable>,

    /// Resolved HIR for all modules (populated after Stage 4)
    pub resolved_programs: HashMap<CompilationModuleId, ResolvedHirProgram>,

    /// Counter for generating unique module IDs
    next_module_id: usize,
}

impl CompilationUnit {
    /// Create a new compilation unit with an entry module
    pub fn new(entry_name: String, entry_path: PathBuf, entry_source: String) -> Self {
        let entry_id = CompilationModuleId(0);
        let entry_module =
            CompiledModule::new(entry_id, entry_name.clone(), entry_path, entry_source, true);

        let mut modules = HashMap::new();
        modules.insert(entry_id, entry_module);

        let mut module_by_name = HashMap::new();
        module_by_name.insert(entry_name, entry_id);

        Self {
            modules,
            module_by_name,
            entry_module: entry_id,
            compilation_order: vec![entry_id], // Will be recalculated after graph building
            global_symbols: None,
            resolved_programs: HashMap::new(),
            next_module_id: 1,
        }
    }

    /// Add a new module to the compilation unit
    pub fn add_module(
        &mut self,
        name: String,
        file_path: PathBuf,
        source: String,
    ) -> CompilationModuleId {
        // Check if module already exists
        if let Some(existing_id) = self.module_by_name.get(&name) {
            return *existing_id;
        }

        let id = CompilationModuleId(self.next_module_id);
        self.next_module_id += 1;

        let module = CompiledModule::new(id, name.clone(), file_path, source, false);
        self.modules.insert(id, module);
        self.module_by_name.insert(name, id);

        id
    }

    /// Get a module by its ID
    pub fn get_module(&self, id: CompilationModuleId) -> Option<&CompiledModule> {
        self.modules.get(&id)
    }

    /// Get a mutable module by its ID
    pub fn get_module_mut(&mut self, id: CompilationModuleId) -> Option<&mut CompiledModule> {
        self.modules.get_mut(&id)
    }

    /// Get a module by its name
    pub fn get_module_by_name(&self, name: &str) -> Option<&CompiledModule> {
        self.module_by_name
            .get(name)
            .and_then(|id| self.modules.get(id))
    }

    /// Get the entry module
    pub fn get_entry_module(&self) -> &CompiledModule {
        self.modules
            .get(&self.entry_module)
            .expect("Entry module must exist")
    }

    /// Check if a module exists by name
    pub fn has_module(&self, name: &str) -> bool {
        self.module_by_name.contains_key(name)
    }

    /// Get the number of modules in the compilation unit
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Set the compilation order after topological sort
    pub fn set_compilation_order(&mut self, order: Vec<CompilationModuleId>) {
        self.compilation_order = order;
    }

    /// Get all module names
    pub fn module_names(&self) -> Vec<&String> {
        self.module_by_name.keys().collect()
    }

    /// Validate that all modules have been parsed to HIR
    pub fn all_modules_have_hir(&self) -> bool {
        self.modules.values().all(|m| m.hir.is_some())
    }

    /// Collect all HIR programs in compilation order
    pub fn collect_hir_programs(
        &self,
    ) -> Result<Vec<(&CompiledModule, &HirProgram)>, CompilerError> {
        let mut result = Vec::new();

        for id in &self.compilation_order {
            let module = self.modules.get(id).ok_or_else(|| {
                CompilerError::codegen_error(
                    format!("Module with ID {:?} not found in compilation unit", id),
                    None,
                    None,
                )
            })?;

            let hir = module.hir.as_ref().ok_or_else(|| {
                CompilerError::codegen_error(
                    format!("Module '{}' has no HIR - was it parsed?", module.name),
                    None,
                    Some(module.location.clone()),
                )
            })?;

            result.push((module, hir));
        }

        Ok(result)
    }
}

impl Default for CompilationUnit {
    fn default() -> Self {
        Self {
            modules: HashMap::new(),
            module_by_name: HashMap::new(),
            entry_module: CompilationModuleId(0),
            compilation_order: Vec::new(),
            global_symbols: None,
            resolved_programs: HashMap::new(),
            next_module_id: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_compilation_unit_creation() {
        let unit = CompilationUnit::new(
            "main".to_string(),
            PathBuf::from("main.cln"),
            "start:\n\tprint(42)".to_string(),
        );

        assert_eq!(unit.module_count(), 1);
        assert!(unit.has_module("main"));
        assert!(unit.get_entry_module().is_entry);
    }

    #[test]
    fn test_add_module() {
        let mut unit = CompilationUnit::new(
            "main".to_string(),
            PathBuf::from("main.cln"),
            "import: utils\nstart:\n\tprint(42)".to_string(),
        );

        let utils_id = unit.add_module(
            "utils".to_string(),
            PathBuf::from("utils.cln"),
            "functions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b".to_string(),
        );

        assert_eq!(unit.module_count(), 2);
        assert!(unit.has_module("utils"));

        let utils = unit.get_module(utils_id).unwrap();
        assert!(!utils.is_entry);
        assert_eq!(utils.name, "utils");
    }

    #[test]
    fn test_duplicate_module_returns_existing_id() {
        let mut unit = CompilationUnit::new(
            "main".to_string(),
            PathBuf::from("main.cln"),
            "start:\n\tprint(\"test\")".to_string(),
        );

        let id1 = unit.add_module(
            "utils".to_string(),
            PathBuf::from("utils.cln"),
            "// utils".to_string(),
        );

        let id2 = unit.add_module(
            "utils".to_string(),
            PathBuf::from("utils.cln"),
            "// utils".to_string(),
        );

        assert_eq!(id1, id2);
        assert_eq!(unit.module_count(), 2);
    }
}
