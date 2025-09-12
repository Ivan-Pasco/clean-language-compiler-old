//! Constraint solver for type inference in Clean Language
//!
//! Implements a constraint-based type inference system using unification
//! and substitution to solve type constraints generated during type checking.

use super::tast::{ConcreteType, TypeConstraint, TypeParameter};
use crate::error::CompilerError;
use crate::ast::SourceLocation;
use std::collections::{HashMap, HashSet, VecDeque};

/// Type variable identifier for constraint solving
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVarId(pub usize);

/// Type substitution mapping type variables to concrete types
pub type Substitution = HashMap<TypeVarId, ConcreteType>;

/// Constraint solver for type inference
#[derive(Debug)]
pub struct ConstraintSolver {
    /// Current substitution being built
    substitution: Substitution,
    
    /// Type variable counter for generating fresh variables
    type_var_counter: usize,
    
    /// Pending constraints to solve
    constraints: VecDeque<TypeConstraint>,
    
    /// Solved constraints (for debugging)
    solved_constraints: Vec<TypeConstraint>,
    
    /// Type variable bounds and relationships
    type_var_bounds: HashMap<TypeVarId, Vec<ConcreteType>>,
    
    /// Errors encountered during solving
    errors: Vec<CompilerError>,
}

/// Result of constraint solving
#[derive(Debug)]
pub struct SolverResult {
    pub substitution: Substitution,
    pub remaining_constraints: Vec<TypeConstraint>,
    pub errors: Vec<CompilerError>,
}

impl ConstraintSolver {
    /// Create a new constraint solver
    pub fn new() -> Self {
        Self {
            substitution: HashMap::new(),
            type_var_counter: 0,
            constraints: VecDeque::new(),
            solved_constraints: Vec::new(),
            type_var_bounds: HashMap::new(),
            errors: Vec::new(),
        }
    }
    
    /// Generate a fresh type variable
    pub fn fresh_type_var(&mut self) -> TypeVarId {
        let id = TypeVarId(self.type_var_counter);
        self.type_var_counter += 1;
        id
    }
    
    /// Add constraints to solve
    pub fn add_constraints(&mut self, constraints: Vec<TypeConstraint>) {
        for constraint in constraints {
            self.constraints.push_back(constraint);
        }
    }
    
    /// Solve all constraints and return the result
    pub fn solve(mut self) -> SolverResult {
        // Process constraints until none remain or we can't make progress
        let mut iterations = 0;
        let max_iterations = 1000; // Prevent infinite loops
        
        while !self.constraints.is_empty() && iterations < max_iterations {
            let initial_count = self.constraints.len();
            
            // Try to solve one constraint
            if let Some(constraint) = self.constraints.pop_front() {
                if let Err(error) = self.solve_constraint(constraint) {
                    self.errors.push(error);
                }
            }
            
            // Check if we made progress
            if self.constraints.len() == initial_count {
                // No progress made, try different solving strategies
                if !self.try_alternative_strategies() {
                    break; // Can't make any more progress
                }
            }
            
            iterations += 1;
        }
        
        if iterations >= max_iterations {
            self.errors.push(CompilerError::type_error(
                "Type inference timeout: too many constraint solving iterations",
                None,
                Some(SourceLocation {
                    file: "<constraint_solver>".to_string(),
                    line: 0,
                    column: 0,
                }),
            ));
        }
        
        SolverResult {
            substitution: self.substitution,
            remaining_constraints: self.constraints.into_iter().collect(),
            errors: self.errors,
        }
    }
    
    /// Solve a single constraint
    fn solve_constraint(&mut self, constraint: TypeConstraint) -> Result<(), CompilerError> {
        match constraint {
            TypeConstraint::Equality { left, right, location } => {
                self.unify(&left, &right, &location)
            }
            
            TypeConstraint::Subtype { subtype, supertype, location } => {
                self.check_subtype(&subtype, &supertype, &location)
            }
            
            TypeConstraint::Implements { type_, interface, location } => {
                self.check_implements(&type_, &interface, &location)
            }
            
            TypeConstraint::HasMember { type_, member_name, member_type, location } => {
                self.check_has_member(&type_, &member_name, &member_type, &location)
            }
        }
    }
    
