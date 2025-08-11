//! Comprehensive inheritance system validation for Clean Language
//!
//! This module handles all aspects of class inheritance validation including:
//! - Inheritance cycle detection
//! - Method overriding validation
//! - Field inheritance rules
//! - Constructor chaining validation
//! - Access control enforcement
//! - Base constructor call validation

use crate::ast::{Class, Constructor, Expression, Function, SourceLocation, Visibility};
use crate::error::CompilerError;
use std::collections::{HashMap, HashSet, VecDeque};

/// Comprehensive inheritance validator
#[derive(Debug)]
pub struct InheritanceValidator {
    /// Map of class name to class definition
    class_registry: HashMap<String, Class>,
    /// Cache for inheritance hierarchies to avoid repeated computation
    hierarchy_cache: HashMap<String, Vec<String>>,
    /// Cache for method resolution to speed up validation
    method_cache: HashMap<String, HashMap<String, Function>>,
}

impl InheritanceValidator {
    pub fn new() -> Self {
        Self {
            class_registry: HashMap::new(),
            hierarchy_cache: HashMap::new(),
            method_cache: HashMap::new(),
        }
    }

    /// Register a class for inheritance validation
    pub fn register_class(&mut self, class: Class) -> Result<(), CompilerError> {
        // Basic validation before registration
        self.validate_class_name(&class.name, &class.location)?;
        
        // Check for duplicate class names
        if self.class_registry.contains_key(&class.name) {
            return Err(CompilerError::type_error(
                format!("Class '{}' is already defined", class.name),
                Some("Choose a different class name or remove the duplicate definition".to_string()),
                class.location.clone(),
            ));
        }

        // Validate base class reference
        if let Some(ref base_class) = class.base_class {
            self.validate_base_class_reference(&class.name, base_class, &class.location)?;
        }

        // Clear caches as class hierarchy may have changed
        self.hierarchy_cache.clear();
        self.method_cache.clear();

        self.class_registry.insert(class.name.clone(), class);
        Ok(())
    }

    /// Comprehensive inheritance validation for all registered classes
    pub fn validate_inheritance(&mut self) -> Result<(), CompilerError> {
        // 1. Check for inheritance cycles
        self.detect_inheritance_cycles()?;
        
        // 2. Validate base class existence
        self.validate_base_class_existence()?;
        
        // 3. Validate constructor inheritance
        self.validate_constructor_inheritance()?;
        
        // 4. Validate method overriding rules
        self.validate_method_overriding()?;
        
        // 5. Validate field inheritance rules
        self.validate_field_inheritance()?;
        
        // 6. Validate access control rules
        self.validate_access_control()?;

        Ok(())
    }

    /// Get the complete inheritance hierarchy for a class (including the class itself)
    pub fn get_inheritance_hierarchy(&mut self, class_name: &str) -> Result<Vec<String>, CompilerError> {
        if let Some(cached) = self.hierarchy_cache.get(class_name) {
            return Ok(cached.clone());
        }

        let mut hierarchy = Vec::new();
        let mut visited = HashSet::new();
        let mut current = Some(class_name.to_string());

        while let Some(current_class) = current {
            // Prevent infinite loops
            if visited.contains(&current_class) {
                return Err(CompilerError::type_error(
                    format!("Inheritance cycle detected involving class '{}'", current_class),
                    Some("Remove circular inheritance relationships".to_string()),
                    None,
                ));
            }

            visited.insert(current_class.clone());
            hierarchy.push(current_class.clone());

            // Get parent class
            current = self.class_registry
                .get(&current_class)
                .and_then(|class| class.base_class.clone());
        }

        // Cache the result
        self.hierarchy_cache.insert(class_name.to_string(), hierarchy.clone());
        Ok(hierarchy)
    }

    /// Check if a class is a subclass of another class
    pub fn is_subclass_of(&mut self, child_class: &str, parent_class: &str) -> Result<bool, CompilerError> {
        let hierarchy = self.get_inheritance_hierarchy(child_class)?;
        Ok(hierarchy.contains(&parent_class.to_string()))
    }

