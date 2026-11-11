//! Constraint solver for type inference in Clean Language
//!
//! Implements a constraint-based type inference system using unification
//! and substitution to solve type constraints generated during type checking.

use super::tast::{ConcreteType, TypeConstraint};
use crate::ast::SourceLocation;
use crate::error::CompilerError;
use std::collections::{HashMap, VecDeque};

/// Type variable identifier for constraint solving
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVarId(pub usize);

/// Type substitution mapping type variables to concrete types
pub type Substitution = HashMap<TypeVarId, ConcreteType>;

/// Constraint solver for type inference
#[derive(Debug)]
pub struct ConstraintSolver<'a> {
    /// Current substitution being built
    substitution: Substitution,

    /// Pending constraints to solve
    constraints: VecDeque<TypeConstraint>,

    /// Errors encountered during solving
    errors: Vec<CompilerError>,

    /// Symbol table for checking inheritance relationships
    symbol_table: Option<&'a crate::resolver::GlobalSymbolTable>,

    /// Per-class capability conformance closure supplied by `TypeInference`
    /// before solving starts. `class_conformance[class_sym]` is the full set
    /// of capabilities the class satisfies (own + transitive parent ones).
    /// Used by the Class → Interface unification rule to accept a class value
    /// in a capability-typed slot iff the class declared `can C` (nominally).
    /// See Clean Language Specification §Capabilities.
    class_conformance:
        HashMap<crate::resolver::SymbolId, std::collections::HashSet<crate::resolver::SymbolId>>,
}

/// Result of constraint solving
#[derive(Debug)]
pub struct SolverResult {
    pub substitution: Substitution,
    pub errors: Vec<CompilerError>,
}

impl<'a> ConstraintSolver<'a> {
    /// Create a new constraint solver
    pub fn new() -> Self {
        Self {
            substitution: HashMap::new(),
            constraints: VecDeque::new(),
            errors: Vec::new(),
            symbol_table: None,
            class_conformance: HashMap::new(),
        }
    }

    /// Create a new constraint solver with symbol table access
    pub fn with_symbol_table(symbol_table: &'a crate::resolver::GlobalSymbolTable) -> Self {
        Self {
            substitution: HashMap::new(),
            constraints: VecDeque::new(),
            errors: Vec::new(),
            symbol_table: Some(symbol_table),
            class_conformance: HashMap::new(),
        }
    }

    /// Install the per-class capability conformance closure computed by
    /// `TypeInference` from the resolver's vtable_descriptors. Consulted
    /// during `unify` for Class → Interface (capability) checks.
    pub fn set_class_conformance(
        &mut self,
        conformance: HashMap<
            crate::resolver::SymbolId,
            std::collections::HashSet<crate::resolver::SymbolId>,
        >,
    ) {
        self.class_conformance = conformance;
    }

    /// Add constraints to solve
    pub fn add_constraints(&mut self, constraints: Vec<TypeConstraint>) {
        for constraint in constraints {
            self.constraints.push_back(constraint);
        }
    }

