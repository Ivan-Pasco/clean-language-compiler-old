use crate::ast::{Class, Function, Program, Type, Visibility};
use crate::error::CompilerError;
use crate::parser;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Module resolver handles loading and linking of external modules
pub struct ModuleResolver {
    /// Cache of loaded modules to avoid duplicate loading
    module_cache: HashMap<String, Module>,
    /// Search paths for finding modules
    module_paths: Vec<PathBuf>,
    /// Current module being processed (for relative imports)
    current_module: Option<String>,
}

/// Represents a loaded module with its exported symbols
#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub file_path: PathBuf,
    pub program: Program,
    pub exports: ModuleExports,
}

/// Exported symbols from a module
#[derive(Debug, Clone)]
pub struct ModuleExports {
    pub functions: HashMap<String, Function>,
    pub classes: HashMap<String, Class>,
    pub types: HashMap<String, Type>,
}

/// Import resolution result
#[derive(Debug, Clone)]
pub struct ImportResolution {
    pub resolved_imports: HashMap<String, Module>,
    pub symbol_map: HashMap<String, String>, // alias -> actual_name
    pub single_symbols: HashMap<String, (String, String)>, // symbol_name -> (module_name, actual_symbol)
}

/// Import type for different import patterns
#[derive(Debug, Clone)]
pub enum ImportType {
    WholeModule,       // import: Math
    ModuleAlias,       // import: Utils as U
    SingleSymbol,      // import: Math.sqrt
    SingleSymbolAlias, // import: Json.decode as jd
}

impl ModuleResolver {
    /// Create a new module resolver
    pub fn new() -> Self {
        Self {
            module_cache: HashMap::new(),
            module_paths: vec![
                PathBuf::from("./"),         // Current directory
                PathBuf::from("./modules/"), // Local modules directory
                PathBuf::from("./lib/"),     // Library directory
                PathBuf::from("./stdlib/"),  // Standard library
            ],
            current_module: None,
        }
    }

    /// Add a search path for modules
    pub fn add_module_path<P: AsRef<Path>>(&mut self, path: P) {
        self.module_paths.push(path.as_ref().to_path_buf());
    }

    /// Set the current module context
    pub fn set_current_module(&mut self, module_name: String) {
        self.current_module = Some(module_name);
    }

    /// Resolve all imports for a program
    pub fn resolve_imports(
        &mut self,
        program: &Program,
    ) -> Result<ImportResolution, CompilerError> {
        let mut resolved_imports = HashMap::new();
        let mut symbol_map = HashMap::new();
        let mut single_symbols = HashMap::new();

        for import_item in &program.imports {
            let import_type = self.classify_import(&import_item.name);

            match import_type {
                ImportType::WholeModule => {
                    // import: Math
                    let module = self.load_module(&import_item.name)?;
                    let import_name = import_item.alias.as_ref().unwrap_or(&import_item.name);
                    resolved_imports.insert(import_name.clone(), module.clone());

                    if let Some(alias) = &import_item.alias {
                        symbol_map.insert(alias.clone(), import_item.name.clone());
                    }
                }
                ImportType::ModuleAlias => {
                    // import: Utils as U
                    let module = self.load_module(&import_item.name)?;
                    if let Some(alias) = &import_item.alias {
                        resolved_imports.insert(alias.clone(), module.clone());
                        symbol_map.insert(alias.clone(), import_item.name.clone());
                    }
                }
                ImportType::SingleSymbol => {
                    // import: Math.sqrt
                    let (module_name, symbol_name) = self.parse_single_symbol(&import_item.name)?;
                    let module = self.load_module(&module_name)?;

                    // Verify the symbol exists in the module
                    if !module.exports.has_function(&symbol_name)
                        && !module.exports.has_class(&symbol_name)
                    {
                        return Err(CompilerError::symbol_error(
                            format!("Symbol '{symbol_name}' not found in module '{module_name}'"),
                            &symbol_name,
                            Some(&module_name),
                        ));
                    }

                    let import_name = import_item.alias.as_ref().unwrap_or(&symbol_name);
                    single_symbols.insert(import_name.clone(), (module_name.clone(), symbol_name));

                    // Also store the module for reference
                    if !resolved_imports.contains_key(&module_name) {
                        resolved_imports.insert(module_name, module);
                    }
                }
                ImportType::SingleSymbolAlias => {
                    // import: Json.decode as jd
                    let (module_name, symbol_name) = self.parse_single_symbol(&import_item.name)?;
                    let module = self.load_module(&module_name)?;

                    // Verify the symbol exists in the module
                    if !module.exports.has_function(&symbol_name)
                        && !module.exports.has_class(&symbol_name)
                    {
                        return Err(CompilerError::symbol_error(
                            format!("Symbol '{symbol_name}' not found in module '{module_name}'"),
                            &symbol_name,
                            Some(&module_name),
                        ));
                    }

                    if let Some(alias) = &import_item.alias {
                        single_symbols
                            .insert(alias.clone(), (module_name.clone(), symbol_name.clone()));
                        symbol_map.insert(alias.clone(), format!("{module_name}.{symbol_name}"));
                    }

                    // Also store the module for reference
                    if !resolved_imports.contains_key(&module_name) {
                        resolved_imports.insert(module_name, module);
                    }
                }
            }
        }

        Ok(ImportResolution {
            resolved_imports,
            symbol_map,
            single_symbols,
        })
    }

