use crate::ast::Type;
use std::collections::HashMap;

#[derive(Clone)]
pub struct Scope {
    variables: HashMap<String, Type>,
    parent: Option<Box<Scope>>,
    scope_stack: Vec<HashMap<String, Type>>,
}

impl Scope {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            parent: None,
            scope_stack: Vec::new(),
        }
    }

    pub fn define_variable<S: Into<String>>(&mut self, name: S, type_: Type) {
        let name_str = name.into();
        if let Some(current_scope) = self.scope_stack.last_mut() {
            current_scope.insert(name_str, type_);
        } else {
            self.variables.insert(name_str, type_);
        }
    }

    pub fn declare_variable<S: Into<String>>(&mut self, name: S, type_: Type) {
        // Alias for define_variable for compatibility
        self.define_variable(name, type_);
    }

    pub fn lookup_variable(&self, name: &str) -> Option<Type> {
        // First check the current scope stack (most recent first)
        for scope in self.scope_stack.iter().rev() {
            if let Some(type_) = scope.get(name) {
                return Some(type_.clone());
            }
        }

        // Then check the base variables
        if let Some(type_) = self.variables.get(name) {
            return Some(type_.clone());
        }

        // Finally check parent scope
        self.parent.as_ref().and_then(|p| p.lookup_variable(name))
    }

    pub fn enter(&mut self) {
        self.scope_stack.push(HashMap::new());
    }

    pub fn exit(&mut self) {
        self.scope_stack.pop();
    }

    /// Get all variable names in the current scope for error suggestions
    pub fn get_all_variable_names(&self) -> Vec<String> {
        let mut names = Vec::new();

        // Add variables from scope stack (most recent first)
        for scope in self.scope_stack.iter().rev() {
            names.extend(scope.keys().cloned());
        }

        // Add base variables
        names.extend(self.variables.keys().cloned());

        // Add parent scope variables
        if let Some(parent) = &self.parent {
            names.extend(parent.get_all_variable_names());
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
}
