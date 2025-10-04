use crate::ast::{SourceLocation, Type};
use std::collections::{HashMap, HashSet};

/// Type variable identifier for constraint solving
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeVar {
    pub id: u32,
    pub name: Option<String>, // For debugging and error messages
}

impl TypeVar {
    pub fn new(id: u32) -> Self {
        Self { id, name: None }
    }

    pub fn with_name(id: u32, name: String) -> Self {
        Self {
            id,
            name: Some(name),
        }
    }
}

/// Constraint types for type inference
#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    /// Two types must be equal: T1 = T2
    Equality {
        left: ConstraintType,
        right: ConstraintType,
        location: Option<SourceLocation>,
        reason: String,
    },

    /// Type must be a subtype of another: T1 <: T2
    Subtype {
        subtype: ConstraintType,
        supertype: ConstraintType,
        location: Option<SourceLocation>,
        reason: String,
    },

    /// Type must have a specific property/capability
    HasProperty {
        type_: ConstraintType,
        property: TypeProperty,
        location: Option<SourceLocation>,
        reason: String,
    },

    /// Function type constraint: (T1, T2, ...) -> TR
    Function {
        params: Vec<ConstraintType>,
        return_type: ConstraintType,
        function_type: ConstraintType,
        location: Option<SourceLocation>,
        reason: String,
    },

    /// Array/List element constraint: Array<T>
    ArrayElement {
        array_type: ConstraintType,
        element_type: ConstraintType,
        location: Option<SourceLocation>,
        reason: String,
    },

    /// Class field/method constraint
    ClassMember {
        class_type: ConstraintType,
        member_name: String,
        member_type: ConstraintType,
        location: Option<SourceLocation>,
        reason: String,
    },
}

/// Types used in constraints (can be concrete types or type variables)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConstraintType {
    /// Concrete type from AST
    Concrete(Type),
    /// Type variable to be solved
    Variable(TypeVar),
    /// Function type
    Function {
        params: Vec<ConstraintType>,
        return_type: Box<ConstraintType>,
    },
    /// Generic type with parameters
    Generic {
        name: String,
        params: Vec<ConstraintType>,
    },
    /// Union type (for error recovery)
    Union(Vec<ConstraintType>),
    /// Bottom type (for unreachable code)
    Bottom,
    /// Top type (for any/unknown)
    Top,
}

/// Type properties that can be checked
#[derive(Debug, Clone, PartialEq)]
pub enum TypeProperty {
    /// Type supports numeric operations
    Numeric,
    /// Type supports comparison operations
    Comparable,
    /// Type supports string conversion
    Stringifiable,
    /// Type can be called as function
    Callable,
    /// Type supports indexing (arrays, strings)
    Indexable,
    /// Type supports iteration
    Iterable,
    /// Type has specific method
    HasMethod(String),
    /// Type has specific field
    HasField(String),
}

/// Collection of constraints for solving
#[derive(Debug, Clone)]
pub struct ConstraintSet {
    pub constraints: Vec<Constraint>,
    pub type_vars: HashMap<u32, TypeVarInfo>,
    pub next_var_id: u32,
}

/// Information about a type variable
#[derive(Debug, Clone)]
pub struct TypeVarInfo {
    pub var: TypeVar,
    pub bounds: Vec<ConstraintType>,
    pub source_location: Option<SourceLocation>,
    pub created_from: String, // Context where this type var was created
}

impl ConstraintSet {
    pub fn new() -> Self {
        Self {
            constraints: Vec::new(),
            type_vars: HashMap::new(),
            next_var_id: 0,
        }
    }

    /// Create a fresh type variable
    pub fn fresh_type_var(&mut self, context: String, location: Option<SourceLocation>) -> TypeVar {
        let id = self.next_var_id;
        self.next_var_id += 1;

        let var = TypeVar::new(id);
        self.type_vars.insert(
            id,
            TypeVarInfo {
                var: var.clone(),
                bounds: Vec::new(),
                source_location: location,
                created_from: context,
            },
        );

        var
    }

    /// Add a constraint to the set
    pub fn add_constraint(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }

    /// Add equality constraint: left = right
    pub fn add_equality(
        &mut self,
        left: ConstraintType,
        right: ConstraintType,
        location: Option<SourceLocation>,
        reason: String,
    ) {
        self.add_constraint(Constraint::Equality {
            left,
            right,
            location,
            reason,
        });
    }

    /// Add subtype constraint: subtype <: supertype
    pub fn add_subtype(
        &mut self,
        subtype: ConstraintType,
        supertype: ConstraintType,
        location: Option<SourceLocation>,
        reason: String,
    ) {
        self.add_constraint(Constraint::Subtype {
            subtype,
            supertype,
            location,
            reason,
        });
    }