    /// Solve all constraints and return the result
    pub fn solve(mut self) -> SolverResult {
        // Each step consumes one constraint; solving never adds constraints.
        while let Some(constraint) = self.constraints.pop_front() {
            if let Err(error) = self.solve_constraint(constraint) {
                self.errors.push(error);
            }
        }

        SolverResult {
            substitution: self.substitution,
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

            // Class → Interface (capability) unification.
            // A class value is compatible with a capability-typed slot iff the
            // class transitively conforms to that capability. Conformance is
            // nominal: the class must have declared `can C` explicitly (or
            // inherited the claim from a parent). Populated from the
            // resolver's vtable_descriptors via `set_class_conformance`.
            // See Clean Language Specification §Capabilities.
            (
                ConcreteType::Class {
                    symbol_id: class_id,
                    ..
                },
                ConcreteType::Interface {
                    symbol_id: cap_id, ..
                },
            ) => {
                let conforms = self
                    .class_conformance
                    .get(class_id)
                    .map(|set| set.contains(cap_id))
                    .unwrap_or(false);
                if conforms {
                    Ok(())
                } else {
                    // Resolve internal SymbolIds to declared names so the diagnostic
                    // matches the spec's SEM011 shape ("Class 'X' does not implement
                    // capability 'Y'; add `class X can Y` ...") rather than exposing
                    // raw #ID numbers to the user. Falls back to `#N` if the symbol
                    // table is unavailable (unit tests that construct the solver
                    // without `with_symbol_table`).
                    let (class_name, cap_name) = if let Some(st) = self.symbol_table {
                        (
                            st.get_symbol(*class_id).map(|s| s.name.clone()),
                            st.get_symbol(*cap_id).map(|s| s.name.clone()),
                        )
                    } else {
                        (None, None)
                    };
                    let class_label = class_name
                        .clone()
                        .unwrap_or_else(|| format!("#{}", class_id.0));
                    let cap_label = cap_name.clone().unwrap_or_else(|| format!("#{}", cap_id.0));
                    let message = if class_name.is_some() && cap_name.is_some() {
                        format!(
                            "Class '{}' does not implement capability '{}' — add `can {}` to the class header and implement its required methods",
                            class_label, cap_label, cap_label
                        )
                    } else {
                        format!(
                            "Class {} does not conform to capability {} — add `can <capability>` to the class header",
                            class_label, cap_label
                        )
                    };
                    Err(CompilerError::semantic_error_with_code(
                        message,
                        None,
                        Some(location.clone()),
                        "SEM011",
                    ))
                }
            }

            // Interface → Interface unification. Two capability-typed values
            // unify iff they refer to the exact same capability symbol.
            // Nominal typing — no structural subtyping between capabilities.
            (
                ConcreteType::Interface {
                    symbol_id: a,
                    type_args: aa,
                },
                ConcreteType::Interface {
                    symbol_id: b,
                    type_args: bb,
                },
            ) => {
                if a == b && aa.len() == bb.len() {
                    for (t1, t2) in aa.iter().zip(bb.iter()) {
                        self.unify(t1, t2, location)?;
                    }
                    Ok(())
                } else {
                    Err(CompilerError::type_error(
                        format!(
                            "Cannot unify incompatible capabilities: #{} vs #{}",
                            a.0, b.0
                        ),
                        None,
                        Some(location.clone()),
                    ))
                }
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

            // none unifies with any type here: unification relates the two
            // sides of comparisons and branches (`x == none`). Whether a slot
            // may *hold* none is directional and checked in `check_subtype`
            // (only `T?` and `any` do, type-system.md §8).
            (ConcreteType::Null, _) | (_, ConcreteType::Null) => Ok(()),

            // Unknown type unifies with anything (error recovery)
            (ConcreteType::Unknown, _) | (_, ConcreteType::Unknown) => Ok(()),

            // Any type unifies with anything (dynamic typing)
            // At runtime, values are boxed with type tags for proper handling
            (ConcreteType::Any, _) | (_, ConcreteType::Any) => Ok(()),

            // Function references unify with Integer (handler/callback parameters)
            // In WASM, function references become integer indices in the function table
            // (tests/cln/functions/function_references_distinct_values.cln).
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

    /// Check that a value of type `subtype` may be stored where a
    /// `supertype` is expected (assignment, argument, return, operand).
    ///
    /// Unlike `unify` this is directional (type-system.md §Assignability):
    /// `integer` widens to `number` but a `number` is not an `integer`, a
    /// child class is its parent class but not the other way round, and a
    /// list converts only when its elements are stored the same way.
    fn check_subtype(
        &mut self,
        subtype: &ConcreteType,
        supertype: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<(), CompilerError> {
        let from = self.apply_substitution(subtype);
        let to = self.apply_substitution(supertype);
        let mismatch = |from: &ConcreteType, to: &ConcreteType| {
            CompilerError::type_error(
                format!("A value of type {} cannot be used as {}", from, to),
                None,
                Some(location.clone()),
            )
        };

        match (&from, &to) {
            (a, b) if a == b => Ok(()),

            // Type variables are bound as in unification.
            (ConcreteType::Generic { .. }, _) | (_, ConcreteType::Generic { .. }) => {
                self.unify(&from, &to, location)
            }

            // Only `T?` (and `any`) hold none; every plain type rejects it
            // (type-system.md §8).
            (ConcreteType::Null, target) if !Self::accepts_none(target) => Err(
                Self::none_into_plain(&target.source_name(self.symbol_table), location),
            ),

            // A `T?` may be none, so it is not a plain `T`: `value!` or
            // `value default fallback` gives the `T` (04-type-system.md,
            // Nullable Types).
            (ConcreteType::Optional(inner), target)
                if !matches!(
                    target,
                    ConcreteType::Optional(_)
                        | ConcreteType::Unknown
                        | ConcreteType::Any
                        | ConcreteType::Undefined
                        | ConcreteType::Union(_)
                ) =>
            {
                Err(CompilerError::type_error(
                    format!(
                        "A value of type {}? may be none and cannot be used as {}",
                        inner.source_name(self.symbol_table),
                        target.source_name(self.symbol_table)
                    ),
                    Some(
                        "Use `value!` when it cannot be none, `value default fallback` to \
                         supply one, or declare the target as optional"
                            .to_string(),
                    ),
                    Some(location.clone()),
                ))
            }

            // Error recovery, the dynamic type, and none into a `T?`.
            (ConcreteType::Unknown | ConcreteType::Any | ConcreteType::Null, _)
            | (_, ConcreteType::Unknown | ConcreteType::Any) => Ok(()),

            // Numbers convert only by widening (integer → number, narrower
            // sized → wider sized of the same signedness).
            (a, b) if Self::is_plain_numeric(a) && Self::is_plain_numeric(b) => {
                if a.is_assignable_to(b) {
                    Ok(())
                } else {
                    Err(CompilerError::type_error(
                        format!(
                            "A value of type {} cannot be used as {} without an explicit conversion",
                            a, b
                        ),
                        Some(format!(
                            "Convert it explicitly, e.g. `value.{}()`",
                            if matches!(b, ConcreteType::Number | ConcreteType::NumberSized { .. }) {
                                "toNumber"
                            } else {
                                "toInteger"
                            }
                        )),
                        Some(location.clone()),
                    ))
                }
            }

            // A child class is assignable to its parent class only.
            (
                ConcreteType::Class {
                    symbol_id: child,
                    type_args: child_args,
                },
                ConcreteType::Class {
                    symbol_id: parent,
                    type_args: parent_args,
                },
            ) => {
                if child != parent && !self.is_subclass_of(*child, *parent) {
                    return Err(self.class_mismatch(*child, *parent, location));
                }
                if child_args.len() != parent_args.len() {
                    return Err(mismatch(&from, &to));
                }
                for (a, b) in child_args.iter().zip(parent_args.iter()) {
                    self.unify(a, b, location)?;
                }
                Ok(())
            }

            // Lists and matrices are covariant in their element type, as long
            // as the elements are stored the same way: a list of integers
            // (4-byte slots) is not a list of numbers (8-byte slots).
            (ConcreteType::Array(a), ConcreteType::Array(b))
            | (ConcreteType::Matrix(a), ConcreteType::Matrix(b)) => {
                let (a, b) = (a.as_ref(), b.as_ref());
                if a != b && (Self::is_plain_numeric(a) || Self::is_plain_numeric(b)) {
                    if matches!(b, ConcreteType::Unknown | ConcreteType::Any)
                        || matches!(a, ConcreteType::Unknown | ConcreteType::Any)
                    {
                        return Ok(());
                    }
                    return Err(CompilerError::type_error(
                        format!("A value of type {} cannot be used as {}", from, to),
                        Some(format!(
                            "The elements are stored differently; build a new list of {} in an `iterate` loop",
                            b
                        )),
                        Some(location.clone()),
                    ));
                }
                self.check_subtype(a, b, location)
            }

            (ConcreteType::Optional(a), ConcreteType::Optional(b)) => {
                let (a, b) = ((**a).clone(), (**b).clone());
                self.check_subtype(&a, &b, location)
            }
            (_, ConcreteType::Optional(inner)) => {
                let inner = (**inner).clone();
                self.check_subtype(&from, &inner, location)
            }
            (ConcreteType::Optional(inner), _) => {
                let inner = (**inner).clone();
                self.check_subtype(&inner, &to, location)
            }

            // Two different scalar types (string, boolean, a number).
            (a, b) if Self::is_scalar(a) && Self::is_scalar(b) => Err(mismatch(&from, &to)),

            // Everything else (functions, capabilities, unions, pairs)
            // follows the unification rules.
            _ => self.unify(&from, &to, location),
        }
    }

    /// Whether a slot of type `t` may hold none: `T?`, `any`, and types not
    /// known yet. Every plain type rejects none (type-system.md §8).
    pub(crate) fn accepts_none(t: &ConcreteType) -> bool {
        match t {
            ConcreteType::Optional(_)
            | ConcreteType::Any
            | ConcreteType::Unknown
            | ConcreteType::Null
            | ConcreteType::Undefined
            | ConcreteType::Generic { .. } => true,
            ConcreteType::Union(members) => members.iter().any(Self::accepts_none),
            _ => false,
        }
    }

    /// The error for none stored into a plain type.
    pub(crate) fn none_into_plain(target: &str, location: &SourceLocation) -> CompilerError {
        CompilerError::type_error(
            format!(
                "none cannot be used as {}: a plain {} is never none",
                target, target
            ),
            Some(format!(
                "Declare the type as `{}?` to allow none, or use a value",
                target
            )),
            Some(location.clone()),
        )
    }

    /// A string, boolean, or number of any width.
    fn is_scalar(t: &ConcreteType) -> bool {
        matches!(t, ConcreteType::String | ConcreteType::Boolean) || Self::is_plain_numeric(t)
    }

    /// `integer` / `number` of any width (not a nullable `T?`).
    fn is_plain_numeric(t: &ConcreteType) -> bool {
        t.is_numeric() && !matches!(t, ConcreteType::Optional(_))
    }

    /// SEM001 for a class value used where an unrelated (or more derived)
    /// class is expected, naming both classes.
    fn class_mismatch(
        &self,
        from: crate::resolver::SymbolId,
        to: crate::resolver::SymbolId,
        location: &SourceLocation,
    ) -> CompilerError {
        let name = |id: crate::resolver::SymbolId| {
            self.symbol_table
                .and_then(|st| st.get_symbol(id).map(|s| s.name.clone()))
                .unwrap_or_else(|| format!("#{}", id.0))
        };
        let (from_name, to_name) = (name(from), name(to));
        let help = if self.is_subclass_of(to, from) {
            format!(
                "'{}' is a parent of '{}': a parent class value is not a '{}'",
                from_name, to_name, to_name
            )
        } else {
            format!("'{}' does not inherit from '{}'", from_name, to_name)
        };
        CompilerError::type_error(
            format!(
                "A value of class '{}' cannot be used as '{}'",
                from_name, to_name
            ),
            Some(help),
            Some(location.clone()),
        )
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

            ConcreteType::Union(types) => {
                ConcreteType::Union(types.iter().map(|t| self.apply_substitution(t)).collect())
            }

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

            ConcreteType::Union(types) => types.iter().any(|t| self.occurs_check(var, t)),

            ConcreteType::Optional(inner) => self.occurs_check(var, inner),

            _ => false,
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