    /// Get all methods available to a class (including inherited methods)
    pub fn get_available_methods(&mut self, class_name: &str) -> Result<HashMap<String, Function>, CompilerError> {
        if let Some(cached) = self.method_cache.get(class_name) {
            return Ok(cached.clone());
        }

        let mut methods = HashMap::new();
        let hierarchy = self.get_inheritance_hierarchy(class_name)?;

        // Process hierarchy from most base class to most derived
        for class_name_in_hierarchy in hierarchy.iter().rev() {
            if let Some(class) = self.class_registry.get(class_name_in_hierarchy) {
                for method in &class.methods {
                    // Override methods from parent classes
                    methods.insert(method.name.clone(), method.clone());
                }
            }
        }

        // Cache the result
        self.method_cache.insert(class_name.to_string(), methods.clone());
        Ok(methods)
    }

    /// Detect inheritance cycles using topological sort
    fn detect_inheritance_cycles(&self) -> Result<(), CompilerError> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adjacency_list: HashMap<String, Vec<String>> = HashMap::new();

        // Build the inheritance graph
        for (class_name, class) in &self.class_registry {
            in_degree.insert(class_name.clone(), 0);
            adjacency_list.insert(class_name.clone(), Vec::new());
        }

        for (class_name, class) in &self.class_registry {
            if let Some(ref base_class) = class.base_class {
                // Check if base class exists
                if !self.class_registry.contains_key(base_class) {
                    continue; // This will be caught in validate_base_class_existence
                }

                adjacency_list
                    .get_mut(base_class)
                    .unwrap()
                    .push(class_name.clone());
                *in_degree.get_mut(class_name).unwrap() += 1;
            }
        }

        // Kahn's algorithm for topological sorting
        let mut queue = VecDeque::new();
        let mut processed_count = 0;