    /// Add property constraint
    pub fn add_property(
        &mut self,
        type_: ConstraintType,
        property: TypeProperty,
        location: Option<SourceLocation>,
        reason: String,
    ) {
        self.add_constraint(Constraint::HasProperty {
            type_,
            property,
            location,
            reason,
        });
    }

    /// Get all constraints involving a specific type variable
    pub fn constraints_for_var(&self, var_id: u32) -> Vec<&Constraint> {
        self.constraints
            .iter()
            .filter(|c| self.constraint_mentions_var(c, var_id))
            .collect()
    }

    /// Check if a constraint mentions a specific type variable
    fn constraint_mentions_var(&self, constraint: &Constraint, var_id: u32) -> bool {
        match constraint {
            Constraint::Equality { left, right, .. } => {
                self.constraint_type_mentions_var(left, var_id)
                    || self.constraint_type_mentions_var(right, var_id)
            }
            Constraint::Subtype {
                subtype, supertype, ..
            } => {
                self.constraint_type_mentions_var(subtype, var_id)
                    || self.constraint_type_mentions_var(supertype, var_id)
            }
            Constraint::HasProperty { type_, .. } => {
                self.constraint_type_mentions_var(type_, var_id)
            }
            Constraint::Function {
                params,
                return_type,
                function_type,
                ..
            } => {
                params
                    .iter()
                    .any(|p| self.constraint_type_mentions_var(p, var_id))
                    || self.constraint_type_mentions_var(return_type, var_id)
                    || self.constraint_type_mentions_var(function_type, var_id)
            }
            Constraint::ArrayElement {
                array_type,
                element_type,
                ..
            } => {
                self.constraint_type_mentions_var(array_type, var_id)
                    || self.constraint_type_mentions_var(element_type, var_id)
            }
            Constraint::ClassMember {
                class_type,
                member_type,
                ..
            } => {
                self.constraint_type_mentions_var(class_type, var_id)
                    || self.constraint_type_mentions_var(member_type, var_id)
            }
        }
    }

    /// Check if a constraint type mentions a specific type variable
    fn constraint_type_mentions_var(&self, constraint_type: &ConstraintType, var_id: u32) -> bool {
        match constraint_type {
            ConstraintType::Variable(var) => var.id == var_id,
            ConstraintType::Function {
                params,
                return_type,
            } => {
                params
                    .iter()
                    .any(|p| self.constraint_type_mentions_var(p, var_id))
                    || self.constraint_type_mentions_var(return_type, var_id)
            }
            ConstraintType::Generic { params, .. } => params
                .iter()
                .any(|p| self.constraint_type_mentions_var(p, var_id)),
            ConstraintType::Union(types) => types
                .iter()
                .any(|t| self.constraint_type_mentions_var(t, var_id)),
            ConstraintType::Concrete(_) | ConstraintType::Bottom | ConstraintType::Top => false,
        }
    }

    /// Get free type variables in the constraint set
    pub fn free_variables(&self) -> HashSet<u32> {
        let mut vars = HashSet::new();
        for constraint in &self.constraints {
            self.collect_free_vars_from_constraint(constraint, &mut vars);
        }
        vars
    }

    fn collect_free_vars_from_constraint(&self, constraint: &Constraint, vars: &mut HashSet<u32>) {
        match constraint {
            Constraint::Equality { left, right, .. } => {
                self.collect_free_vars_from_type(left, vars);
                self.collect_free_vars_from_type(right, vars);
            }
            Constraint::Subtype {
                subtype, supertype, ..
            } => {
                self.collect_free_vars_from_type(subtype, vars);
                self.collect_free_vars_from_type(supertype, vars);
            }
            Constraint::HasProperty { type_, .. } => {
                self.collect_free_vars_from_type(type_, vars);
            }
            Constraint::Function {
                params,
                return_type,
                function_type,
                ..
            } => {
                for param in params {
                    self.collect_free_vars_from_type(param, vars);
                }
                self.collect_free_vars_from_type(return_type, vars);
                self.collect_free_vars_from_type(function_type, vars);
            }
            Constraint::ArrayElement {
                array_type,
                element_type,
                ..
            } => {
                self.collect_free_vars_from_type(array_type, vars);
                self.collect_free_vars_from_type(element_type, vars);
            }
            Constraint::ClassMember {
                class_type,
                member_type,
                ..
            } => {
                self.collect_free_vars_from_type(class_type, vars);
                self.collect_free_vars_from_type(member_type, vars);
            }
        }
    }

    fn collect_free_vars_from_type(
        &self,
        constraint_type: &ConstraintType,
        vars: &mut HashSet<u32>,
    ) {
        match constraint_type {
            ConstraintType::Variable(var) => {
                vars.insert(var.id);
            }
            ConstraintType::Function {
                params,
                return_type,
            } => {
                for param in params {
                    self.collect_free_vars_from_type(param, vars);
                }
                self.collect_free_vars_from_type(return_type, vars);
            }
            ConstraintType::Generic { params, .. } => {
                for param in params {
                    self.collect_free_vars_from_type(param, vars);
                }
            }
            ConstraintType::Union(types) => {
                for typ in types {
                    self.collect_free_vars_from_type(typ, vars);
                }
            }
            ConstraintType::Concrete(_) | ConstraintType::Bottom | ConstraintType::Top => {}
        }
    }
}

