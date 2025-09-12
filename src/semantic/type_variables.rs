use crate::ast::{SourceLocation, Type};
use crate::semantic::constraints::{ConstraintType, TypeVar};
use std::collections::{HashMap, HashSet};
// use std::sync::Arc;  // Currently unused

/// Type variable manager for constraint-based type inference
#[derive(Debug, Clone)]
pub struct TypeVariableManager {
    /// Map from type variable ID to its current binding
    bindings: HashMap<u32, ConstraintType>,
    /// Map from type variable ID to its metadata
    metadata: HashMap<u32, TypeVarMetadata>,
    /// Next available type variable ID
    next_id: u32,
    /// Stack of scope levels for scoped type variables
    scope_stack: Vec<ScopeLevel>,
    /// Current scope depth
    current_scope: usize,
}

/// Metadata about a type variable
#[derive(Debug, Clone)]
pub struct TypeVarMetadata {
    pub var: TypeVar,
    pub scope_level: usize,
    pub source_location: Option<SourceLocation>,
    pub created_context: String,
    pub bounds: Vec<ConstraintType>,
    pub is_generic: bool,
    pub variance: Variance,
}

/// Variance of a type variable (for generics)
#[derive(Debug, Clone, PartialEq)]
pub enum Variance {
    /// Covariant: T can be replaced by subtypes
    Covariant,
    /// Contravariant: T can be replaced by supertypes  
    Contravariant,
    /// Invariant: T must be exactly the same type
    Invariant,
    /// Bivariant: T can be any type (rarely used)
    Bivariant,
}

/// Scope level for type variables
#[derive(Debug, Clone)]
struct ScopeLevel {
    level: usize,
    variables: HashSet<u32>,
}