    /// Unify two types (make them equal through substitution)
    fn unify(&mut self, left: &ConcreteType, right: &ConcreteType, location: &SourceLocation) -> Result<(), CompilerError> {
        // Apply current substitution to both types
        let left = self.apply_substitution(left);
        let right = self.apply_substitution(right);
        
        match (&left, &right) {
            // Identical types unify trivially
            (a, b) if a == b => Ok(()),
            
            // Type variable unification
            (ConcreteType::Generic { name: var_name, .. }, t) | 
            (t, ConcreteType::Generic { name: var_name, .. }) => {
                // Extract type variable ID (simplified - in real implementation would be more complex)
                if let Ok(var_id) = var_name.parse::<usize>() {
                    let type_var = TypeVarId(var_id);
                    self.bind_type_var(type_var, t.clone(), location)
                } else {
                    Err(CompilerError::type_error(
                        &format!("Invalid type variable: {}", var_name),
                        None,
                        Some(location.clone()),
                    ))
                }
            }
            
            // Array unification
            (ConcreteType::Array(a), ConcreteType::Array(b)) => {
                self.unify(a, b, location)
            }
            
            // Function unification
            (ConcreteType::Function { parameters: p1, return_type: r1, is_async: a1 },
             ConcreteType::Function { parameters: p2, return_type: r2, is_async: a2 }) => {
                if a1 != a2 {
                    return Err(CompilerError::type_error(
                        "Cannot unify async and non-async functions",
                        None,
                        Some(location.clone()),
                    ));
                }
                
                if p1.len() != p2.len() {
                    return Err(CompilerError::type_error(
                        &format!("Function parameter count mismatch: {} vs {}", p1.len(), p2.len()),
                        None,
                        Some(location.clone()),
                    ));
                }
                
                // Unify parameters
                for (param1, param2) in p1.iter().zip(p2.iter()) {
                    self.unify(param1, param2, location)?;
                }
                
                // Unify return types
                self.unify(r1, r2, location)
            }
            
            // Class unification
            (ConcreteType::Class { symbol_id: id1, type_args: args1 },
             ConcreteType::Class { symbol_id: id2, type_args: args2 }) => {
                if id1 != id2 {
                    return Err(CompilerError::type_error(
                        &format!("Cannot unify different classes: {} vs {}", id1.0, id2.0),
                        None,
                        Some(location.clone()),
                    ));
                }
                
                if args1.len() != args2.len() {
                    return Err(CompilerError::type_error(
                        "Class type argument count mismatch",
                        None,
                        Some(location.clone()),
                    ));
                }
                
                // Unify type arguments
                for (arg1, arg2) in args1.iter().zip(args2.iter()) {
                    self.unify(arg1, arg2, location)?;
                }
                
                Ok(())
            }
            
            // Tuple unification
            (ConcreteType::Tuple(t1), ConcreteType::Tuple(t2)) => {
                if t1.len() != t2.len() {
                    return Err(CompilerError::type_error(
                        &format!("Tuple size mismatch: {} vs {}", t1.len(), t2.len()),
                        None,
                        Some(location.clone()),
                    ));
                }
                
                for (type1, type2) in t1.iter().zip(t2.iter()) {
                    self.unify(type1, type2, location)?;
                }
                
                Ok(())
            }
            
            // Union type handling (simplified)
            (ConcreteType::Union(types), t) | (t, ConcreteType::Union(types)) => {
                // Check if t is assignable to any type in the union
                if types.iter().any(|union_type| t.is_assignable_to(union_type)) {
                    Ok(())
                } else {
                    Err(CompilerError::type_error(
                        &format!("Type {} is not compatible with union type", t),
                        None,
                        Some(location.clone()),
                    ))
                }
            }
            
            // Integer can unify with Number
            (ConcreteType::Integer, ConcreteType::Number) | 
            (ConcreteType::Number, ConcreteType::Integer) => Ok(()),
            
            // Null and Undefined can unify with each other (for function return types)
            (ConcreteType::Null, ConcreteType::Undefined) | 
            (ConcreteType::Undefined, ConcreteType::Null) => Ok(()),
            
            // Unknown type unifies with anything (error recovery)
            (ConcreteType::Unknown, _) | (_, ConcreteType::Unknown) => Ok(()),
            
            // Types cannot be unified
            _ => Err(CompilerError::type_error(
                &format!("Cannot unify types: {} and {}", left, right),
                None,
                Some(location.clone()),
            )),
        }
    }
    
