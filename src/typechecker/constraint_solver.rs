//! Constraint solver for type inference in Clean Language
//!
//! Implements a constraint-based type inference system using unification
//! and substitution to solve type constraints generated during type checking.

use super::tast::{ConcreteType, TypeConstraint};
use crate::ast::SourceLocation;
use crate::error::CompilerError;
use std::collections::{HashMap, HashSet, VecDeque};

/// Type variable identifier for constraint solving
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVarId(pub usize);

/// Type substitution mapping type variables to concrete types
pub type Substitution = HashMap<TypeVarId, ConcreteType>;

/// Constraint solver for type inference
#[derive(Debug)]
#[allow(dead_code)] // Constraint-based solver not yet integrated; inference runs via TypeInference directly
pub struct ConstraintSolver<'a> {
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

    /// Symbol table for checking inheritance relationships
    symbol_table: Option<&'a crate::resolver::GlobalSymbolTable>,
}

/// Result of constraint solving
#[derive(Debug)]
pub struct SolverResult {
    pub substitution: Substitution,
    pub remaining_constraints: Vec<TypeConstraint>,
    pub errors: Vec<CompilerError>,
}

impl<'a> ConstraintSolver<'a> {
    /// Create a new constraint solver
    pub fn new() -> Self {
        Self {
            substitution: HashMap::new(),
            type_var_counter: 0,
            constraints: VecDeque::new(),
            solved_constraints: Vec::new(),
            type_var_bounds: HashMap::new(),
            errors: Vec::new(),
            symbol_table: None,
        }
    }

    /// Create a new constraint solver with symbol table access
    pub fn with_symbol_table(symbol_table: &'a crate::resolver::GlobalSymbolTable) -> Self {
        Self {
            substitution: HashMap::new(),
            type_var_counter: 0,
            constraints: VecDeque::new(),
            solved_constraints: Vec::new(),
            type_var_bounds: HashMap::new(),
            errors: Vec::new(),
            symbol_table: Some(symbol_table),
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
        // Process constraints until none remain or we can't make progress.
        //
        // The iteration cap is a safety net against pathological infinite loops
        // in `solve_constraint`. Genuine non-progress is already detected by the
        // `try_alternative_strategies` `break` below — so we don't need the cap
        // to catch stalls, only true loops.
        //
        // Each plugin-generated `data:` block expands into many constraints
        // (~250 with frame.data — the class declaration, two generic result
        // types `PagedResult<T>` and `CursorResult<T>`, the `find`, `first`,
        // `save`, `delete`, `paginate`, `cursor` methods, and the implicit
        // `this` references inside each). A realistic project with 50 data
        // models therefore submits ~12500 constraints; one with 100 models
        // submits ~25000. The previous 10000-iteration cap rejected the
        // former. Setting the cap to 100000 covers projects up to ~400 data
        // models — comfortably ahead of any realistic application — while
        // still trapping an actual runaway. See SEM001 (fingerprint
        // 7cc56e80f714).
        let mut iterations = 0;
        let max_iterations = 100_000;

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
                    byte_start: None,
                    byte_end: None,
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
            TypeConstraint::Equality {
                left,
                right,
                location,
            } => self.unify(&left, &right, &location),

            TypeConstraint::Subtype {
                subtype,
                supertype,
                location,
            } => self.check_subtype(&subtype, &supertype, &location),

            TypeConstraint::Implements {
                type_,
                interface,
                location,
            } => self.check_implements(&type_, &interface, &location),

            TypeConstraint::HasMember {
                type_,
                member_name,
                member_type,
                location,
            } => self.check_has_member(&type_, &member_name, &member_type, &location),
        }
    }

