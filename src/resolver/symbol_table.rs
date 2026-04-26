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
        is_mutable: bool,
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
    /// State variable - persistent global variable with optional guard
    StateVariable {
        var_type: HirType,
        scope: crate::hir::HirStateScope, // App or Screen scope
        has_guard: bool,                  // Whether the state has a guard clause
        /// true when declared inside `computed:` — read-only, assignment is STATE004 error.
        /// spec/semantic-rules.md STATE004: "Cannot assign to computed state variable"
        is_computed: bool,
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
    /// True when the symbol was declared inside a `private:` sub-section.
    ///
    /// A private symbol may only be accessed from within its owning scope:
    /// - Private module function: callable only within the same module.
    /// - Private class field/method: accessible only within the class body.
    /// - Private state variable: accessible only within the same module.
    ///
    /// Violations are reported as SEM005 errors by the resolver.
    pub is_private: bool,
    /// The name of the class (or module) that owns this private symbol.
    /// Used by the resolver to determine whether access is permitted.
    /// `None` for public symbols (the field is irrelevant in that case).
    pub owner_scope_name: Option<String>,
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
    pub(crate) builtins: HashSet<SymbolId>,
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
                    byte_start: None,
                    byte_end: None,
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

    /// Add built-in static classes (Integer only - Math, StringUtils, String removed)
    fn add_builtin_static_classes(&mut self, global_scope: ScopeId) {
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
                byte_start: None,
                byte_end: None,
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
                    byte_start: None,
                    byte_end: None,
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
            is_private: false,
            owner_scope_name: None,
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
            tracing::warn!(
                symbol_name = %name,
                max_depth = MAX_SCOPE_DEPTH,
                "Maximum scope depth exceeded looking up symbol"
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

    /// Mark a symbol as private and record the name of its owning scope
    /// (the class name for class members, or `"<module>"` for module-level private items).
    ///
    /// Called by the resolver after creating a symbol that belongs to a `private:` sub-section.
    pub fn mark_as_private(&mut self, symbol_id: SymbolId, owner_scope_name: String) {
        if let Some(symbol) = self.symbols.get_mut(&symbol_id) {
            symbol.is_private = true;
            symbol.owner_scope_name = Some(owner_scope_name);
        }
    }

    /// Return `true` if the symbol is private and the access is from outside `owner_scope_name`.
    ///
    /// `access_context_name` is the name of the current class or `"<module>"` for module scope.
    /// Returns `false` (no violation) when the symbol is public or when the context matches the owner.
    pub fn is_private_access_violation(
        &self,
        symbol_id: SymbolId,
        access_context_name: &str,
    ) -> bool {
        if let Some(symbol) = self.symbols.get(&symbol_id) {
            if !symbol.is_private {
                return false;
            }
            match &symbol.owner_scope_name {
                Some(owner) => owner != access_context_name,
                None => false, // No owner stored — treat as non-private for safety.
            }
        } else {
            false
        }
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

    /// Get all symbols in the symbol table (for MIR building)
    /// Returns a reference to the internal symbols HashMap
    pub fn all_symbols(&self) -> &HashMap<SymbolId, Symbol> {
        &self.symbols
    }

    /// Add built-in namespace functions (e.g., math.sin for math.sin calls)
    /// NOTE: Use dot notation to match WASM function names
    fn add_builtin_namespace_functions(&mut self, global_scope: ScopeId) {
        let namespace_functions = vec![
            // Math namespace functions - using dot notation to match WASM
            ("math.sin", vec![HirType::Number], HirType::Number),
            ("math.cos", vec![HirType::Number], HirType::Number),
            ("math.tan", vec![HirType::Number], HirType::Number),
            ("math.asin", vec![HirType::Number], HirType::Number),
            ("math.acos", vec![HirType::Number], HirType::Number),
            ("math.atan", vec![HirType::Number], HirType::Number),
            (
                "math.atan2",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            ("math.sinh", vec![HirType::Number], HirType::Number),
            ("math.cosh", vec![HirType::Number], HirType::Number),
            ("math.tanh", vec![HirType::Number], HirType::Number),
            ("math.log", vec![HirType::Number], HirType::Number),
            ("math.ln", vec![HirType::Number], HirType::Number),
            ("math.log10", vec![HirType::Number], HirType::Number),
            ("math.log2", vec![HirType::Number], HirType::Number),
            ("math.exp", vec![HirType::Number], HirType::Number),
            ("math.exp2", vec![HirType::Number], HirType::Number),
            ("math.abs", vec![HirType::Number], HirType::Number),
            ("math.floor", vec![HirType::Number], HirType::Number),
            ("math.ceil", vec![HirType::Number], HirType::Number),
            ("math.round", vec![HirType::Number], HirType::Number),
            ("math.sqrt", vec![HirType::Number], HirType::Number),
            ("math.trunc", vec![HirType::Number], HirType::Number),
            ("math.pi", vec![], HirType::Number),
            ("math.e", vec![], HirType::Number),
            ("math.tau", vec![], HirType::Number),
            (
                "math.pow",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math.max",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math.min",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            // String namespace functions
            // Use string.length to match semantic analyzer and codegen
            ("string.length", vec![HirType::String], HirType::Integer),
            (
                "string.concat",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            ("string.isEmpty", vec![HirType::String], HirType::Boolean),
            ("string.isBlank", vec![HirType::String], HirType::Boolean),
            (
                "string.indexOf",
                vec![HirType::String, HirType::String],
                HirType::Integer,
            ),
            (
                "string.lastIndexOf",
                vec![HirType::String, HirType::String],
                HirType::Integer,
            ),
            (
                "string.substring",
                vec![HirType::String, HirType::Integer, HirType::Integer],
                HirType::String,
            ),
            ("string.toUpperCase", vec![HirType::String], HirType::String),
            ("string.toLowerCase", vec![HirType::String], HirType::String),
            ("string.trim", vec![HirType::String], HirType::String),
            ("string.trimStart", vec![HirType::String], HirType::String),
            ("string.trimEnd", vec![HirType::String], HirType::String),
            (
                "string.split",
                vec![HirType::String, HirType::String],
                HirType::List(Box::new(HirType::String)),
            ),
            (
                "string.join",
                vec![HirType::List(Box::new(HirType::String)), HirType::String],
                HirType::String,
            ),
            (
                "string.padStart",
                vec![HirType::String, HirType::Integer, HirType::String],
                HirType::String,
            ),
            (
                "string.padEnd",
                vec![HirType::String, HirType::Integer, HirType::String],
                HirType::String,
            ),
            (
                "string.charAt",
                vec![HirType::String, HirType::Integer],
                HirType::String,
            ),
            (
                "string.charCodeAt",
                vec![HirType::String, HirType::Integer],
                HirType::Integer,
            ),
            (
                "string.contains",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "string.startsWith",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "string.endsWith",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "string.replace",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "string.replaceAll",
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
            // Both list.size and list.length are supported; length is canonical per spec
            (
                "list.size",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Integer,
            ),
            (
                "list.length",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Integer,
            ),
            (
                "list.isEmpty",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Boolean,
            ),
            (
                "list.isNotEmpty",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Boolean,
            ),
            (
                "list.push",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Void,
            ),
            (
                "list.pop",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.removeLast",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.get",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Integer],
                HirType::Number,
            ),
            (
                "list.remove",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Integer],
                HirType::Number,
            ),
            (
                "list.peek",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.add",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::List(Box::new(HirType::Number)), // Returns modified list per spec
            ),
            (
                "list.contains",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Boolean,
            ),
            (
                "list.insert",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::Integer,
                    HirType::Number,
                ],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.indexOf",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Integer,
            ),
            (
                "list.lastIndexOf",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Integer,
            ),
            (
                "list.first",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.last",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.reverse",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.sort",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.slice",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::Integer,
                    HirType::Integer,
                ],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.concat",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::List(Box::new(HirType::Number)),
                ],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.fill",
                vec![
                    HirType::Integer, // size parameter
                    HirType::Number,  // value parameter (generic placeholder)
                ],
                HirType::List(Box::new(HirType::Number)), // returns new list
            ),
            (
                "list.range",
                vec![HirType::Integer, HirType::Integer],
                HirType::List(Box::new(HirType::Integer)),
            ),
            (
                "list.join",
                vec![HirType::List(Box::new(HirType::Number)), HirType::String],
                HirType::String,
            ),
            // HTTP namespace functions
            ("http.get", vec![HirType::String], HirType::String),
            (
                "http.post",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http.put",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http.patch",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            ("http.delete", vec![HirType::String], HirType::String),
            // File namespace functions
            ("file.read", vec![HirType::String], HirType::String),
            (
                "file.write",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "file.append",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            ("file.exists", vec![HirType::String], HirType::Boolean),
            ("file.delete", vec![HirType::String], HirType::Boolean),
            // JSON namespace functions  // BOOK: json-module
            // JSON parse functions return Any type (dynamic data structures)
            ("json.textToData", vec![HirType::String], HirType::Any),
            ("json.tryTextToData", vec![HirType::String], HirType::Any),
            // JSON stringify functions take Any type and return string
            ("json.dataToText", vec![HirType::Any], HirType::String),
            ("json.prettyDataToText", vec![HirType::Any], HirType::String),
            // Input namespace functions (input(), input.integer(), input.yesNo(), etc.)
            ("input", vec![HirType::String], HirType::String),
            ("input_string", vec![HirType::String], HirType::String),
            ("input_integer", vec![HirType::String], HirType::Integer),
            ("input_number", vec![HirType::String], HirType::Number),
            ("input_yesNo", vec![HirType::String], HirType::Boolean),
            (
                "input_integerWithDefault",
                vec![HirType::String, HirType::Integer],
                HirType::Integer,
            ),
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
                    byte_start: None,
                    byte_end: None,
                },
            );
            self.builtins.insert(symbol_id);
        }
    }

    /// Add built-in namespace identifiers (math, string, etc.)
    fn add_builtin_namespace_identifiers(&mut self, global_scope: ScopeId) {
        // Math namespace - so "math" is recognized as a valid identifier
        let math_functions = vec![
            self.lookup_symbol("math.sin").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.cos").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.tan").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.asin").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.acos").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.atan").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.atan2").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.sinh").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.cosh").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.tanh").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.log").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.ln").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.log10").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.log2").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.exp").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.exp2").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.abs").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.floor").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.ceil").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.round").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.sqrt").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.trunc").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.pi").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.e").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.tau").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.pow").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.max").unwrap_or(SymbolId(0)),
            self.lookup_symbol("math.min").unwrap_or(SymbolId(0)),
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
                byte_start: None,
                byte_end: None,
            },
        );
        self.builtins.insert(math_namespace_id);

        // String namespace - so "string" is recognized as a valid identifier
        // Use string.length to match semantic analyzer and codegen
        let string_functions = vec![
            self.lookup_symbol("string.length").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.concat").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.isEmpty").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.isBlank").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.indexOf").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.lastIndexOf")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.substring")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.toUpperCase")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.toLowerCase")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.trim").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.trimStart")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.trimEnd").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.contains").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.startsWith")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.endsWith").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.replace").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.replaceAll")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.split").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.join").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.padStart").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.padEnd").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.charAt").unwrap_or(SymbolId(0)),
            self.lookup_symbol("string.charCodeAt")
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
                byte_start: None,
                byte_end: None,
            },
        );
        self.builtins.insert(string_namespace_id);

        // Create list namespace
        // list.length is canonical per spec; list.size retained for compatibility
        let list_functions = vec![
            self.lookup_symbol("list.size").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.length").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.isEmpty").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.isNotEmpty").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.push").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.pop").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.removeLast").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.get").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.remove").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.peek").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.add").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.contains").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.insert").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.indexOf").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.lastIndexOf")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.first").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.last").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.reverse").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.sort").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.slice").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.concat").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.fill").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.range").unwrap_or(SymbolId(0)),
            self.lookup_symbol("list.join").unwrap_or(SymbolId(0)),
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
                byte_start: None,
                byte_end: None,
            },
        );
        self.builtins.insert(list_namespace_id);

        // Create http namespace
        let http_functions = vec![
            self.lookup_symbol("http.get").unwrap_or(SymbolId(0)),
            self.lookup_symbol("http.post").unwrap_or(SymbolId(0)),
            self.lookup_symbol("http.put").unwrap_or(SymbolId(0)),
            self.lookup_symbol("http.patch").unwrap_or(SymbolId(0)),
            self.lookup_symbol("http.delete").unwrap_or(SymbolId(0)),
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
                byte_start: None,
                byte_end: None,
            },
        );
        self.builtins.insert(http_namespace_id);

        // Create file namespace
        let file_functions = vec![
            self.lookup_symbol("file.read").unwrap_or(SymbolId(0)),
            self.lookup_symbol("file.write").unwrap_or(SymbolId(0)),
            self.lookup_symbol("file.append").unwrap_or(SymbolId(0)),
            self.lookup_symbol("file.exists").unwrap_or(SymbolId(0)),
            self.lookup_symbol("file.delete").unwrap_or(SymbolId(0)),
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
                byte_start: None,
                byte_end: None,
            },
        );
        self.builtins.insert(file_namespace_id);

        // Create json namespace  // BOOK: json-module
        let json_functions = vec![
            self.lookup_symbol("json.textToData").unwrap_or(SymbolId(0)),
            self.lookup_symbol("json.tryTextToData")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("json.dataToText").unwrap_or(SymbolId(0)),
            self.lookup_symbol("json.prettyDataToText")
                .unwrap_or(SymbolId(0)),
        ];

        let json_namespace_id = self.create_symbol(
            "json".to_string(),
            SymbolKind::Namespace {
                functions: json_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
                byte_start: None,
                byte_end: None,
            },
        );
        self.builtins.insert(json_namespace_id);

        // Create input namespace
        let input_functions = vec![
            self.lookup_symbol("input").unwrap_or(SymbolId(0)),
            self.lookup_symbol("input_string").unwrap_or(SymbolId(0)),
            self.lookup_symbol("input_integer").unwrap_or(SymbolId(0)),
            self.lookup_symbol("input_number").unwrap_or(SymbolId(0)),
            self.lookup_symbol("input_yesNo").unwrap_or(SymbolId(0)),
            self.lookup_symbol("input_integerWithDefault")
                .unwrap_or(SymbolId(0)),
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
                byte_start: None,
                byte_end: None,
            },
        );
        self.builtins.insert(input_namespace_id);

        // Create validator namespace
        let validator_functions = vec![
            self.lookup_symbol("validator.create")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.createWithName")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.run").unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.runField")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.validate")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.ok").unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.error").unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.isOk").unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.isError")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.getValue")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.getErrors")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.getFirstError")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.field").unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.required")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.optional")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.match").unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.range").unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.minLength")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.maxLength")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.custom")
                .unwrap_or(SymbolId(0)),
            self.lookup_symbol("validator.message")
                .unwrap_or(SymbolId(0)),
        ];

        let validator_namespace_id = self.create_symbol(
            "validator".to_string(),
            SymbolKind::Namespace {
                functions: validator_functions,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
                byte_start: None,
                byte_end: None,
            },
        );
        self.builtins.insert(validator_namespace_id);
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
                byte_start: None,
                byte_end: None,
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
                byte_start: None,
                byte_end: None,
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
                byte_start: None,
                byte_end: None,
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