    /// Check subtype relationship
    fn check_subtype(&mut self, subtype: &ConcreteType, supertype: &ConcreteType, location: &SourceLocation) -> Result<(), CompilerError> {
        let subtype = self.apply_substitution(subtype);
        let supertype = self.apply_substitution(supertype);
        
        if subtype.is_assignable_to(&supertype) {
            Ok(())
        } else {
            Err(CompilerError::type_error(
                &format!("Type {} is not a subtype of {}", subtype, supertype),
                None,
                Some(location.clone()),
            ))
        }
    }
    
    /// Check interface implementation
    fn check_implements(&mut self, type_: &ConcreteType, interface: &ConcreteType, location: &SourceLocation) -> Result<(), CompilerError> {
        // Simplified implementation - would need full interface checking
        match (type_, interface) {
            (ConcreteType::Class { .. }, ConcreteType::Interface { .. }) => {
                // Would check if class implements all interface methods
                Ok(())
            }
            _ => Err(CompilerError::type_error(
                &format!("Type {} does not implement interface {}", type_, interface),
                None,
                Some(location.clone()),
            ))
        }
    }
    
    /// Check if type has specific member
    fn check_has_member(&mut self, type_: &ConcreteType, member_name: &str, member_type: &ConcreteType, location: &SourceLocation) -> Result<(), CompilerError> {
        let type_ = self.apply_substitution(type_);
        
        match &type_ {
            ConcreteType::Array(_) => {
                // Built-in array methods
                match member_name {
                    "length" => self.unify(&ConcreteType::Integer, member_type, location),
                    "push" | "pop" | "shift" | "unshift" => {
                        // Would need more sophisticated method type checking
                        Ok(())
                    }
                    _ => Err(CompilerError::type_error(
                        &format!("Array type does not have member: {}", member_name),
                        None,
                        Some(location.clone()),
                    ))
                }
            }
            
            ConcreteType::String => {
                // Built-in string methods
                match member_name {
                    "length" => self.unify(&ConcreteType::Integer, member_type, location),
                    "substring" | "charAt" | "indexOf" => {
                        // Would need more sophisticated method type checking
                        Ok(())
                    }
                    _ => Err(CompilerError::type_error(
                        &format!("String type does not have member: {}", member_name),
                        None,
                        Some(location.clone()),
                    ))
                }
            }
            
            ConcreteType::Class { symbol_id, .. } => {
                // Would look up class definition and check members
                // For now, assume all member accesses are valid
                Ok(())
            }
            
            _ => Err(CompilerError::type_error(
                &format!("Type {} does not have member: {}", type_, member_name),
                None,
                Some(location.clone()),
            ))
        }
    }
    
    /// Bind a type variable to a concrete type
    fn bind_type_var(&mut self, var: TypeVarId, type_: ConcreteType, location: &SourceLocation) -> Result<(), CompilerError> {
        // Check for occurs check (prevent infinite types)
        if self.occurs_check(var, &type_) {
            return Err(CompilerError::type_error(
                "Infinite type detected (occurs check failed)",
                None,
                Some(location.clone()),
            ));
        }
        
        // Apply current substitution to the type being bound
        let resolved_type = self.apply_substitution(&type_);
        
        // Add to substitution
        self.substitution.insert(var, resolved_type);
        
        Ok(())
    }
    
