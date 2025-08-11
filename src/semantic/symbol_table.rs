//! Comprehensive symbol table and scope management for Clean Language

use crate::ast::{FunctionModifier, SourceLocation, Type, Visibility};
use std::collections::HashMap;
use std::fmt;

/// Represents different kinds of symbols in the language
#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Variable {
        type_: Type,
        is_mutable: bool,
        is_initialized: bool,
    },
    Function {
        parameters: Vec<Type>,
        return_type: Type,
        visibility: Visibility,
        modifiers: Vec<FunctionModifier>,
        is_async: bool,
    },
    Class {
        fields: HashMap<String, Type>,
        methods: HashMap<String, Type>,
        base_class: Option<String>,
        visibility: Visibility,
    },
    Parameter {
        type_: Type,
        default_value: Option<String>, // String representation of default value
    },
    Constant {
        type_: Type,
        value: String, // String representation of the constant value
    },
}

/// Symbol information including metadata
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Option<SourceLocation>,
    pub scope_level: usize,
    pub is_used: bool,
    pub is_exported: bool,
}

impl Symbol {
    pub fn new(
        name: String,
        kind: SymbolKind,
        location: Option<SourceLocation>,
        scope_level: usize,
    ) -> Self {
        Self {
            name,
            kind,
            location,
            scope_level,
            is_used: false,
            is_exported: false,
        }
    }

    /// Get the type of this symbol
    pub fn get_type(&self) -> Type {
        match &self.kind {
            SymbolKind::Variable { type_, .. } => type_.clone(),
            SymbolKind::Function { return_type, .. } => return_type.clone(),
            SymbolKind::Class { .. } => Type::Object(self.name.clone()),
            SymbolKind::Parameter { type_, .. } => type_.clone(),
            SymbolKind::Constant { type_, .. } => type_.clone(),
        }
    }

    /// Check if this symbol is a function
    pub fn is_function(&self) -> bool {
        matches!(self.kind, SymbolKind::Function { .. })
    }

    /// Check if this symbol is a variable
    pub fn is_variable(&self) -> bool {
        matches!(self.kind, SymbolKind::Variable { .. })
    }

    /// Check if this symbol is a class
    pub fn is_class(&self) -> bool {
        matches!(self.kind, SymbolKind::Class { .. })
    }

    /// Mark this symbol as used
    pub fn mark_used(&mut self) {
        self.is_used = true;
    }
}

/// Represents a scope in the program
#[derive(Debug, Clone)]
pub struct ScopeInfo {
    pub scope_type: ScopeType,
    pub parent_scope: Option<usize>, // Index into the scope stack
    pub symbols: HashMap<String, Symbol>,
    pub level: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScopeType {
    Global,
    Function(String), // Function name
    Class(String),    // Class name
    Block,
    Loop,
    Conditional,
    TryCatch,
}

impl fmt::Display for ScopeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScopeType::Global => write!(f, "global"),
            ScopeType::Function(name) => write!(f, "function '{}'", name),
            ScopeType::Class(name) => write!(f, "class '{}'", name),
            ScopeType::Block => write!(f, "block"),
            ScopeType::Loop => write!(f, "loop"),
            ScopeType::Conditional => write!(f, "conditional"),
            ScopeType::TryCatch => write!(f, "try-catch"),
        }
    }
}

/// Comprehensive symbol table with hierarchical scopes
#[derive(Debug)]
pub struct SymbolTable {
    scopes: Vec<ScopeInfo>,
    current_scope: usize,
    scope_counter: usize,
}

impl SymbolTable {
    pub fn new() -> Self {
        let global_scope = ScopeInfo {
            scope_type: ScopeType::Global,
            parent_scope: None,
            symbols: HashMap::new(),
            level: 0,
        };

        Self {
            scopes: vec![global_scope],
            current_scope: 0,
            scope_counter: 1,
        }
    }

    /// Enter a new scope
    pub fn enter_scope(&mut self, scope_type: ScopeType) -> usize {
        let new_scope = ScopeInfo {
            scope_type,
            parent_scope: Some(self.current_scope),
            symbols: HashMap::new(),
            level: self.scopes[self.current_scope].level + 1,
        };

        let new_scope_id = self.scope_counter;
        self.scopes.push(new_scope);
        self.current_scope = new_scope_id;
        self.scope_counter += 1;
        new_scope_id
    }