    /// Classify the type of import based on the import string
    fn classify_import(&self, import_name: &str) -> ImportType {
        if import_name.contains('.') {
            ImportType::SingleSymbol
        } else {
            ImportType::WholeModule
        }
    }

    /// Parse a single symbol import like "Math.sqrt" into ("Math", "sqrt")
    fn parse_single_symbol(&self, import_name: &str) -> Result<(String, String), CompilerError> {
        let parts: Vec<&str> = import_name.split('.').collect();
        if parts.len() != 2 {
            return Err(CompilerError::import_error(
                format!("Invalid single symbol import format: '{import_name}'"),
                import_name,
                None,
            ));
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }

    /// Load a module by name
    fn load_module(&mut self, module_name: &str) -> Result<Module, CompilerError> {
        // Check cache first
        if let Some(cached_module) = self.module_cache.get(module_name) {
            return Ok(cached_module.clone());
        }

        // Find the module file
        let module_path = self.find_module_file(module_name)?;

        // Read and parse the module
        let source = fs::read_to_string(&module_path).map_err(|e| {
            CompilerError::module_error(
                format!("Failed to read module '{module_name}': {e}"),
                Some(format!(
                    "Check if the file exists and is readable: {}",
                    module_path.display()
                )),
                None,
            )
        })?;

        // Parse the module
        let program =
            parser::parse_with_file(&source, &module_path.to_string_lossy()).map_err(|e| {
                CompilerError::module_error(
                    format!("Failed to parse module '{module_name}': {e}"),
                    Some("Check the syntax of the module file".to_string()),
                    None,
                )
            })?;

        // Extract exports
        let exports = self.extract_exports(&program);

        // Create module
        let module = Module {
            name: module_name.to_string(),
            file_path: module_path,
            program,
            exports,
        };

        // Cache the module
        self.module_cache
            .insert(module_name.to_string(), module.clone());

        Ok(module)
    }

    /// Find a module file in the search paths
    fn find_module_file(&self, module_name: &str) -> Result<PathBuf, CompilerError> {
        let possible_extensions = ["clean", "cln"];

        for search_path in &self.module_paths {
            for extension in &possible_extensions {
                let file_path = search_path.join(format!("{module_name}.{extension}"));
                if file_path.exists() {
                    return Ok(file_path);
                }
            }
        }

        let search_paths_str = self
            .module_paths
            .iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ");

        Err(CompilerError::module_error(
            format!("Module '{module_name}' not found in search paths"),
            Some(format!("Search paths: {search_paths_str}")),
            None,
        ))
    }

    /// Extract exported symbols from a program
    /// According to specification: public by default, private when marked
    fn extract_exports(&self, program: &Program) -> ModuleExports {
        let mut exports = ModuleExports {
            functions: HashMap::new(),
            classes: HashMap::new(),
            types: HashMap::new(),
        };

        // Export functions (public by default, unless marked private)
        for function in &program.functions {
            // According to specification: functions are public by default
            if function.visibility != Visibility::Private {
                exports
                    .functions
                    .insert(function.name.clone(), function.clone());
            }
        }

        // Export classes (public by default)
        for class in &program.classes {
            // Classes are public by default in Clean Language
            exports.classes.insert(class.name.clone(), class.clone());

            // Also export the class as a type
            exports
                .types
                .insert(class.name.clone(), Type::Object(class.name.clone()));
        }

        exports
    }

    /// Get all loaded modules
    pub fn get_loaded_modules(&self) -> &HashMap<String, Module> {
        &self.module_cache
    }

    /// Clear the module cache
    pub fn clear_cache(&mut self) {
        self.module_cache.clear();
    }

    /// Resolve a symbol reference in the context of imports
    pub fn resolve_symbol(
        &self,
        symbol_name: &str,
        imports: &ImportResolution,
    ) -> Option<(String, String)> {
        // Check if it's a direct single symbol import
        if let Some((module_name, actual_symbol)) = imports.single_symbols.get(symbol_name) {
            return Some((module_name.clone(), actual_symbol.clone()));
        }

        // Check if it's a module.symbol reference
        if symbol_name.contains('.') {
            let parts: Vec<&str> = symbol_name.split('.').collect();
            if parts.len() == 2 {
                let module_name = parts[0];
                let symbol = parts[1];

                // Check if the module is imported
                if imports.resolved_imports.contains_key(module_name) {
                    return Some((module_name.to_string(), symbol.to_string()));
                }

                // Check if it's an aliased module
                if let Some(actual_module) = imports.symbol_map.get(module_name) {
                    return Some((actual_module.clone(), symbol.to_string()));
                }
            }
        }

        None
    }
}

impl ModuleExports {
    /// Check if a function is exported
    pub fn has_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    /// Check if a class is exported
    pub fn has_class(&self, name: &str) -> bool {
        self.classes.contains_key(name)
    }