    /// Unify two types (make them equal through substitution)
    fn unify(
        &mut self,
        left: &ConcreteType,
        right: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<(), CompilerError> {
        // Apply current substitution to both types
        let left = self.apply_substitution(left);
        let right = self.apply_substitution(right);

        match (&left, &right) {
            // Identical types unify trivially
            (a, b) if a == b => Ok(()),

            // Type variable unification
            (ConcreteType::Generic { name: var_name, .. }, t)
            | (t, ConcreteType::Generic { name: var_name, .. }) => {
                // Special handling for 'any' - it unifies with any type
                if var_name == "any" {
                    return Ok(());
                }

                // Extract type variable ID (simplified - in real implementation would be more complex)
                if let Ok(var_id) = var_name.parse::<usize>() {
                    let type_var = TypeVarId(var_id);
                    self.bind_type_var(type_var, t.clone(), location)
                } else {
                    Err(CompilerError::type_error(
                        format!("Invalid type variable: {}", var_name),
                        None,
                        Some(location.clone()),
                    ))
                }
            }

            // Array unification
            (ConcreteType::Array(a), ConcreteType::Array(b)) => self.unify(a, b, location),

            // Matrix unification
            (ConcreteType::Matrix(a), ConcreteType::Matrix(b)) => self.unify(a, b, location),

            // Array<Array<T>> can unify with Matrix<T> (2D array literal assignment to matrix)
            (ConcreteType::Array(inner), ConcreteType::Matrix(element_type))
            | (ConcreteType::Matrix(element_type), ConcreteType::Array(inner)) => {
                // Check if inner is Array<T> where T unifies with element_type
                if let ConcreteType::Array(inner_element_type) = &**inner {
                    self.unify(inner_element_type, element_type, location)
                } else {
                    Err(CompilerError::type_error(
                        format!(
                            "Cannot unify types: {} and Matrix<{}>",
                            ConcreteType::Array(inner.clone()),
                            element_type
                        ),
                        None,
                        Some(location.clone()),
                    ))
                }
            }

            // Function unification
            (
                ConcreteType::Function {
                    parameters: p1,
                    return_type: r1,
                    is_background: a1,
                },
                ConcreteType::Function {
                    parameters: p2,
                    return_type: r2,
                    is_background: a2,
                },
            ) => {
                if a1 != a2 {
                    return Err(CompilerError::type_error(
                        "Cannot unify async and non-async functions",
                        None,
                        Some(location.clone()),
                    ));
                }

                if p1.len() != p2.len() {
                    return Err(CompilerError::type_error(
                        format!(
                            "Function parameter count mismatch: {} vs {}",
                            p1.len(),
                            p2.len()
                        ),
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
            (
                ConcreteType::Class {
                    symbol_id: id1,
                    type_args: args1,
                },
                ConcreteType::Class {
                    symbol_id: id2,
                    type_args: args2,
                },
            ) => {
                // Check if they're the same class
                if id1 != id2 {
                    // Check for inheritance relationship
                    // Allow unifying child class with parent class
                    if !self.is_subclass_of(*id1, *id2) && !self.is_subclass_of(*id2, *id1) {
                        return Err(CompilerError::type_error(
                            format!("Cannot unify incompatible classes: {} vs {}", id1.0, id2.0),
                            None,
                            Some(location.clone()),
                        ));
                    }
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
                        format!("Tuple size mismatch: {} vs {}", t1.len(), t2.len()),
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
                if types
                    .iter()
                    .any(|union_type| t.is_assignable_to(union_type))
                {
                    Ok(())
                } else {
                    Err(CompilerError::type_error(
                        format!("Type {} is not compatible with union type", t),
                        None,
                        Some(location.clone()),
                    ))
                }
            }

            // Integer can unify with Number
            (ConcreteType::Integer, ConcreteType::Number)
            | (ConcreteType::Number, ConcreteType::Integer) => Ok(()),

            // Sized numeric types unify with their unsized counterparts
            // number:64 is just number with explicit precision — they are compatible
            (ConcreteType::NumberSized { .. }, ConcreteType::Number)
            | (ConcreteType::Number, ConcreteType::NumberSized { .. }) => Ok(()),

            // Two NumberSized: reject narrowing assignments (type-system.md §sized-numbers)
            (ConcreteType::NumberSized { bits: b1 }, ConcreteType::NumberSized { bits: b2 }) => {
                if b1 > b2 {
                    Err(CompilerError::type_error(
                        format!(
                            "Precision loss: cannot assign number:{} to number:{} without explicit cast",
                            b1, b2
                        ),
                        None,
                        Some(location.clone()),
                    ))
                } else {
                    Ok(())
                }
            }

            // IntegerSized unifies with Integer
            (ConcreteType::IntegerSized { .. }, ConcreteType::Integer)
            | (ConcreteType::Integer, ConcreteType::IntegerSized { .. }) => Ok(()),

            // Two IntegerSized: enforce signedness match (type-system.md §sized-integers)
            (
                ConcreteType::IntegerSized {
                    bits: b1,
                    unsigned: u1,
                },
                ConcreteType::IntegerSized {
                    bits: b2,
                    unsigned: u2,
                },
            ) => {
                if u1 != u2 {
                    let left_desc = if *u1 { "unsigned" } else { "signed" };
                    let right_desc = if *u2 { "unsigned" } else { "signed" };
                    Err(CompilerError::type_error(
                        format!(
                            "Signedness mismatch: cannot assign {} integer:{} to {} integer:{} variable",
                            left_desc, b1, right_desc, b2
                        ),
                        None,
                        Some(location.clone()),
                    ))
                } else {
                    Ok(())
                }
            }

            // Integer/IntegerSized can unify with Number/NumberSized (widening)
            (ConcreteType::Integer, ConcreteType::NumberSized { .. })
            | (ConcreteType::NumberSized { .. }, ConcreteType::Integer) => Ok(()),
            (ConcreteType::IntegerSized { .. }, ConcreteType::Number)
            | (ConcreteType::Number, ConcreteType::IntegerSized { .. }) => Ok(()),
            (ConcreteType::IntegerSized { .. }, ConcreteType::NumberSized { .. })
            | (ConcreteType::NumberSized { .. }, ConcreteType::IntegerSized { .. }) => Ok(()),

            // Null and Undefined can unify with each other (for function return types)
            (ConcreteType::Null, ConcreteType::Undefined)
            | (ConcreteType::Undefined, ConcreteType::Null) => Ok(()),

            // Null can unify with ANY type — per type-system.md §5 row 121:
            // "Every type accepts null. There is no non-nullable type annotation."
            // This includes Integer, Number, Boolean, String, Array, Class, etc.
            (ConcreteType::Null, _) | (_, ConcreteType::Null) => Ok(()),

            // Unknown type unifies with anything (error recovery)
            (ConcreteType::Unknown, _) | (_, ConcreteType::Unknown) => Ok(()),

            // Any type unifies with anything (dynamic typing)
            // At runtime, values are boxed with type tags for proper handling
            (ConcreteType::Any, _) | (_, ConcreteType::Any) => Ok(()),

            // Function references unify with Integer (handler/callback parameters)
            // In WASM, function references become integer indices in the function table
            (ConcreteType::Function { .. }, ConcreteType::Integer)
            | (ConcreteType::Integer, ConcreteType::Function { .. }) => Ok(()),

            // Optional unification: Optional(A) unifies with Optional(B) if A unifies with B
            (ConcreteType::Optional(a), ConcreteType::Optional(b)) => self.unify(a, b, location),

            // Optional(T) unifies with T and vice-versa (T is a subtype of Optional(T))
            (ConcreteType::Optional(inner), other) | (other, ConcreteType::Optional(inner)) => {
                // Allow the inner type to unify with the concrete type — the Optional wrapper
                // is a compile-time annotation, not a runtime wrapper, so both representations
                // are compatible at the WASM level.
                self.unify(inner, other, location)
            }

            // Types cannot be unified
            _ => Err(CompilerError::type_error(
                format!("Cannot unify types: {} and {}", left, right),
                None,
                Some(location.clone()),
            )),
        }
    }

    /// Check subtype relationship
    fn check_subtype(
        &mut self,
        subtype: &ConcreteType,
        supertype: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<(), CompilerError> {
        let subtype = self.apply_substitution(subtype);
        let supertype = self.apply_substitution(supertype);

        if subtype.is_assignable_to(&supertype) {
            Ok(())
        } else {
            Err(CompilerError::type_error(
                format!("Type {} is not a subtype of {}", subtype, supertype),
                None,
                Some(location.clone()),
            ))
        }
    }

    /// Check interface implementation
    fn check_implements(
        &mut self,
        type_: &ConcreteType,
        interface: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<(), CompilerError> {
        // Simplified implementation - would need full interface checking
        match (type_, interface) {
            (ConcreteType::Class { .. }, ConcreteType::Interface { .. }) => {
                // Would check if class implements all interface methods
                Ok(())
            }
            _ => Err(CompilerError::type_error(
                format!("Type {} does not implement interface {}", type_, interface),
                None,
                Some(location.clone()),
            )),
        }
    }

    /// Check if type has specific member
    fn check_has_member(
        &mut self,
        type_: &ConcreteType,
        member_name: &str,
        member_type: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<(), CompilerError> {
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
                        format!("Array type does not have member: {}", member_name),
                        None,
                        Some(location.clone()),
                    )),
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
                        format!("String type does not have member: {}", member_name),
                        None,
                        Some(location.clone()),
                    )),
                }
            }

            ConcreteType::Class { .. } => {
                // Would look up class definition and check members
                // For now, assume all member accesses are valid
                Ok(())
            }

            _ => Err(CompilerError::type_error(
                format!("Type {} does not have member: {}", type_, member_name),
                None,
                Some(location.clone()),
            )),
        }
    }

    /// Bind a type variable to a concrete type
    fn bind_type_var(
        &mut self,
        var: TypeVarId,
        type_: ConcreteType,
        location: &SourceLocation,
    ) -> Result<(), CompilerError> {
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
    pub fn apply_substitution(&self, type_: &ConcreteType) -> ConcreteType {
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

            ConcreteType::Matrix(element_type) => {
                ConcreteType::Matrix(Box::new(self.apply_substitution(element_type)))
            }

            ConcreteType::Function {
                parameters,
                return_type,
                is_background,
            } => ConcreteType::Function {
                parameters: parameters
                    .iter()
                    .map(|p| self.apply_substitution(p))
                    .collect(),
                return_type: Box::new(self.apply_substitution(return_type)),
                is_background: *is_background,
            },

            ConcreteType::Class {
                symbol_id,
                type_args,
            } => ConcreteType::Class {
                symbol_id: *symbol_id,
                type_args: type_args
                    .iter()
                    .map(|t| self.apply_substitution(t))
                    .collect(),
            },

            ConcreteType::Interface {
                symbol_id,
                type_args,
            } => ConcreteType::Interface {
                symbol_id: *symbol_id,
                type_args: type_args
                    .iter()
                    .map(|t| self.apply_substitution(t))
                    .collect(),
            },

            ConcreteType::Tuple(types) => {
                ConcreteType::Tuple(types.iter().map(|t| self.apply_substitution(t)).collect())
            }

            ConcreteType::Union(types) => {
                ConcreteType::Union(types.iter().map(|t| self.apply_substitution(t)).collect())
            }

            ConcreteType::Intersection(types) => ConcreteType::Intersection(
                types.iter().map(|t| self.apply_substitution(t)).collect(),
            ),

            ConcreteType::Optional(inner) => {
                ConcreteType::Optional(Box::new(self.apply_substitution(inner)))
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

            ConcreteType::Matrix(element_type) => self.occurs_check(var, element_type),

            ConcreteType::Function {
                parameters,
                return_type,
                ..
            } => {
                parameters.iter().any(|p| self.occurs_check(var, p))
                    || self.occurs_check(var, return_type)
            }

            ConcreteType::Class { type_args, .. } | ConcreteType::Interface { type_args, .. } => {
                type_args.iter().any(|t| self.occurs_check(var, t))
            }

            ConcreteType::Tuple(types)
            | ConcreteType::Union(types)
            | ConcreteType::Intersection(types) => types.iter().any(|t| self.occurs_check(var, t)),

            ConcreteType::Optional(inner) => self.occurs_check(var, inner),

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
                TypeConstraint::Subtype {
                    subtype, supertype, ..
                } => {
                    self.collect_type_vars(subtype, &mut unconstrained_vars);
                    self.collect_type_vars(supertype, &mut unconstrained_vars);
                }
                TypeConstraint::Implements {
                    type_, interface, ..
                } => {
                    self.collect_type_vars(type_, &mut unconstrained_vars);
                    self.collect_type_vars(interface, &mut unconstrained_vars);
                }
                TypeConstraint::HasMember {
                    type_, member_type, ..
                } => {
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
            ConcreteType::Matrix(element_type) => {
                self.collect_type_vars(element_type, vars);
            }
            ConcreteType::Function {
                parameters,
                return_type,
                ..
            } => {
                for param in parameters {
                    self.collect_type_vars(param, vars);
                }
                self.collect_type_vars(return_type, vars);
            }
            ConcreteType::Class { type_args, .. } | ConcreteType::Interface { type_args, .. } => {
                for arg in type_args {
                    self.collect_type_vars(arg, vars);
                }
            }
            ConcreteType::Tuple(types)
            | ConcreteType::Union(types)
            | ConcreteType::Intersection(types) => {
                for t in types {
                    self.collect_type_vars(t, vars);
                }
            }
            _ => {}
        }
    }

    /// Check if child_id is a subclass of parent_id
    fn is_subclass_of(
        &self,
        child_id: crate::resolver::SymbolId,
        parent_id: crate::resolver::SymbolId,
    ) -> bool {
        if let Some(symbol_table) = self.symbol_table {
            // Get the child class symbol
            if let Some(child_symbol) = symbol_table.get_symbol(child_id) {
                if let crate::resolver::SymbolKind::Class {
                    parent: Some(parent_symbol_id),
                    ..
                } = &child_symbol.kind
                {
                    // Check immediate parent
                    if *parent_symbol_id == parent_id {
                        return true;
                    }
                    // Recursively check parent's ancestors
                    return self.is_subclass_of(*parent_symbol_id, parent_id);
                }
            }
        }
        false
    }
}

impl<'a> Default for ConstraintSolver<'a> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_location() -> SourceLocation {
        SourceLocation {
            file: "test.cln".to_string(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
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
        assert_eq!(
            result.substitution.get(&TypeVarId(0)),
            Some(&ConcreteType::Integer)
        );
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
