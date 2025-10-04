//! Symbol Table Implementation for Name Resolution
//!
//! This module provides symbol table functionality for tracking and resolving
//! symbols (variables, functions, classes, methods, fields) across different scopes.

use crate::ast::SourceLocation;
use crate::hir::*;
use std::collections::{HashMap, HashSet};

/// Unique identifier for symbols
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub usize);

/// Unique identifier for modules
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(pub usize);

/// Unique identifier for scopes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub usize);

/// Symbol kinds for type checking and usage validation
#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Variable {
        var_type: HirType,
    },
    Parameter {
        param_type: HirType,
    },
    Function {
        parameters: Vec<HirType>,
        return_type: Option<HirType>,
    },
    Class {
        fields: Vec<SymbolId>,
        methods: Vec<SymbolId>,
        parent: Option<SymbolId>,
    },
    Method {
        class_id: SymbolId,
        parameters: Vec<HirType>,
        return_type: HirType,
    },
    Field {
        class_id: SymbolId,
        field_type: HirType,
    },
    Constructor {
        class_id: SymbolId,
        parameters: Vec<HirType>,
    },
    Module {
        exported_symbols: Vec<SymbolId>,
    },
    Namespace {
        functions: Vec<SymbolId>,
    },
}

/// Symbol information stored in the symbol table
#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub scope_id: ScopeId,
    pub location: SourceLocation,
    pub is_exported: bool,
    pub is_imported: bool,
    pub module_id: Option<ModuleId>,
}

/// Scope information for nested scoping
#[derive(Debug, Clone)]
pub struct Scope {
    pub id: ScopeId,
    pub parent: Option<ScopeId>,
    pub symbols: HashMap<String, SymbolId>,
    pub scope_type: ScopeType,
}

/// Types of scopes in Clean Language
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeType {
    Global,
    Function {
        function_id: SymbolId,
    },
    Class {
        class_id: SymbolId,
    },
    Method {
        method_id: SymbolId,
        class_id: SymbolId,
    },
    Constructor {
        class_id: SymbolId,
    },
    Block,
    Test,
}

/// Module information for import resolution
#[derive(Debug, Clone)]
pub struct Module {
    pub id: ModuleId,
    pub name: String,
    pub file_path: String,
    pub exported_symbols: HashMap<String, SymbolId>,
    pub imported_modules: Vec<ModuleId>,
}

/// Global symbol table managing all symbols and scopes
#[derive(Debug, Clone)]
pub struct GlobalSymbolTable {
    symbols: HashMap<SymbolId, Symbol>,
    scopes: HashMap<ScopeId, Scope>,
    modules: HashMap<ModuleId, Module>,

    // Generators for unique IDs
    next_symbol_id: usize,
    next_scope_id: usize,
    next_module_id: usize,

    // Current context
    current_scope: ScopeId,
    current_module: Option<ModuleId>,

    // Built-in symbols (populated during initialization)
    builtins: HashSet<SymbolId>,
}

impl GlobalSymbolTable {
    /// Create a new global symbol table with built-in symbols
    pub fn new() -> Self {
        let mut table = Self {
            symbols: HashMap::new(),
            scopes: HashMap::new(),
            modules: HashMap::new(),
            next_symbol_id: 0,
            next_scope_id: 0,
            next_module_id: 0,
            current_scope: ScopeId(0),
            current_module: None,
            builtins: HashSet::new(),
        };

        // Create global scope
        table.create_scope(None, ScopeType::Global);

        // Add built-in symbols
        table.add_builtins();

        table
    }

    /// Add built-in symbols to the global scope
    fn add_builtins(&mut self) {
        let global_scope = self.current_scope;

        // Built-in functions
        let builtin_functions = vec![
            ("print", vec![HirType::String], Some(HirType::Void)),
            ("println", vec![HirType::String], Some(HirType::Void)),
            ("printl", vec![HirType::String], Some(HirType::Void)),
            ("abs", vec![HirType::Number], Some(HirType::Number)),
            (
                "max",
                vec![HirType::Number, HirType::Number],
                Some(HirType::Number),
            ),
            (
                "min",
                vec![HirType::Number, HirType::Number],
                Some(HirType::Number),
            ),
            ("sqrt", vec![HirType::Number], Some(HirType::Number)),
            (
                "pow",
                vec![HirType::Number, HirType::Number],
                Some(HirType::Number),
            ),
            ("toString", vec![HirType::Integer], Some(HirType::String)),
            ("toInteger", vec![HirType::String], Some(HirType::Integer)),
        ];

        for (name, params, return_type) in builtin_functions {
            let symbol_id = self.create_symbol(
                name.to_string(),
                SymbolKind::Function {
                    parameters: params,
                    return_type,
                },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                },
            );

            self.builtins.insert(symbol_id);
        }