    /// Get an exported function
    pub fn get_function(&self, name: &str) -> Option<&Function> {
        self.functions.get(name)
    }

    /// Get an exported class
    pub fn get_class(&self, name: &str) -> Option<&Class> {
        self.classes.get(name)
    }

    /// Get all exported function names
    pub fn function_names(&self) -> Vec<&String> {
        self.functions.keys().collect()
    }

    /// Get all exported class names
    pub fn class_names(&self) -> Vec<&String> {
        self.classes.keys().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_module_resolver_creation() {
        let resolver = ModuleResolver::new();
        assert_eq!(resolver.module_cache.len(), 0);
        assert!(!resolver.module_paths.is_empty());
    }

    #[test]
    fn test_add_module_path() {
        let mut resolver = ModuleResolver::new();
        let initial_paths = resolver.module_paths.len();

        resolver.add_module_path("/custom/path");
        assert_eq!(resolver.module_paths.len(), initial_paths + 1);
    }

    #[test]
    fn test_module_file_search() {
        let dir = tempdir().unwrap();
        let module_path = dir.path().join("TestModule.clean");
        fs::write(&module_path, "function test() -> void\n\tprint(\"test\")").unwrap();

        let mut resolver = ModuleResolver::new();
        resolver.add_module_path(dir.path());

        let found_path = resolver.find_module_file("TestModule").unwrap();
        // Compare just the file name and extension, not the full path
        assert_eq!(found_path.file_name(), module_path.file_name());
        assert!(found_path.exists());
    }

    #[test]
    fn test_module_not_found() {
        let resolver = ModuleResolver::new();
        let result = resolver.find_module_file("NonExistentModule");
        assert!(result.is_err());
    }
}