    /// Exit the current scope
    pub fn exit_scope(&mut self) -> Result<Vec<Symbol>, String> {
        if self.current_scope == 0 {
            return Err("Cannot exit global scope".to_string());
        }

        let exited_scope = &self.scopes[self.current_scope];
        let unused_symbols: Vec<Symbol> = exited_scope
            .symbols
            .values()
            .filter(|symbol| !symbol.is_used && symbol.is_variable())
            .cloned()
            .collect();

        // Return to parent scope
        if let Some(parent_scope) = exited_scope.parent_scope {
            self.current_scope = parent_scope;
        }

        Ok(unused_symbols)
    }

    /// Define a symbol in the current scope
    pub fn define_symbol(&mut self, symbol: Symbol) -> Result<(), String> {
        let current_scope = &mut self.scopes[self.current_scope];

        // Check for redefinition in current scope
        if current_scope.symbols.contains_key(&symbol.name) {
            return Err(format!(
                "Symbol '{}' is already defined in {} scope",
                symbol.name, current_scope.scope_type
            ));
        }

        current_scope.symbols.insert(symbol.name.clone(), symbol);
        Ok(())
    }

    /// Define a variable in the current scope
    pub fn define_variable(
        &mut self,
        name: String,
        type_: Type,
        location: Option<SourceLocation>,
        is_mutable: bool,
    ) -> Result<(), String> {
        let symbol = Symbol::new(
            name,
            SymbolKind::Variable {
                type_,
                is_mutable,
                is_initialized: false,
            },
            location,
            self.scopes[self.current_scope].level,
        );
        self.define_symbol(symbol)
    }

    /// Define a function in the current scope
    pub fn define_function(
        &mut self,
        name: String,
        parameters: Vec<Type>,
        return_type: Type,
        location: Option<SourceLocation>,
        visibility: Visibility,
        modifiers: Vec<FunctionModifier>,
        is_async: bool,
    ) -> Result<(), String> {
        let symbol = Symbol::new(
            name,
            SymbolKind::Function {
                parameters,
                return_type,
                visibility,
                modifiers,
                is_async,
            },
            location,
            self.scopes[self.current_scope].level,
        );
        self.define_symbol(symbol)
    }

    /// Define a class in the current scope
    pub fn define_class(
        &mut self,
        name: String,
        fields: HashMap<String, Type>,
        methods: HashMap<String, Type>,
        base_class: Option<String>,
        location: Option<SourceLocation>,
        visibility: Visibility,
    ) -> Result<(), String> {
        let symbol = Symbol::new(
            name,
            SymbolKind::Class {
                fields,
                methods,
                base_class,
                visibility,
            },
            location,
            self.scopes[self.current_scope].level,
        );
        self.define_symbol(symbol)
    }

    /// Lookup a symbol starting from current scope and traversing up
    pub fn lookup_symbol(&self, name: &str) -> Option<&Symbol> {
        let mut current_scope_id = self.current_scope;

        loop {
            let scope = &self.scopes[current_scope_id];

            if let Some(symbol) = scope.symbols.get(name) {
                return Some(symbol);
            }

            // Move to parent scope
            match scope.parent_scope {
                Some(parent_id) => current_scope_id = parent_id,
                None => break,
            }
        }

        None
    }

    /// Lookup a symbol and mark it as used
    pub fn lookup_and_use_symbol(&mut self, name: &str) -> Option<Type> {
        // First find the symbol
        let symbol_info = self.lookup_symbol_with_scope(name)?;
        let (scope_id, symbol_type) = symbol_info;

        // Mark it as used
        if let Some(symbol) = self.scopes[scope_id].symbols.get_mut(name) {
            symbol.mark_used();
        }

        Some(symbol_type)
    }

    /// Lookup a symbol with scope information
    fn lookup_symbol_with_scope(&self, name: &str) -> Option<(usize, Type)> {
        let mut current_scope_id = self.current_scope;

        loop {
            let scope = &self.scopes[current_scope_id];

            if let Some(symbol) = scope.symbols.get(name) {
                return Some((current_scope_id, symbol.get_type()));
            }

            // Move to parent scope
            match scope.parent_scope {
                Some(parent_id) => current_scope_id = parent_id,
                None => break,
            }
        }

        None
    }