        // Built-in static classes with methods
        self.add_builtin_static_classes(global_scope);

        // Built-in namespace functions (e.g., math_sin for math.sin())
        self.add_builtin_namespace_functions(global_scope);

        // Built-in namespace identifiers (so math.max() syntax works)
        self.add_builtin_namespace_identifiers(global_scope);

        // Built-in types are handled differently - they're part of HirType enum
        // Built-in methods are resolved dynamically based on receiver type
    }

    /// Add built-in static classes (Math, StringUtils, etc.)
    fn add_builtin_static_classes(&mut self, global_scope: ScopeId) {
        // Math class with static methods
        let math_class_id = self.create_symbol(
            "Math".to_string(),
            SymbolKind::Class {
                fields: Vec::new(),
                methods: Vec::new(),
                parent: None,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );

        let math_methods = vec![
            ("abs", vec![HirType::Number], HirType::Number),
            ("floor", vec![HirType::Number], HirType::Integer),
            ("ceil", vec![HirType::Number], HirType::Integer),
            ("round", vec![HirType::Number], HirType::Integer),
            ("sqrt", vec![HirType::Number], HirType::Number),
            (
                "pow",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            ("sin", vec![HirType::Number], HirType::Number),
            ("cos", vec![HirType::Number], HirType::Number),
            ("tan", vec![HirType::Number], HirType::Number),
            (
                "max",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "min",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
        ];

        let mut math_method_ids = Vec::new();
        for (method_name, params, return_type) in math_methods {
            let method_id = self.create_symbol(
                method_name.to_string(),
                SymbolKind::Method {
                    class_id: math_class_id,
                    parameters: params,
                    return_type,
                },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                },
            );
            math_method_ids.push(method_id);
            self.builtins.insert(method_id);
        }

        // Update Math class with methods
        if let Some(math_symbol) = self.symbols.get_mut(&math_class_id) {
            if let SymbolKind::Class { methods, .. } = &mut math_symbol.kind {
                *methods = math_method_ids;
            }
        }
        self.builtins.insert(math_class_id);

        // StringUtils class with static methods
        let stringutils_class_id = self.create_symbol(
            "StringUtils".to_string(),
            SymbolKind::Class {
                fields: Vec::new(),
                methods: Vec::new(),
                parent: None,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );

        let stringutils_methods = vec![
            ("length", vec![HirType::String], HirType::Integer),
            (
                "concat",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "substring",
                vec![HirType::String, HirType::Integer, HirType::Integer],
                HirType::String,
            ),
            (
                "indexOf",
                vec![HirType::String, HirType::String],
                HirType::Integer,
            ),
            (
                "replace",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            ("toUpperCase", vec![HirType::String], HirType::String),
            ("toLowerCase", vec![HirType::String], HirType::String),
        ];

        let mut stringutils_method_ids = Vec::new();
        for (method_name, params, return_type) in stringutils_methods {
            let method_id = self.create_symbol(
                method_name.to_string(),
                SymbolKind::Method {
                    class_id: stringutils_class_id,
                    parameters: params,
                    return_type,
                },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                },
            );
            stringutils_method_ids.push(method_id);
            self.builtins.insert(method_id);
        }

        // Update StringUtils class with methods
        if let Some(stringutils_symbol) = self.symbols.get_mut(&stringutils_class_id) {
            if let SymbolKind::Class { methods, .. } = &mut stringutils_symbol.kind {
                *methods = stringutils_method_ids;
            }
        }
        self.builtins.insert(stringutils_class_id);

        // String class with static methods
        let string_class_id = self.create_symbol(
            "String".to_string(),
            SymbolKind::Class {
                fields: Vec::new(),
                methods: Vec::new(),
                parent: None,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );

        let string_methods = vec![
            ("fromCharCode", vec![HirType::Integer], HirType::String),
            ("isEmpty", vec![HirType::String], HirType::Boolean),
        ];

        let mut string_method_ids = Vec::new();
        for (method_name, params, return_type) in string_methods {
            let method_id = self.create_symbol(
                method_name.to_string(),
                SymbolKind::Method {
                    class_id: string_class_id,
                    parameters: params,
                    return_type,
                },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                },
            );
            string_method_ids.push(method_id);
            self.builtins.insert(method_id);
        }

        // Update String class with methods
        if let Some(string_symbol) = self.symbols.get_mut(&string_class_id) {
            if let SymbolKind::Class { methods, .. } = &mut string_symbol.kind {
                *methods = string_method_ids;
            }
        }
        self.builtins.insert(string_class_id);

        // Integer class with static methods
        let integer_class_id = self.create_symbol(
            "Integer".to_string(),
            SymbolKind::Class {
                fields: Vec::new(),
                methods: Vec::new(),
                parent: None,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );

        let integer_methods = vec![("parse", vec![HirType::String], HirType::Integer)];

        let mut integer_method_ids = Vec::new();
        for (method_name, params, return_type) in integer_methods {
            let method_id = self.create_symbol(
                method_name.to_string(),
                SymbolKind::Method {
                    class_id: integer_class_id,
                    parameters: params,
                    return_type,
                },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                },
            );
            integer_method_ids.push(method_id);
            self.builtins.insert(method_id);
        }

        // Update Integer class with methods
        if let Some(integer_symbol) = self.symbols.get_mut(&integer_class_id) {
            if let SymbolKind::Class { methods, .. } = &mut integer_symbol.kind {
                *methods = integer_method_ids;
            }
        }
        self.builtins.insert(integer_class_id);
    }

    /// Generate a new unique symbol ID
    fn next_symbol_id(&mut self) -> SymbolId {
        let id = SymbolId(self.next_symbol_id);
        self.next_symbol_id += 1;
        id
    }

    /// Generate a new unique scope ID
    fn next_scope_id(&mut self) -> ScopeId {
        let id = ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        id
    }

    /// Generate a new unique module ID
    fn next_module_id(&mut self) -> ModuleId {
        let id = ModuleId(self.next_module_id);
        self.next_module_id += 1;
        id
    }

    /// Create a new scope
    pub fn create_scope(&mut self, parent: Option<ScopeId>, scope_type: ScopeType) -> ScopeId {
        let scope_id = self.next_scope_id();

        // Prevent circular reference: scope should never be its own parent
        let parent_scope = match parent {
            Some(p) => {
                // Ensure parent is not the same as the new scope (should be impossible but safety check)
                if p != scope_id {
                    Some(p)
                } else {
                    None
                }
            }
            None => {
                // Use current scope as parent unless it would create a circular reference
                if self.current_scope != scope_id {
                    Some(self.current_scope)
                } else {
                    None
                }
            }
        };

        let scope = Scope {
            id: scope_id,
            parent: parent_scope,
            symbols: HashMap::new(),
            scope_type,
        };

        self.scopes.insert(scope_id, scope);
        scope_id
    }

    /// Enter a scope (set as current)
    pub fn enter_scope(&mut self, scope_id: ScopeId) {
        self.current_scope = scope_id;
    }

    /// Exit current scope (return to parent)
    pub fn exit_scope(&mut self) {
        if let Some(scope) = self.scopes.get(&self.current_scope) {
            if let Some(parent_id) = scope.parent {
                self.current_scope = parent_id;
            }
        }
    }

    /// Create a new symbol and add it to the current scope
    pub fn create_symbol(
        &mut self,
        name: String,
        kind: SymbolKind,
        scope_id: ScopeId,
        location: SourceLocation,
    ) -> SymbolId {
        let symbol_id = self.next_symbol_id();
        let symbol = Symbol {
            id: symbol_id,
            name: name.clone(),
            kind,
            scope_id,
            location,
            is_exported: false,
            is_imported: false,
            module_id: self.current_module,
        };

        self.symbols.insert(symbol_id, symbol);

        // Add to scope
        if let Some(scope) = self.scopes.get_mut(&scope_id) {
            scope.symbols.insert(name, symbol_id);
        }

        symbol_id
    }

    /// Look up a symbol by name in the current scope and parent scopes
    pub fn lookup_symbol(&self, name: &str) -> Option<SymbolId> {
        self.lookup_symbol_in_scope(name, self.current_scope)
    }

    /// Look up a symbol by name starting from a specific scope
    pub fn lookup_symbol_in_scope(&self, name: &str, scope_id: ScopeId) -> Option<SymbolId> {
        self.lookup_symbol_in_scope_with_depth(name, scope_id, 0)
    }

    fn lookup_symbol_in_scope_with_depth(
        &self,
        name: &str,
        scope_id: ScopeId,
        depth: usize,
    ) -> Option<SymbolId> {
        const MAX_SCOPE_DEPTH: usize = 50; // Prevent infinite recursion in scope chains

        if depth > MAX_SCOPE_DEPTH {
            eprintln!(
                "WARNING: Maximum scope depth exceeded looking up symbol '{}'",
                name
            );
            return None;
        }

        if let Some(scope) = self.scopes.get(&scope_id) {
            // Check current scope
            if let Some(&symbol_id) = scope.symbols.get(name) {
                return Some(symbol_id);
            }

            // Check parent scope
            if let Some(parent_id) = scope.parent {
                return self.lookup_symbol_in_scope_with_depth(name, parent_id, depth + 1);
            }
        }
        None
    }

    /// Look up a symbol in a specific class scope (for method/field access)
    pub fn lookup_class_member(&self, class_id: SymbolId, member_name: &str) -> Option<SymbolId> {
        if let Some(symbol) = self.symbols.get(&class_id) {
            if let SymbolKind::Class {
                fields, methods, ..
            } = &symbol.kind
            {
                // Check fields
                for &field_id in fields {
                    if let Some(field_symbol) = self.symbols.get(&field_id) {
                        if field_symbol.name == member_name {
                            return Some(field_id);
                        }
                    }
                }

                // Check methods
                for &method_id in methods {
                    if let Some(method_symbol) = self.symbols.get(&method_id) {
                        if method_symbol.name == member_name {
                            return Some(method_id);
                        }
                    }
                }

                // Check parent class if exists
                if let SymbolKind::Class {
                    parent: Some(parent_id),
                    ..
                } = &symbol.kind
                {
                    return self.lookup_class_member(*parent_id, member_name);
                }
            }
        }
        None
    }

    /// Get symbol by ID
    pub fn get_symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(&id)
    }

    /// Get mutable symbol by ID
    pub fn get_symbol_mut(&mut self, id: SymbolId) -> Option<&mut Symbol> {
        self.symbols.get_mut(&id)
    }

    /// Get scope by ID
    pub fn get_scope(&self, id: ScopeId) -> Option<&Scope> {
        self.scopes.get(&id)
    }

    /// Check if a symbol is a built-in
    pub fn is_builtin(&self, symbol_id: SymbolId) -> bool {
        self.builtins.contains(&symbol_id)
    }

    /// Mark a symbol as builtin
    pub fn mark_as_builtin(&mut self, symbol_id: SymbolId) {
        self.builtins.insert(symbol_id);
    }

    /// Create a new module
    pub fn create_module(&mut self, name: String, file_path: String) -> ModuleId {
        let module_id = self.next_module_id();
        let module = Module {
            id: module_id,
            name: name.clone(),
            file_path,
            exported_symbols: HashMap::new(),
            imported_modules: Vec::new(),
        };

        self.modules.insert(module_id, module);
        module_id
    }

    /// Set current module
    pub fn set_current_module(&mut self, module_id: ModuleId) {
        self.current_module = Some(module_id);
    }

    /// Get module by ID
    pub fn get_module(&self, id: ModuleId) -> Option<&Module> {
        self.modules.get(&id)
    }

    /// Export a symbol from a module
    pub fn export_symbol(&mut self, module_id: ModuleId, name: String, symbol_id: SymbolId) {
        if let Some(module) = self.modules.get_mut(&module_id) {
            module.exported_symbols.insert(name, symbol_id);
        }

        if let Some(symbol) = self.symbols.get_mut(&symbol_id) {
            symbol.is_exported = true;
        }
    }

    /// Import a symbol from a module
    pub fn import_symbol(&mut self, from_module: ModuleId, symbol_name: &str) -> Option<SymbolId> {
        if let Some(module) = self.modules.get(&from_module) {
            if let Some(&symbol_id) = module.exported_symbols.get(symbol_name) {
                // Mark symbol as imported
                if let Some(symbol) = self.symbols.get_mut(&symbol_id) {
                    symbol.is_imported = true;
                }
                return Some(symbol_id);
            }
        }
        None
    }

    /// Get all symbols in current scope
    pub fn current_scope_symbols(&self) -> Vec<SymbolId> {
        if let Some(scope) = self.scopes.get(&self.current_scope) {
            scope.symbols.values().copied().collect()
        } else {
            Vec::new()
        }
    }

    /// Get current scope ID
    pub fn current_scope_id(&self) -> ScopeId {
        self.current_scope
    }

    /// Check for symbol conflicts in current scope
    pub fn has_symbol_in_current_scope(&self, name: &str) -> bool {
        if let Some(scope) = self.scopes.get(&self.current_scope) {
            scope.symbols.contains_key(name)
        } else {
            false
        }
    }

    /// Find all accessible symbols (for completion/suggestion purposes)
    pub fn accessible_symbols(&self) -> Vec<SymbolId> {
        let mut symbols = Vec::new();
        let mut current_scope_id = Some(self.current_scope);

        while let Some(scope_id) = current_scope_id {
            if let Some(scope) = self.scopes.get(&scope_id) {
                symbols.extend(scope.symbols.values());
                current_scope_id = scope.parent;
            } else {
                break;
            }
        }

        symbols
    }

    /// Add built-in namespace functions (e.g., math_sin for math.sin calls)
    fn add_builtin_namespace_functions(&mut self, global_scope: ScopeId) {
        let namespace_functions = vec![
            // Math namespace functions
            ("math_sin", vec![HirType::Number], HirType::Number),
            ("math_cos", vec![HirType::Number], HirType::Number),
            ("math_tan", vec![HirType::Number], HirType::Number),
            ("math_asin", vec![HirType::Number], HirType::Number),
            ("math_acos", vec![HirType::Number], HirType::Number),
            ("math_atan", vec![HirType::Number], HirType::Number),
            (
                "math_atan2",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            ("math_sinh", vec![HirType::Number], HirType::Number),
            ("math_cosh", vec![HirType::Number], HirType::Number),
            ("math_tanh", vec![HirType::Number], HirType::Number),
            ("math_log", vec![HirType::Number], HirType::Number),
            ("math_ln", vec![HirType::Number], HirType::Number),
            ("math_log10", vec![HirType::Number], HirType::Number),
            ("math_log2", vec![HirType::Number], HirType::Number),
            ("math_exp", vec![HirType::Number], HirType::Number),
            ("math_exp2", vec![HirType::Number], HirType::Number),
            ("math_abs", vec![HirType::Number], HirType::Number),
            ("math_floor", vec![HirType::Number], HirType::Number),
            ("math_ceil", vec![HirType::Number], HirType::Number),
            ("math_round", vec![HirType::Number], HirType::Number),
            ("math_sqrt", vec![HirType::Number], HirType::Number),
            ("math_trunc", vec![HirType::Number], HirType::Number),
            ("math_pi", vec![], HirType::Number),
            ("math_e", vec![], HirType::Number),
            ("math_tau", vec![], HirType::Number),
            (
                "math_pow",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math_max",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math_min",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math_add",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math_subtract",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math_multiply",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math_divide",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            // String namespace functions
            // Canonical: string_size (not string_length - see Clean Language Specification)
            ("string_size", vec![HirType::String], HirType::Integer),
            (
                "string_concat",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            ("string_isEmpty", vec![HirType::String], HirType::Boolean),
            ("string_isBlank", vec![HirType::String], HirType::Boolean),
            (
                "string_indexOf",
                vec![HirType::String, HirType::String],
                HirType::Integer,
            ),
            (
                "string_lastIndexOf",
                vec![HirType::String, HirType::String],
                HirType::Integer,
            ),
            (
                "string_substring",
                vec![HirType::String, HirType::Integer, HirType::Integer],
                HirType::String,
            ),
            ("string_toUpperCase", vec![HirType::String], HirType::String),
            ("string_toLowerCase", vec![HirType::String], HirType::String),
            ("string_trim", vec![HirType::String], HirType::String),
            ("string_trimStart", vec![HirType::String], HirType::String),
            ("string_trimEnd", vec![HirType::String], HirType::String),
            (
                "string_split",
                vec![HirType::String, HirType::String],
                HirType::List(Box::new(HirType::String)),
            ),
            (
                "string_join",
                vec![HirType::List(Box::new(HirType::String)), HirType::String],
                HirType::String,
            ),
            (
                "string_padStart",
                vec![HirType::String, HirType::Integer, HirType::String],
                HirType::String,
            ),
            (
                "string_padEnd",
                vec![HirType::String, HirType::Integer, HirType::String],
                HirType::String,
            ),
            (
                "string_charAt",
                vec![HirType::String, HirType::Integer],
                HirType::String,
            ),
            (
                "string_charCodeAt",
                vec![HirType::String, HirType::Integer],
                HirType::Integer,
            ),
            (
                "string_contains",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "string_startsWith",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "string_endsWith",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "string_replace",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "string_replaceAll",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            // StringUtils namespace functions (for StringUtils.length() syntax)
            (
                "StringUtils_length",
                vec![HirType::String],
                HirType::Integer,
            ),
            (
                "StringUtils_concat",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "StringUtils_substring",
                vec![HirType::String, HirType::Integer, HirType::Integer],
                HirType::String,
            ),
            (
                "StringUtils_indexOf",
                vec![HirType::String, HirType::String],
                HirType::Integer,
            ),
            (
                "StringUtils_replace",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            // List namespace functions
            // Canonical: list_size (not list_length - see Clean Language Specification)
            (
                "list_size",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Integer,
            ),
            (
                "list_isEmpty",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Boolean,
            ),
            (
                "list_isNotEmpty",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Boolean,
            ),
            (
                "list_push",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Void,
            ),
            (
                "list_pop",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list_get",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Integer],
                HirType::Number,
            ),
            (
                "list_remove",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Integer],
                HirType::Number,
            ),
            (
                "list_peek",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list_add",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Void,
            ),
            (
                "list_contains",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Boolean,
            ),
            (
                "list_insert",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::Integer,
                    HirType::Number,
                ],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list_indexOf",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Integer,
            ),
            (
                "list_lastIndexOf",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Integer,
            ),
            (
                "list_first",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list_last",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list_reverse",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list_sort",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list_slice",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::Integer,
                    HirType::Integer,
                ],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list_concat",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::List(Box::new(HirType::Number)),
                ],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list_fill",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::Number,
                    HirType::Integer,
                ],
                HirType::Void,
            ),
            (
                "list_range",
                vec![HirType::Integer, HirType::Integer],
                HirType::List(Box::new(HirType::Integer)),
            ),
            (
                "list_join",
                vec![HirType::List(Box::new(HirType::Number)), HirType::String],
                HirType::String,
            ),
            // HTTP namespace functions
            ("http_get", vec![HirType::String], HirType::String),
            (
                "http_post",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http_put",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http_patch",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            ("http_delete", vec![HirType::String], HirType::String),
            // File namespace functions
            ("file_read", vec![HirType::String], HirType::String),
            (
                "file_write",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "file_append",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            ("file_exists", vec![HirType::String], HirType::Boolean),
            ("file_delete", vec![HirType::String], HirType::Boolean),
            // Comparison namespace functions (compare.integer.*, compare.number.*)
            (
                "compare.integer_equal",
                vec![HirType::Integer, HirType::Integer],
                HirType::Boolean,
            ),
            (
                "compare.integer_notEqual",
                vec![HirType::Integer, HirType::Integer],
                HirType::Boolean,
            ),
            (
                "compare.integer_lessThan",
                vec![HirType::Integer, HirType::Integer],
                HirType::Boolean,
            ),
            (
                "compare.integer_greaterThan",
                vec![HirType::Integer, HirType::Integer],
                HirType::Boolean,
            ),
            (
                "compare.integer_lessEqual",
                vec![HirType::Integer, HirType::Integer],
                HirType::Boolean,
            ),
            (
                "compare.integer_greaterEqual",
                vec![HirType::Integer, HirType::Integer],
                HirType::Boolean,
            ),
            (
                "compare.number_equal",
                vec![HirType::Number, HirType::Number],
                HirType::Boolean,
            ),
            (
                "compare.number_notEqual",
                vec![HirType::Number, HirType::Number],
                HirType::Boolean,
            ),
            (
                "compare.number_lessThan",
                vec![HirType::Number, HirType::Number],
                HirType::Boolean,
            ),
            (
                "compare.number_greaterThan",
                vec![HirType::Number, HirType::Number],
                HirType::Boolean,
            ),
            (
                "compare.number_lessEqual",
                vec![HirType::Number, HirType::Number],
                HirType::Boolean,
            ),
            (
                "compare.number_greaterEqual",
                vec![HirType::Number, HirType::Number],
                HirType::Boolean,
            ),
            // Conditional namespace functions (conditional.integer, conditional.number, etc.)
            (
                "conditional_integer",
                vec![HirType::Boolean, HirType::Integer, HirType::Integer],
                HirType::Integer,
            ),
            (
                "conditional_number",
                vec![HirType::Boolean, HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "conditional_string",
                vec![HirType::Boolean, HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "conditional_boolean",
                vec![HirType::Boolean, HirType::Boolean, HirType::Boolean],
                HirType::Boolean,
            ),
            // Logical namespace functions (logical.and, logical.or, logical.not)
            (
                "logical_and",
                vec![HirType::Boolean, HirType::Boolean],
                HirType::Boolean,
            ),
            (
                "logical_or",
                vec![HirType::Boolean, HirType::Boolean],
                HirType::Boolean,
            ),
            ("logical_not", vec![HirType::Boolean], HirType::Boolean),
            // Input namespace functions (input(), input.integer(), input.yesNo(), etc.)
            ("input", vec![HirType::String], HirType::String),
            ("input_integer", vec![HirType::String], HirType::Integer),
            ("input_number", vec![HirType::String], HirType::Number),
            ("input_yesNo", vec![HirType::String], HirType::Boolean),
        ];

        for (name, params, return_type) in namespace_functions {
            let symbol_id = self.create_symbol(
                name.to_string(),
                SymbolKind::Function {
                    parameters: params,
                    return_type: Some(return_type),
                },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                },
            );
            self.builtins.insert(symbol_id);
        }
    }

    /// Add built-in namespace identifiers (math, string, etc.)
    fn add_builtin_namespace_identifiers(&mut self, global_scope: ScopeId) {
        // Math namespace - so "math" is recognized as a valid identifier
        let math_functions = vec![
            self.lookup_symbol("math_sin").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_cos").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_tan").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_asin").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_acos").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_atan").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_atan2").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_sinh").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_cosh").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_tanh").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_log").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_ln").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_log10").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_log2").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_exp").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_exp2").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_abs").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_floor").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_ceil").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_round").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_sqrt").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_trunc").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_pi").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_e").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_tau").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_pow").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_max").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_min").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_add").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_subtract").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_multiply").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math_divide").unwrap_or(SymbolId(0)),
        ];

        let math_namespace_id = self.create_symbol(
            "math".to_string(),
            SymbolKind::Namespace {
                functions: math_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(math_namespace_id);

        // String namespace - so "string" is recognized as a valid identifier
        // Canonical: string_size (not string_length - see Clean Language Specification)
        let string_functions = vec![
            self.lookup_symbol("string_size").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_concat").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_isEmpty").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_isBlank").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_indexOf").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_lastIndexOf")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_substring")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_toUpperCase")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_toLowerCase")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_trim").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_trimStart")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_trimEnd").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_contains").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_startsWith")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_endsWith").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_replace").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_replaceAll")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_split").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_join").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_padStart").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_padEnd").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_charAt").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string_charCodeAt")
                .unwrap_or(SymbolId(0)),
        ];

        let string_namespace_id = self.create_symbol(
            "string".to_string(),
            SymbolKind::Namespace {
                functions: string_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(string_namespace_id);

        // Create list namespace
        // Canonical: list_size (not list_length - see Clean Language Specification)
        let list_functions = vec![
            self.lookup_symbol("list_size").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_isEmpty").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_isNotEmpty").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_push").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_pop").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_get").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_remove").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_peek").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_add").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_contains").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_insert").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_indexOf").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_lastIndexOf")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_first").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_last").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_reverse").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_sort").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_slice").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_concat").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_fill").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_range").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list_join").unwrap_or(SymbolId(0)),
        ];

        let list_namespace_id = self.create_symbol(
            "list".to_string(),
            SymbolKind::Namespace {
                functions: list_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(list_namespace_id);

        // Create http namespace
        // Create conditional namespace
        let conditional_functions = vec![
            self.lookup_symbol("conditional.integer")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("conditional.number")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("conditional.string")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("conditional.boolean")
                .unwrap_or(SymbolId(0)),
        ];

        let conditional_namespace_id = self.create_symbol(
            "conditional".to_string(),
            SymbolKind::Namespace {
                functions: conditional_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(conditional_namespace_id);

        let http_functions = vec![
            self.lookup_symbol("http_get").unwrap_or(SymbolId(0)),
            self.lookup_symbol("http_post").unwrap_or(SymbolId(0)),
            self.lookup_symbol("http_put").unwrap_or(SymbolId(0)),
            self.lookup_symbol("http_patch").unwrap_or(SymbolId(0)),
            self.lookup_symbol("http_delete").unwrap_or(SymbolId(0)),
        ];

        let http_namespace_id = self.create_symbol(
            "http".to_string(),
            SymbolKind::Namespace {
                functions: http_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(http_namespace_id);

        // Create file namespace
        let file_functions = vec![
            self.lookup_symbol("file_read").unwrap_or(SymbolId(0)),
            self.lookup_symbol("file_write").unwrap_or(SymbolId(0)),
            self.lookup_symbol("file_append").unwrap_or(SymbolId(0)),
            self.lookup_symbol("file_exists").unwrap_or(SymbolId(0)),
            self.lookup_symbol("file_delete").unwrap_or(SymbolId(0)),
        ];

        let file_namespace_id = self.create_symbol(
            "file".to_string(),
            SymbolKind::Namespace {
                functions: file_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(file_namespace_id);

        // Create compare namespace (compare.integer.*, compare.number.*)
        let compare_functions = vec![
            self.lookup_symbol("compare.integer_equal")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.integer_notEqual")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.integer_lessThan")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.integer_greaterThan")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.integer_lessEqual")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.integer_greaterEqual")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.number_equal")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.number_notEqual")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.number_lessThan")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.number_greaterThan")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.number_lessEqual")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("compare.number_greaterEqual")
                .unwrap_or(SymbolId(0)),
        ];

        let compare_namespace_id = self.create_symbol(
            "compare".to_string(),
            SymbolKind::Namespace {
                functions: compare_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(compare_namespace_id);

        // Create conditional namespace
        let conditional_functions = vec![
            self.lookup_symbol("conditional_integer")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("conditional_number")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("conditional_string")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("conditional_boolean")
                .unwrap_or(SymbolId(0)),
        ];

        let conditional_namespace_id = self.create_symbol(
            "conditional".to_string(),
            SymbolKind::Namespace {
                functions: conditional_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(conditional_namespace_id);

        // Create logical namespace
        let logical_functions = vec![
            self.lookup_symbol("logical_and").unwrap_or(SymbolId(0)),
            self.lookup_symbol("logical_or").unwrap_or(SymbolId(0)),
            self.lookup_symbol("logical_not").unwrap_or(SymbolId(0)),
        ];

        let logical_namespace_id = self.create_symbol(
            "logical".to_string(),
            SymbolKind::Namespace {
                functions: logical_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(logical_namespace_id);

        // Create input namespace
        let input_functions = vec![
            self.lookup_symbol("input").unwrap_or(SymbolId(0)),
            self.lookup_symbol("input_integer").unwrap_or(SymbolId(0)),
            self.lookup_symbol("input_number").unwrap_or(SymbolId(0)),
            self.lookup_symbol("input_yesNo").unwrap_or(SymbolId(0)),
        ];

        let input_namespace_id = self.create_symbol(
            "input".to_string(),
            SymbolKind::Namespace {
                functions: input_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
            },
        );
        self.builtins.insert(input_namespace_id);
    }
}

impl Default for GlobalSymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_table_creation() {
        let table = GlobalSymbolTable::new();

        // Should have global scope
        assert!(table.scopes.contains_key(&ScopeId(0)));

        // Should have built-in functions
        assert!(!table.builtins.is_empty());

        // Built-ins should be in global scope
        let print_symbol = table.lookup_symbol("print");
        assert!(print_symbol.is_some());
        assert!(table.is_builtin(print_symbol.unwrap()));
    }

    #[test]
    fn test_scope_management() {
        let mut table = GlobalSymbolTable::new();

        // Create function scope
        let func_scope = table.create_scope(
            None,
            ScopeType::Function {
                function_id: SymbolId(999),
            },
        );
        table.enter_scope(func_scope);

        // Add parameter
        let param_id = table.create_symbol(
            "x".to_string(),
            SymbolKind::Parameter {
                param_type: HirType::Integer,
            },
            func_scope,
            SourceLocation {
                file: "test".to_string(),
                line: 1,
                column: 1,
            },
        );

        // Should find parameter in function scope
        assert_eq!(table.lookup_symbol("x"), Some(param_id));

        // Exit scope
        table.exit_scope();

        // Should not find parameter in global scope
        assert_eq!(table.lookup_symbol("x"), None);
    }

    #[test]
    fn test_class_member_lookup() {
        let mut table = GlobalSymbolTable::new();

        // Create class
        let class_id = table.create_symbol(
            "TestClass".to_string(),
            SymbolKind::Class {
                fields: vec![],
                methods: vec![],
                parent: None,
            },
            table.current_scope_id(),
            SourceLocation {
                file: "test".to_string(),
                line: 1,
                column: 1,
            },
        );

        // Create field
        let field_id = table.create_symbol(
            "field".to_string(),
            SymbolKind::Field {
                class_id,
                field_type: HirType::Integer,
            },
            table.current_scope_id(),
            SourceLocation {
                file: "test".to_string(),
                line: 2,
                column: 1,
            },
        );

        // Update class to include field
        if let Some(symbol) = table.get_symbol_mut(class_id) {
            if let SymbolKind::Class { fields, .. } = &mut symbol.kind {
                fields.push(field_id);
            }
        }

        // Should find field in class
        assert_eq!(table.lookup_class_member(class_id, "field"), Some(field_id));
        assert_eq!(table.lookup_class_member(class_id, "nonexistent"), None);
    }
}