        // Find all nodes with no incoming edges
        for (class_name, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(class_name.clone());
            }
        }

        while let Some(current_class) = queue.pop_front() {
            processed_count += 1;

            if let Some(children) = adjacency_list.get(&current_class) {
                for child in children {
                    let child_degree = in_degree.get_mut(child).unwrap();
                    *child_degree -= 1;
                    if *child_degree == 0 {
                        queue.push_back(child.clone());
                    }
                }
            }
        }

        // If not all nodes were processed, there's a cycle
        if processed_count != self.class_registry.len() {
            // Find a class involved in the cycle for better error reporting
            for (class_name, &degree) in &in_degree {
                if degree > 0 {
                    let class = self.class_registry.get(class_name).unwrap();
                    return Err(CompilerError::type_error(
                        format!("Inheritance cycle detected involving class '{}'", class_name),
                        Some("Remove circular inheritance relationships".to_string()),
                        class.location.clone(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Validate that all base classes exist
    fn validate_base_class_existence(&self) -> Result<(), CompilerError> {
        for (class_name, class) in &self.class_registry {
            if let Some(ref base_class) = class.base_class {
                if !self.class_registry.contains_key(base_class) {
                    return Err(CompilerError::type_error(
                        format!("Base class '{}' for class '{}' does not exist", base_class, class_name),
                        Some(format!("Define class '{}' before using it as a base class", base_class)),
                        class.location.clone(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Validate constructor inheritance rules
    fn validate_constructor_inheritance(&self) -> Result<(), CompilerError> {
        for (class_name, class) in &self.class_registry {
            if let Some(ref base_class_name) = class.base_class {
                if let Some(ref constructor) = class.constructor {
                    self.validate_base_constructor_call(class_name, constructor, base_class_name)?;
                }
            }
        }
        Ok(())
    }

    /// Validate that constructors properly call base constructors
    fn validate_base_constructor_call(
        &self,
        class_name: &str,
        constructor: &Constructor,
        base_class_name: &str,
    ) -> Result<(), CompilerError> {
        let base_class = self.class_registry.get(base_class_name).ok_or_else(|| {
            CompilerError::type_error(
                format!("Base class '{}' not found", base_class_name),
                None,
                constructor.location.clone(),
            )
        })?;

        // If base class has a constructor, derived class must call base()
        if base_class.constructor.is_some() {
            let has_base_call = self.has_base_constructor_call(&constructor.body);
            if !has_base_call {
                return Err(CompilerError::type_error(
                    format!("Constructor in class '{}' must call base constructor", class_name),
                    Some("Add 'base(args...)' call to constructor".to_string()),
                    constructor.location.clone(),
                ));
            }
        }

        Ok(())
    }

    /// Check if constructor body contains a base() call
    fn has_base_constructor_call(&self, statements: &[crate::ast::Statement]) -> bool {
        for statement in statements {
            match statement {
                crate::ast::Statement::Expression { expr, .. } => {
                    if matches!(expr, Expression::BaseCall { .. }) {
                        return true;
                    }
                }
                // Recursively check nested statements
                crate::ast::Statement::If { then_branch, else_branch, .. } => {
                    if self.has_base_constructor_call(then_branch) {
                        return true;
                    }
                    if let Some(else_stmts) = else_branch {
                        if self.has_base_constructor_call(else_stmts) {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Validate method overriding rules
    fn validate_method_overriding(&self) -> Result<(), CompilerError> {
        for (class_name, class) in &self.class_registry {
            if let Some(ref base_class_name) = class.base_class {
                self.validate_class_method_overrides(class_name, class, base_class_name)?;
            }
        }
        Ok(())
    }

    /// Validate method overrides for a specific class
    fn validate_class_method_overrides(
        &self,
        class_name: &str,
        class: &Class,
        base_class_name: &str,
    ) -> Result<(), CompilerError> {
        let base_class = self.class_registry.get(base_class_name).ok_or_else(|| {
            CompilerError::type_error(
                format!("Base class '{}' not found", base_class_name),
                None,
                class.location.clone(),
            )
        })?;

        for method in &class.methods {
            // Check if this method overrides a base class method
            if let Some(base_method) = base_class.methods.iter().find(|m| m.name == method.name) {
                self.validate_method_override_compatibility(method, base_method, class_name)?;
            }
        }

        // Recursively check base classes
        if let Some(ref grandparent) = base_class.base_class {
            self.validate_class_method_overrides(class_name, class, grandparent)?;
        }

        Ok(())
    }

    /// Validate that method override is compatible with base method
    fn validate_method_override_compatibility(
        &self,
        derived_method: &Function,
        base_method: &Function,
        class_name: &str,
    ) -> Result<(), CompilerError> {
        // Check parameter count
        if derived_method.parameters.len() != base_method.parameters.len() {
            return Err(CompilerError::type_error(
                format!(
                    "Method '{}' in class '{}' has {} parameters, but overridden method has {}",
                    derived_method.name,
                    class_name,
                    derived_method.parameters.len(),
                    base_method.parameters.len()
                ),
                Some("Match the parameter count of the overridden method".to_string()),
                derived_method.location.clone(),
            ));
        }

        // Check parameter types
        for (i, (derived_param, base_param)) in derived_method
            .parameters
            .iter()
            .zip(base_method.parameters.iter())
            .enumerate()
        {
            if derived_param.type_ != base_param.type_ {
                return Err(CompilerError::type_error(
                    format!(
                        "Parameter {} of method '{}' in class '{}' has type '{}', but overridden method expects '{}'",
                        i + 1,
                        derived_method.name,
                        class_name,
                        derived_param.type_,
                        base_param.type_
                    ),
                    Some("Match the parameter types of the overridden method".to_string()),
                    derived_method.location.clone(),
                ));
            }
        }

        // Check return type
        if derived_method.return_type != base_method.return_type {
            return Err(CompilerError::type_error(
                format!(
                    "Method '{}' in class '{}' returns '{}', but overridden method returns '{}'",
                    derived_method.name,
                    class_name,
                    derived_method.return_type,
                    base_method.return_type
                ),
                Some("Match the return type of the overridden method".to_string()),
                derived_method.location.clone(),
            ));
        }

        // Check visibility (cannot reduce visibility)
        if !self.is_visibility_compatible(&derived_method.visibility, &base_method.visibility) {
            return Err(CompilerError::type_error(
                format!(
                    "Method '{}' in class '{}' cannot reduce visibility from {:?} to {:?}",
                    derived_method.name,
                    class_name,
                    base_method.visibility,
                    derived_method.visibility
                ),
                Some("Cannot reduce visibility when overriding methods".to_string()),
                derived_method.location.clone(),
            ));
        }

        Ok(())
    }

    /// Check if visibility change is compatible (cannot reduce visibility)
    fn is_visibility_compatible(&self, derived_visibility: &Visibility, base_visibility: &Visibility) -> bool {
        match (base_visibility, derived_visibility) {
            (Visibility::Public, Visibility::Private) => false, // Cannot reduce from public to private
            _ => true, // All other combinations are valid
        }
    }

    /// Validate field inheritance rules
    fn validate_field_inheritance(&self) -> Result<(), CompilerError> {
        for (class_name, class) in &self.class_registry {
            if let Some(ref base_class_name) = class.base_class {
                self.validate_field_name_conflicts(class_name, class, base_class_name)?;
            }
        }
        Ok(())
    }

    /// Check for field name conflicts in inheritance hierarchy
    fn validate_field_name_conflicts(
        &self,
        class_name: &str,
        class: &Class,
        base_class_name: &str,
    ) -> Result<(), CompilerError> {
        let base_class = self.class_registry.get(base_class_name).ok_or_else(|| {
            CompilerError::type_error(
                format!("Base class '{}' not found", base_class_name),
                None,
                class.location.clone(),
            )
        })?;

        for field in &class.fields {
            // Check if this field shadows a base class field
            if let Some(base_field) = base_class.fields.iter().find(|f| f.name == field.name) {
                // In Clean Language, field shadowing is generally not allowed
                // to prevent confusion and maintain clear inheritance semantics
                return Err(CompilerError::type_error(
                    format!(
                        "Field '{}' in class '{}' shadows field from base class '{}'",
                        field.name, class_name, base_class_name
                    ),
                    Some("Choose a different field name or remove the conflicting field".to_string()),
                    class.location.clone(),
                ));
            }
        }

        // Recursively check base classes
        if let Some(ref grandparent) = base_class.base_class {
            self.validate_field_name_conflicts(class_name, class, grandparent)?;
        }

        Ok(())
    }

    /// Validate access control rules
    fn validate_access_control(&self) -> Result<(), CompilerError> {
        // This would validate that private members are only accessed within the class
        // and that proper access control is maintained in inheritance
        for (class_name, class) in &self.class_registry {
            self.validate_class_access_control(class_name, class)?;
        }
        Ok(())
    }

    /// Validate access control for a specific class
    fn validate_class_access_control(&self, _class_name: &str, _class: &Class) -> Result<(), CompilerError> {
        // TODO: Implement comprehensive access control validation
        // This would involve analyzing method bodies to ensure private fields/methods
        // are not accessed from inappropriate contexts
        Ok(())
    }

    /// Validate class name
    fn validate_class_name(&self, name: &str, location: &Option<SourceLocation>) -> Result<(), CompilerError> {
        if name.is_empty() {
            return Err(CompilerError::type_error(
                "Class name cannot be empty".to_string(),
                Some("Provide a valid class name".to_string()),
                location.clone(),
            ));
        }

        // Check for reserved names
        if self.is_reserved_name(name) {
            return Err(CompilerError::type_error(
                format!("'{}' is a reserved name and cannot be used as a class name", name),
                Some("Choose a different class name".to_string()),
                location.clone(),
            ));
        }

        Ok(())
    }

    /// Check if a name is reserved
    fn is_reserved_name(&self, name: &str) -> bool {
        matches!(
            name,
            "string" | "integer" | "number" | "boolean" | "void" | "any" | "list" | "matrix"
                | "Object" | "String" | "Integer" | "Number" | "Boolean"
        )
    }

    /// Validate base class reference
    fn validate_base_class_reference(
        &self,
        class_name: &str,
        base_class_name: &str,
        location: &Option<SourceLocation>,
    ) -> Result<(), CompilerError> {
        if class_name == base_class_name {
            return Err(CompilerError::type_error(
                format!("Class '{}' cannot inherit from itself", class_name),
                Some("Remove the self-inheritance or choose a different base class".to_string()),
                location.clone(),
            ));
        }

        Ok(())
    }

    /// Get class definition by name
    pub fn get_class(&self, name: &str) -> Option<&Class> {
        self.class_registry.get(name)
    }

    /// Get all registered classes
    pub fn get_all_classes(&self) -> &HashMap<String, Class> {
        &self.class_registry
    }

    /// Clear all caches (useful for testing or after major changes)
    pub fn clear_caches(&mut self) {
        self.hierarchy_cache.clear();
        self.method_cache.clear();
    }
}

impl Default for InheritanceValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Parameter, SourceLocation};

    fn create_test_location() -> Option<SourceLocation> {
        Some(SourceLocation {
            line: 1,
            column: 1,
            file: "test.cln".to_string(),
        })
    }

    fn create_test_class(name: &str, base_class: Option<String>) -> Class {
        Class {
            name: name.to_string(),
            type_parameters: Vec::new(),
            description: None,
            base_class,
            base_class_type_args: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            constructor: None,
            location: create_test_location(),
        }
    }

    #[test]
    fn test_simple_inheritance() {
        let mut validator = InheritanceValidator::new();
        
        let parent_class = create_test_class("Parent", None);
        let child_class = create_test_class("Child", Some("Parent".to_string()));
        
        assert!(validator.register_class(parent_class).is_ok());
        assert!(validator.register_class(child_class).is_ok());
        assert!(validator.validate_inheritance().is_ok());
    }

    #[test]
    fn test_inheritance_cycle_detection() {
        let mut validator = InheritanceValidator::new();
        
        let class_a = create_test_class("A", Some("B".to_string()));
        let class_b = create_test_class("B", Some("A".to_string()));
        
        assert!(validator.register_class(class_a).is_ok());
        assert!(validator.register_class(class_b).is_ok());
        assert!(validator.validate_inheritance().is_err());
    }

    #[test]
    fn test_missing_base_class() {
        let mut validator = InheritanceValidator::new();
        
        let child_class = create_test_class("Child", Some("NonExistent".to_string()));
        
        assert!(validator.register_class(child_class).is_ok());
        assert!(validator.validate_inheritance().is_err());
    }

    #[test]
    fn test_self_inheritance() {
        let mut validator = InheritanceValidator::new();
        
        let self_inheriting_class = create_test_class("SelfInheriting", Some("SelfInheriting".to_string()));
        
        assert!(validator.register_class(self_inheriting_class).is_err());
    }

    #[test]
    fn test_reserved_class_name() {
        let mut validator = InheritanceValidator::new();
        
        let reserved_class = create_test_class("string", None);
        
        assert!(validator.register_class(reserved_class).is_err());
    }

    #[test]
    fn test_inheritance_hierarchy() {
        let mut validator = InheritanceValidator::new();
        
        let grandparent_class = create_test_class("GrandParent", None);
        let parent_class = create_test_class("Parent", Some("GrandParent".to_string()));
        let child_class = create_test_class("Child", Some("Parent".to_string()));
        
        assert!(validator.register_class(grandparent_class).is_ok());
        assert!(validator.register_class(parent_class).is_ok());
        assert!(validator.register_class(child_class).is_ok());
        assert!(validator.validate_inheritance().is_ok());
        
        let hierarchy = validator.get_inheritance_hierarchy("Child").unwrap();
        assert_eq!(hierarchy, vec!["Child", "Parent", "GrandParent"]);
    }
}