    /// Check if a symbol exists in the current scope (not parent scopes)
    pub fn exists_in_current_scope(&self, name: &str) -> bool {
        self.scopes[self.current_scope].symbols.contains_key(name)
    }

    /// Get all symbols in scope for error suggestions
    pub fn get_all_symbol_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut current_scope_id = self.current_scope;

        loop {
            let scope = &self.scopes[current_scope_id];
            names.extend(scope.symbols.keys().cloned());

            // Move to parent scope
            match scope.parent_scope {
                Some(parent_id) => current_scope_id = parent_id,
                None => break,
            }
        }

        // Remove duplicates while preserving order (most recent first)
        let mut unique_names = Vec::new();
        for name in names {
            if !unique_names.contains(&name) {
                unique_names.push(name);
            }
        }

        unique_names
    }

    /// Get the current scope type
    pub fn current_scope_type(&self) -> &ScopeType {
        &self.scopes[self.current_scope].scope_type
    }

    /// Get the current scope level
    pub fn current_scope_level(&self) -> usize {
        self.scopes[self.current_scope].level
    }

    /// Check if we're currently in a function scope
    pub fn in_function_scope(&self) -> bool {
        let mut current_scope_id = self.current_scope;

        loop {
            let scope = &self.scopes[current_scope_id];

            if matches!(scope.scope_type, ScopeType::Function(_)) {
                return true;
            }

            // Move to parent scope
            match scope.parent_scope {
                Some(parent_id) => current_scope_id = parent_id,
                None => break,
            }
        }

        false
    }

    /// Check if we're currently in a class scope
    pub fn in_class_scope(&self) -> bool {
        let mut current_scope_id = self.current_scope;

        loop {
            let scope = &self.scopes[current_scope_id];

            if matches!(scope.scope_type, ScopeType::Class(_)) {
                return true;
            }

            // Move to parent scope
            match scope.parent_scope {
                Some(parent_id) => current_scope_id = parent_id,
                None => break,
            }
        }

        false
    }

    /// Check if we're currently in a loop scope
    pub fn in_loop_scope(&self) -> bool {
        let mut current_scope_id = self.current_scope;

        loop {
            let scope = &self.scopes[current_scope_id];

            if matches!(scope.scope_type, ScopeType::Loop) {
                return true;
            }

            // Move to parent scope
            match scope.parent_scope {
                Some(parent_id) => current_scope_id = parent_id,
                None => break,
            }
        }

        false
    }

    /// Get current function name if in function scope
    pub fn current_function_name(&self) -> Option<&str> {
        let mut current_scope_id = self.current_scope;

        loop {
            let scope = &self.scopes[current_scope_id];

            if let ScopeType::Function(name) = &scope.scope_type {
                return Some(name);
            }

            // Move to parent scope
            match scope.parent_scope {
                Some(parent_id) => current_scope_id = parent_id,
                None => break,
            }
        }

        None
    }

    /// Get current class name if in class scope
    pub fn current_class_name(&self) -> Option<&str> {
        let mut current_scope_id = self.current_scope;

        loop {
            let scope = &self.scopes[current_scope_id];

            if let ScopeType::Class(name) = &scope.scope_type {
                return Some(name);
            }

            // Move to parent scope
            match scope.parent_scope {
                Some(parent_id) => current_scope_id = parent_id,
                None => break,
            }
        }

        None
    }

    /// Get all unused symbols for warnings
    pub fn get_unused_symbols(&self) -> Vec<&Symbol> {
        let mut unused = Vec::new();

        for scope in &self.scopes {
            for symbol in scope.symbols.values() {
                if !symbol.is_used && symbol.is_variable() && scope.scope_type != ScopeType::Global
                {
                    unused.push(symbol);
                }
            }
        }

        unused
    }

    /// Print the symbol table for debugging
    pub fn debug_print(&self) {
        println!("Symbol Table Debug:");
        for (i, scope) in self.scopes.iter().enumerate() {
            let current_marker = if i == self.current_scope {
                " <- CURRENT"
            } else {
                ""
            };
            println!(
                "  Scope {}: {} (level: {}){}",
                i, scope.scope_type, scope.level, current_marker
            );

            for (name, symbol) in &scope.symbols {
                let used_marker = if symbol.is_used { "✓" } else { "✗" };
                println!("    {} {}: {:?}", used_marker, name, symbol.kind);
            }
        }
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}