    /// Apply current substitution to a type
    fn apply_substitution(&self, type_: &ConcreteType) -> ConcreteType {
        match type_ {
            ConcreteType::Generic { name, .. } => {
                // Try to resolve type variable (simplified)
                if let Ok(var_id) = name.parse::<usize>() {
                    let type_var = TypeVarId(var_id);
                    if let Some(substituted) = self.substitution.get(&type_var) {
                        // Recursively apply substitution
                        self.apply_substitution(substituted)
                    } else {
                        type_.clone()
                    }
                } else {
                    type_.clone()
                }
            }
            
            ConcreteType::Array(element_type) => {
                ConcreteType::Array(Box::new(self.apply_substitution(element_type)))
            }
            
            ConcreteType::Function { parameters, return_type, is_async } => {
                ConcreteType::Function {
                    parameters: parameters.iter().map(|p| self.apply_substitution(p)).collect(),
                    return_type: Box::new(self.apply_substitution(return_type)),
                    is_async: *is_async,
                }
            }
            
            ConcreteType::Class { symbol_id, type_args } => {
                ConcreteType::Class {
                    symbol_id: *symbol_id,
                    type_args: type_args.iter().map(|t| self.apply_substitution(t)).collect(),
                }
            }
            
            ConcreteType::Interface { symbol_id, type_args } => {
                ConcreteType::Interface {
                    symbol_id: *symbol_id,
                    type_args: type_args.iter().map(|t| self.apply_substitution(t)).collect(),
                }
            }
            
            ConcreteType::Tuple(types) => {
                ConcreteType::Tuple(types.iter().map(|t| self.apply_substitution(t)).collect())
            }
            
            ConcreteType::Union(types) => {
                ConcreteType::Union(types.iter().map(|t| self.apply_substitution(t)).collect())
            }
            
            ConcreteType::Intersection(types) => {
                ConcreteType::Intersection(types.iter().map(|t| self.apply_substitution(t)).collect())
            }
            
            // Primitive types don't need substitution
            _ => type_.clone(),
        }
    }
    
    /// Check if a type variable occurs in a type (prevents infinite types)
    fn occurs_check(&self, var: TypeVarId, type_: &ConcreteType) -> bool {
        match type_ {
            ConcreteType::Generic { name, .. } => {
                if let Ok(other_var_id) = name.parse::<usize>() {
                    TypeVarId(other_var_id) == var
                } else {
                    false
                }
            }
            
            ConcreteType::Array(element_type) => self.occurs_check(var, element_type),
            
            ConcreteType::Function { parameters, return_type, .. } => {
                parameters.iter().any(|p| self.occurs_check(var, p)) ||
                self.occurs_check(var, return_type)
            }
            
            ConcreteType::Class { type_args, .. } |
            ConcreteType::Interface { type_args, .. } => {
                type_args.iter().any(|t| self.occurs_check(var, t))
            }
            
            ConcreteType::Tuple(types) |
            ConcreteType::Union(types) |
            ConcreteType::Intersection(types) => {
                types.iter().any(|t| self.occurs_check(var, t))
            }
            
            _ => false,
        }
    }
    
    /// Try alternative solving strategies when stuck
    fn try_alternative_strategies(&mut self) -> bool {
        // Strategy 1: Try to resolve any type variables to their bounds
        if self.resolve_bounded_type_vars() {
            return true;
        }
        
        // Strategy 2: Try to break down complex constraints
        if self.decompose_complex_constraints() {
            return true;
        }
        
        // Strategy 3: Apply default types to unconstrained variables
        if self.apply_default_types() {
            return true;
        }
        
        false
    }
    
    /// Resolve type variables that have clear bounds
    fn resolve_bounded_type_vars(&mut self) -> bool {
        let mut made_progress = false;
        
        for (var_id, bounds) in &self.type_var_bounds.clone() {
            if bounds.len() == 1 && !self.substitution.contains_key(var_id) {
                // Only one bound - use it as the type
                self.substitution.insert(*var_id, bounds[0].clone());
                made_progress = true;
            }
        }
        
        made_progress
    }
    
    /// Break down complex constraints into simpler ones
    fn decompose_complex_constraints(&mut self) -> bool {
        // Would implement constraint decomposition here
        false
    }
    
    /// Apply default types to unconstrained type variables
    fn apply_default_types(&mut self) -> bool {
        let mut made_progress = false;
        
        // Find unconstrained type variables in remaining constraints
        let mut unconstrained_vars = HashSet::new();
        
        for constraint in &self.constraints {
            match constraint {
                TypeConstraint::Equality { left, right, .. } => {
                    self.collect_type_vars(left, &mut unconstrained_vars);
                    self.collect_type_vars(right, &mut unconstrained_vars);
                }
                TypeConstraint::Subtype { subtype, supertype, .. } => {
                    self.collect_type_vars(subtype, &mut unconstrained_vars);
                    self.collect_type_vars(supertype, &mut unconstrained_vars);
                }
                TypeConstraint::Implements { type_, interface, .. } => {
                    self.collect_type_vars(type_, &mut unconstrained_vars);
                    self.collect_type_vars(interface, &mut unconstrained_vars);
                }
                TypeConstraint::HasMember { type_, member_type, .. } => {
                    self.collect_type_vars(type_, &mut unconstrained_vars);
                    self.collect_type_vars(member_type, &mut unconstrained_vars);
                }
            }
        }
        
        // Remove already substituted variables
        for var_id in self.substitution.keys() {
            unconstrained_vars.remove(var_id);
        }
        
        // Apply default types (Unknown for now)
        for var_id in unconstrained_vars {
            self.substitution.insert(var_id, ConcreteType::Unknown);
            made_progress = true;
        }
        
        made_progress
    }
    