impl TypeVariableManager {
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            metadata: HashMap::new(),
            next_id: 0,
            scope_stack: vec![ScopeLevel {
                level: 0,
                variables: HashSet::new(),
            }],
            current_scope: 0,
        }
    }

    /// Create a fresh type variable
    pub fn fresh_var(&mut self, context: String, location: Option<SourceLocation>) -> TypeVar {
        let id = self.next_id;
        self.next_id += 1;

        let var = TypeVar::new(id);
        let metadata = TypeVarMetadata {
            var: var.clone(),
            scope_level: self.current_scope,
            source_location: location,
            created_context: context,
            bounds: Vec::new(),
            is_generic: false,
            variance: Variance::Invariant,
        };

        self.metadata.insert(id, metadata);

        // Add to current scope
        if let Some(scope) = self.scope_stack.last_mut() {
            scope.variables.insert(id);
        }

        var
    }

    /// Create a fresh generic type variable with variance
    pub fn fresh_generic_var(
        &mut self,
        context: String,
        location: Option<SourceLocation>,
        variance: Variance,
    ) -> TypeVar {
        let var = self.fresh_var(context, location);

        if let Some(metadata) = self.metadata.get_mut(&var.id) {
            metadata.is_generic = true;
            metadata.variance = variance;
        }

        var
    }

    /// Bind a type variable to a concrete type
    pub fn bind(&mut self, var: &TypeVar, typ: ConstraintType) -> Result<(), String> {
        // Check if variable is already bound
        if let Some(existing_binding) = self.bindings.get(&var.id) {
            return Err(format!(
                "Type variable {} is already bound to {:?}, cannot rebind to {:?}",
                var.id, existing_binding, typ
            ));
        }

        // Check for occur check (prevent infinite types)
        if self.occurs_check(&var, &typ) {
            return Err(format!(
                "Occur check failed: type variable {} occurs in {:?}",
                var.id, typ
            ));
        }

        // Check bounds constraints
        if let Some(metadata) = self.metadata.get(&var.id) {
            for bound in &metadata.bounds {
                if !self.satisfies_bound(&typ, bound)? {
                    return Err(format!(
                        "Type {:?} does not satisfy bound {:?} for variable {}",
                        typ, bound, var.id
                    ));
                }
            }
        }

        self.bindings.insert(var.id, typ);
        Ok(())
    }

    /// Get the current binding for a type variable
    pub fn get_binding(&self, var: &TypeVar) -> Option<&ConstraintType> {
        self.bindings.get(&var.id)
    }

    /// Resolve a constraint type by following bindings
    pub fn resolve(&self, typ: &ConstraintType) -> ConstraintType {
        match typ {
            ConstraintType::Variable(var) => {
                if let Some(binding) = self.bindings.get(&var.id) {
                    // Recursively resolve in case of chained bindings
                    self.resolve(binding)
                } else {
                    typ.clone()
                }
            }
            ConstraintType::Function {
                params,
                return_type,
            } => ConstraintType::Function {
                params: params.iter().map(|p| self.resolve(p)).collect(),
                return_type: Box::new(self.resolve(return_type)),
            },
            ConstraintType::Generic { name, params } => ConstraintType::Generic {
                name: name.clone(),
                params: params.iter().map(|p| self.resolve(p)).collect(),
            },
            ConstraintType::Union(types) => {
                let resolved_types: Vec<_> = types.iter().map(|t| self.resolve(t)).collect();
                // Simplify union if possible
                self.simplify_union(resolved_types)
            }
            _ => typ.clone(),
        }
    }

    /// Enter a new type variable scope
    pub fn enter_scope(&mut self) {
        self.current_scope += 1;
        self.scope_stack.push(ScopeLevel {
            level: self.current_scope,
            variables: HashSet::new(),
        });
    }

    /// Exit current type variable scope and clean up variables
    pub fn exit_scope(&mut self) -> Vec<TypeVar> {
        if self.scope_stack.len() <= 1 {
            return Vec::new(); // Cannot exit global scope
        }

        let scope = self.scope_stack.pop().unwrap();
        self.current_scope = self.scope_stack.last().map(|s| s.level).unwrap_or(0);

        // Collect variables that are going out of scope
        let mut exiting_vars = Vec::new();
        for var_id in &scope.variables {
            if let Some(metadata) = self.metadata.get(var_id) {
                exiting_vars.push(metadata.var.clone());
            }
        }

        // Clean up bindings and metadata for scoped variables
        for var_id in scope.variables {
            self.bindings.remove(&var_id);
            self.metadata.remove(&var_id);
        }

        exiting_vars
    }

    /// Add a bound constraint to a type variable
    pub fn add_bound(&mut self, var: &TypeVar, bound: ConstraintType) -> Result<(), String> {
        // Check if bound is consistent with existing bindings
        if let Some(binding) = self.bindings.get(&var.id).cloned() {
            if !self.satisfies_bound(&binding, &bound)? {
                return Err(format!(
                    "Current binding {:?} does not satisfy new bound {:?} for variable {}",
                    binding, bound, var.id
                ));
            }
        }

        if let Some(metadata) = self.metadata.get_mut(&var.id) {
            metadata.bounds.push(bound);
            Ok(())
        } else {
            Err(format!("Type variable {} not found", var.id))
        }
    }

    /// Get metadata for a type variable
    pub fn get_metadata(&self, var: &TypeVar) -> Option<&TypeVarMetadata> {
        self.metadata.get(&var.id)
    }

    /// Check if a type satisfies a bound constraint
    fn satisfies_bound(
        &self,
        typ: &ConstraintType,
        bound: &ConstraintType,
    ) -> Result<bool, String> {
        // Resolve both types first
        let resolved_type = self.resolve(typ);
        let resolved_bound = self.resolve(bound);

        match (&resolved_type, &resolved_bound) {
            // Concrete types
            (ConstraintType::Concrete(t1), ConstraintType::Concrete(t2)) => {
                Ok(self.is_subtype_concrete(t1, t2))
            }

            // Top type is satisfied by everything
            (_, ConstraintType::Top) => Ok(true),

            // Bottom type satisfies everything
            (ConstraintType::Bottom, _) => Ok(true),

            // Function types
            (
                ConstraintType::Function {
                    params: p1,
                    return_type: r1,
                },
                ConstraintType::Function {
                    params: p2,
                    return_type: r2,
                },
            ) => {
                if p1.len() != p2.len() {
                    return Ok(false);
                }

                // Contravariant in parameters, covariant in return type
                for (param1, param2) in p1.iter().zip(p2.iter()) {
                    if !self.satisfies_bound(param2, param1)? {
                        return Ok(false);
                    }
                }

                self.satisfies_bound(r1, r2)
            }

            // Generic types
            (
                ConstraintType::Generic {
                    name: n1,
                    params: p1,
                },
                ConstraintType::Generic {
                    name: n2,
                    params: p2,
                },
            ) => {
                if n1 != n2 || p1.len() != p2.len() {
                    return Ok(false);
                }

                // Check parameter bounds (variance-aware)
                for (param1, param2) in p1.iter().zip(p2.iter()) {
                    if !self.satisfies_bound(param1, param2)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }

            // Union types
            (ConstraintType::Union(types), bound) => {
                // All types in union must satisfy bound
                for typ in types {
                    if !self.satisfies_bound(typ, bound)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }

            (typ, ConstraintType::Union(bounds)) => {
                // Type must satisfy at least one bound in union
                for bound in bounds {
                    if self.satisfies_bound(typ, bound)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }

            // Variables not yet resolved
            (ConstraintType::Variable(_), _) | (_, ConstraintType::Variable(_)) => {
                // Cannot determine satisfaction with unresolved variables
                Ok(true) // Assume satisfiable for now
            }

            // Top type with other concrete types
            (ConstraintType::Top, _) => Ok(false), // Top is not a subtype of anything

            // All other cases default to false
            _ => Ok(false),
        }
    }

    /// Check subtype relationship for concrete types
    fn is_subtype_concrete(&self, subtype: &Type, supertype: &Type) -> bool {
        match (subtype, supertype) {
            // Same types
            (t1, t2) if t1 == t2 => true,

            // Number hierarchy: Integer <: Number
            (Type::Integer, Type::Number) => true,

            // List covariance: List<T1> <: List<T2> if T1 <: T2
            (Type::List(t1), Type::List(t2)) => self.is_subtype_concrete(t1, t2),

            // Any is supertype of everything except itself
            (_, Type::Any) => true,
            (Type::Any, _) => false,

            // Void is subtype of nothing except itself
            (Type::Void, _) => false,

            _ => false,
        }
    }

    /// Occur check to prevent infinite types
    fn occurs_check(&self, var: &TypeVar, typ: &ConstraintType) -> bool {
        match typ {
            ConstraintType::Variable(other_var) => {
                if var.id == other_var.id {
                    return true;
                }
                // Follow bindings
                if let Some(binding) = self.bindings.get(&other_var.id) {
                    self.occurs_check(var, binding)
                } else {
                    false
                }
            }
            ConstraintType::Function {
                params,
                return_type,
            } => {
                params.iter().any(|p| self.occurs_check(var, p))
                    || self.occurs_check(var, return_type)
            }
            ConstraintType::Generic { params, .. } => {
                params.iter().any(|p| self.occurs_check(var, p))
            }
            ConstraintType::Union(types) => types.iter().any(|t| self.occurs_check(var, t)),
            ConstraintType::Concrete(_) | ConstraintType::Bottom | ConstraintType::Top => false,
        }
    }

    /// Simplify union types by removing duplicates and redundant types
    fn simplify_union(&self, types: Vec<ConstraintType>) -> ConstraintType {
        if types.len() == 1 {
            return types.into_iter().next().unwrap();
        }

        let mut simplified = Vec::new();
        let mut seen = HashSet::new();

        for typ in types {
            // Skip duplicates
            if seen.contains(&typ) {
                continue;
            }

            // Skip if we already have a supertype
            let mut is_redundant = false;
            for existing in &simplified {
                if let (ConstraintType::Concrete(t1), ConstraintType::Concrete(t2)) =
                    (&typ, existing)
                {
                    if self.is_subtype_concrete(t1, t2) {
                        is_redundant = true;
                        break;
                    }
                }
            }

            if !is_redundant {
                // Remove any existing subtypes
                simplified.retain(|existing| {
                    if let (ConstraintType::Concrete(t1), ConstraintType::Concrete(t2)) =
                        (existing, &typ)
                    {
                        !self.is_subtype_concrete(t1, t2)
                    } else {
                        true
                    }
                });

                seen.insert(typ.clone());
                simplified.push(typ);
            }
        }

        if simplified.len() == 1 {
            simplified.into_iter().next().unwrap()
        } else {
            ConstraintType::Union(simplified)
        }
    }

    /// Get all unbound type variables
    pub fn unbound_variables(&self) -> Vec<TypeVar> {
        self.metadata
            .values()
            .filter(|meta| !self.bindings.contains_key(&meta.var.id))
            .map(|meta| meta.var.clone())
            .collect()
    }

    /// Get all bindings as a map
    pub fn get_all_bindings(&self) -> &HashMap<u32, ConstraintType> {
        &self.bindings
    }

    /// Clear all bindings (for testing)
    pub fn clear_bindings(&mut self) {
        self.bindings.clear();
    }

    /// Instantiate a generic type with fresh type variables
    pub fn instantiate_generic(
        &mut self,
        generic_type: &ConstraintType,
        context: String,
        location: Option<SourceLocation>,
    ) -> ConstraintType {
        // Create substitution map for generic variables
        let mut substitution = HashMap::new();
        self.collect_generic_vars(generic_type, &mut substitution, &context, &location);
        self.apply_substitution(generic_type, &substitution)
    }

    /// Collect generic variables and create fresh instances
    fn collect_generic_vars(
        &mut self,
        typ: &ConstraintType,
        substitution: &mut HashMap<u32, ConstraintType>,
        context: &str,
        location: &Option<SourceLocation>,
    ) {
        match typ {
            ConstraintType::Variable(var) => {
                if let Some(metadata) = self.metadata.get(&var.id) {
                    if metadata.is_generic && !substitution.contains_key(&var.id) {
                        let fresh_var =
                            self.fresh_var(format!("{}_instantiation", context), location.clone());
                        substitution.insert(var.id, ConstraintType::Variable(fresh_var));
                    }
                }
            }
            ConstraintType::Function {
                params,
                return_type,
            } => {
                for param in params {
                    self.collect_generic_vars(param, substitution, context, location);
                }
                self.collect_generic_vars(return_type, substitution, context, location);
            }
            ConstraintType::Generic { params, .. } => {
                for param in params {
                    self.collect_generic_vars(param, substitution, context, location);
                }
            }
            ConstraintType::Union(types) => {
                for typ in types {
                    self.collect_generic_vars(typ, substitution, context, location);
                }
            }
            _ => {}
        }
    }

    /// Apply substitution to a type
    fn apply_substitution(
        &self,
        typ: &ConstraintType,
        substitution: &HashMap<u32, ConstraintType>,
    ) -> ConstraintType {
        match typ {
            ConstraintType::Variable(var) => substitution
                .get(&var.id)
                .cloned()
                .unwrap_or_else(|| typ.clone()),
            ConstraintType::Function {
                params,
                return_type,
            } => ConstraintType::Function {
                params: params
                    .iter()
                    .map(|p| self.apply_substitution(p, substitution))
                    .collect(),
                return_type: Box::new(self.apply_substitution(return_type, substitution)),
            },
            ConstraintType::Generic { name, params } => ConstraintType::Generic {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|p| self.apply_substitution(p, substitution))
                    .collect(),
            },
            ConstraintType::Union(types) => ConstraintType::Union(
                types
                    .iter()
                    .map(|t| self.apply_substitution(t, substitution))
                    .collect(),
            ),
            _ => typ.clone(),
        }
    }
}

impl Default for TypeVariableManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_variable_creation() {
        let mut manager = TypeVariableManager::new();
        let var1 = manager.fresh_var("test1".to_string(), None);
        let var2 = manager.fresh_var("test2".to_string(), None);

        assert_eq!(var1.id, 0);
        assert_eq!(var2.id, 1);
        assert_eq!(manager.metadata.len(), 2);
    }

    #[test]
    fn test_variable_binding() {
        let mut manager = TypeVariableManager::new();
        let var = manager.fresh_var("test".to_string(), None);

        let result = manager.bind(&var, ConstraintType::Concrete(Type::Integer));
        assert!(result.is_ok());

        let binding = manager.get_binding(&var);
        assert_eq!(binding, Some(&ConstraintType::Concrete(Type::Integer)));
    }

    #[test]
    fn test_occur_check() {
        let mut manager = TypeVariableManager::new();
        let var = manager.fresh_var("test".to_string(), None);

        // Try to bind var to function type containing var (should fail)
        let result = manager.bind(
            &var,
            ConstraintType::Function {
                params: vec![ConstraintType::Variable(var.clone())],
                return_type: Box::new(ConstraintType::Concrete(Type::Integer)),
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Occur check failed"));
    }

    #[test]
    fn test_type_resolution() {
        let mut manager = TypeVariableManager::new();
        let var1 = manager.fresh_var("test1".to_string(), None);
        let var2 = manager.fresh_var("test2".to_string(), None);

        // Bind var1 -> var2 -> Integer
        manager
            .bind(&var1, ConstraintType::Variable(var2.clone()))
            .unwrap();
        manager
            .bind(&var2, ConstraintType::Concrete(Type::Integer))
            .unwrap();

        let resolved = manager.resolve(&ConstraintType::Variable(var1));
        assert_eq!(resolved, ConstraintType::Concrete(Type::Integer));
    }

    #[test]
    fn test_scope_management() {
        let mut manager = TypeVariableManager::new();

        // Create variable in global scope
        let global_var = manager.fresh_var("global".to_string(), None);

        // Enter new scope
        manager.enter_scope();
        let scoped_var = manager.fresh_var("scoped".to_string(), None);

        assert_eq!(manager.metadata.len(), 2);

        // Exit scope
        let exiting_vars = manager.exit_scope();

        assert_eq!(exiting_vars.len(), 1);
        assert_eq!(exiting_vars[0].id, scoped_var.id);
        assert_eq!(manager.metadata.len(), 1);
        assert!(manager.metadata.contains_key(&global_var.id));
        assert!(!manager.metadata.contains_key(&scoped_var.id));
    }

    #[test]
    fn test_generic_instantiation() {
        let mut manager = TypeVariableManager::new();
        let generic_var = manager.fresh_generic_var("T".to_string(), None, Variance::Covariant);
        let original_var_id = generic_var.id;

        let generic_type = ConstraintType::Generic {
            name: "Array".to_string(),
            params: vec![ConstraintType::Variable(generic_var)],
        };

        let instantiated =
            manager.instantiate_generic(&generic_type, "test_instantiation".to_string(), None);

        if let ConstraintType::Generic { params, .. } = instantiated {
            if let ConstraintType::Variable(fresh_var) = &params[0] {
                assert_ne!(fresh_var.id, original_var_id);
            } else {
                panic!("Expected fresh type variable");
            }
        } else {
            panic!("Expected generic type");
        }
    }
}