impl Default for ConstraintSet {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert AST Type to ConstraintType
impl From<Type> for ConstraintType {
    fn from(ast_type: Type) -> Self {
        match ast_type {
            Type::List(inner) => ConstraintType::Generic {
                name: "List".to_string(),
                params: vec![ConstraintType::from(*inner)],
            },
            Type::Function(params, return_type) => ConstraintType::Function {
                params: params.into_iter().map(ConstraintType::from).collect(),
                return_type: Box::new(ConstraintType::from(*return_type)),
            },
            other => ConstraintType::Concrete(other),
        }
    }
}

/// Convert ConstraintType back to AST Type (when possible)
impl TryFrom<ConstraintType> for Type {
    type Error = String;

    fn try_from(constraint_type: ConstraintType) -> Result<Self, Self::Error> {
        match constraint_type {
            ConstraintType::Concrete(typ) => Ok(typ),
            ConstraintType::Function {
                params,
                return_type,
            } => {
                let param_types: Result<Vec<Type>, String> =
                    params.into_iter().map(|p| p.try_into()).collect();
                let return_typ: Type = (*return_type).try_into()?;
                Ok(Type::Function(param_types?, Box::new(return_typ)))
            }
            ConstraintType::Generic { name, params } => match name.as_str() {
                "List" | "Array" => {
                    if params.len() == 1 {
                        let element_type: Type = params.into_iter().next().unwrap().try_into()?;
                        Ok(Type::List(Box::new(element_type)))
                    } else {
                        Err(format!(
                            "List type expects 1 parameter, got {}",
                            params.len()
                        ))
                    }
                }
                _ => Err(format!("Unknown generic type: {}", name)),
            },
            ConstraintType::Variable(var) => Err(format!(
                "Cannot convert unsolved type variable {:?} to concrete type",
                var
            )),
            ConstraintType::Union(_) => {
                Err("Cannot convert union type to concrete type".to_string())
            }
            ConstraintType::Bottom => {
                Err("Cannot convert bottom type to concrete type".to_string())
            }
            ConstraintType::Top => Ok(Type::Any),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraint_set_creation() {
        let mut constraints = ConstraintSet::new();
        let var1 = constraints.fresh_type_var("test".to_string(), None);
        let var2 = constraints.fresh_type_var("test2".to_string(), None);

        assert_eq!(var1.id, 0);
        assert_eq!(var2.id, 1);
        assert_eq!(constraints.type_vars.len(), 2);
    }

    #[test]
    fn test_equality_constraint() {
        let mut constraints = ConstraintSet::new();
        let var1 = constraints.fresh_type_var("test".to_string(), None);

        constraints.add_equality(
            ConstraintType::Variable(var1.clone()),
            ConstraintType::Concrete(Type::Integer),
            None,
            "test equality".to_string(),
        );

        assert_eq!(constraints.constraints.len(), 1);
        if let Constraint::Equality {
            left,
            right,
            reason,
            ..
        } = &constraints.constraints[0]
        {
            assert_eq!(*left, ConstraintType::Variable(var1));
            assert_eq!(*right, ConstraintType::Concrete(Type::Integer));
            assert_eq!(reason, "test equality");
        } else {
            panic!("Expected equality constraint");
        }
    }

    #[test]
    fn test_type_conversion() {
        let ast_type = Type::List(Box::new(Type::Integer));
        let constraint_type = ConstraintType::from(ast_type.clone());

        if let ConstraintType::Generic { name, params } = constraint_type {
            assert_eq!(name, "List");
            assert_eq!(params.len(), 1);
            assert_eq!(params[0], ConstraintType::Concrete(Type::Integer));
        } else {
            panic!("Expected generic List type");
        }

        let back_to_ast: Type = ConstraintType::Generic {
            name: "List".to_string(),
            params: vec![ConstraintType::Concrete(Type::Integer)],
        }
        .try_into()
        .unwrap();

        assert_eq!(back_to_ast, ast_type);
    }

    #[test]
    fn test_free_variables() {
        let mut constraints = ConstraintSet::new();
        let var1 = constraints.fresh_type_var("test1".to_string(), None);
        let var2 = constraints.fresh_type_var("test2".to_string(), None);

        constraints.add_equality(
            ConstraintType::Variable(var1.clone()),
            ConstraintType::Variable(var2.clone()),
            None,
            "test".to_string(),
        );

        let free_vars = constraints.free_variables();
        assert_eq!(free_vars.len(), 2);
        assert!(free_vars.contains(&var1.id));
        assert!(free_vars.contains(&var2.id));
    }
}