    /// Collect type variables from a type
    fn collect_type_vars(&self, type_: &ConcreteType, vars: &mut HashSet<TypeVarId>) {
        match type_ {
            ConcreteType::Generic { name, .. } => {
                if let Ok(var_id) = name.parse::<usize>() {
                    vars.insert(TypeVarId(var_id));
                }
            }
            ConcreteType::Array(element_type) => {
                self.collect_type_vars(element_type, vars);
            }
            ConcreteType::Function { parameters, return_type, .. } => {
                for param in parameters {
                    self.collect_type_vars(param, vars);
                }
                self.collect_type_vars(return_type, vars);
            }
            ConcreteType::Class { type_args, .. } |
            ConcreteType::Interface { type_args, .. } => {
                for arg in type_args {
                    self.collect_type_vars(arg, vars);
                }
            }
            ConcreteType::Tuple(types) |
            ConcreteType::Union(types) |
            ConcreteType::Intersection(types) => {
                for t in types {
                    self.collect_type_vars(t, vars);
                }
            }
            _ => {}
        }
    }
}

impl Default for ConstraintSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::SymbolId;

    fn test_location() -> SourceLocation {
        SourceLocation {
            file: "test.cln".to_string(),
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_basic_unification() {
        let mut solver = ConstraintSolver::new();
        
        // Create constraint: integer = integer
        let constraint = TypeConstraint::Equality {
            left: ConcreteType::Integer,
            right: ConcreteType::Integer,
            location: test_location(),
        };
        
        solver.add_constraints(vec![constraint]);
        let result = solver.solve();
        
        assert!(result.errors.is_empty());
        assert!(result.remaining_constraints.is_empty());
    }

    #[test]
    fn test_type_variable_unification() {
        let mut solver = ConstraintSolver::new();
        
        // Create constraint: T = integer (where T is type variable 0)
        let constraint = TypeConstraint::Equality {
            left: ConcreteType::Generic {
                name: "0".to_string(),
                bounds: vec![],
            },
            right: ConcreteType::Integer,
            location: test_location(),
        };
        
        solver.add_constraints(vec![constraint]);
        let result = solver.solve();
        
        assert!(result.errors.is_empty());
        assert_eq!(result.substitution.get(&TypeVarId(0)), Some(&ConcreteType::Integer));
    }

    #[test]
    fn test_array_unification() {
        let mut solver = ConstraintSolver::new();
        
        // Create constraint: Array<integer> = Array<integer>
        let constraint = TypeConstraint::Equality {
            left: ConcreteType::Array(Box::new(ConcreteType::Integer)),
            right: ConcreteType::Array(Box::new(ConcreteType::Integer)),
            location: test_location(),
        };
        
        solver.add_constraints(vec![constraint]);
        let result = solver.solve();
        
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_unification_failure() {
        let mut solver = ConstraintSolver::new();
        
        // Create constraint: integer = string (should fail)
        let constraint = TypeConstraint::Equality {
            left: ConcreteType::Integer,
            right: ConcreteType::String,
            location: test_location(),
        };
        
        solver.add_constraints(vec![constraint]);
        let result = solver.solve();
        
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_occurs_check() {
        let mut solver = ConstraintSolver::new();
        
        // Create constraint: T = Array<T> (should fail occurs check)
        let constraint = TypeConstraint::Equality {
            left: ConcreteType::Generic {
                name: "0".to_string(),
                bounds: vec![],
            },
            right: ConcreteType::Array(Box::new(ConcreteType::Generic {
                name: "0".to_string(),
                bounds: vec![],
            })),
            location: test_location(),
        };
        
        solver.add_constraints(vec![constraint]);
        let result = solver.solve();
        
        assert!(!result.errors.is_empty());
    }
}