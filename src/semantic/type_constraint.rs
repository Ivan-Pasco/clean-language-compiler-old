use crate::ast::Type;

/// Type constraint trait for semantic analysis
pub trait TypeConstraint: Send + Sync {
    #[allow(dead_code)]
    fn check(&self, type_: &Type) -> bool;
}

/// Numeric type constraint - accepts integers and floats
pub struct NumericTypeConstraint;

impl TypeConstraint for NumericTypeConstraint {
    fn check(&self, type_: &Type) -> bool {
        matches!(
            type_,
            Type::Integer | Type::Number | Type::IntegerSized { .. } | Type::NumberSized { .. }
        )
    }
}

/// Base type constraint - accepts basic types
pub struct BaseTypeConstraint;

impl TypeConstraint for BaseTypeConstraint {
    fn check(&self, type_: &Type) -> bool {
        matches!(
            type_,
            Type::Integer | Type::Number | Type::String | Type::Boolean | Type::Void
        )
    }
}

/// Any type constraint - accepts all types
pub struct AnyTypeConstraint;

impl TypeConstraint for AnyTypeConstraint {
    fn check(&self, _type_: &Type) -> bool {
        true
    }
}

/// Comparable type constraint - accepts types that can be compared
pub struct ComparableConstraint;

impl TypeConstraint for ComparableConstraint {
    fn check(&self, type_: &Type) -> bool {
        matches!(
            type_,
            Type::Integer
                | Type::Number
                | Type::String
                | Type::IntegerSized { .. }
                | Type::NumberSized { .. }
        )
    }
}

/// Inheritance type constraint - checks inheritance relationships
pub struct InheritanceConstraint {
    pub base_type: Type,
}

impl InheritanceConstraint {
    #[allow(dead_code)]
    pub fn new(base_type: Type) -> Self {
        Self { base_type }
    }

    fn is_subclass(&self, child_name: &str, base_name: &str) -> bool {
        // For now, simple name equality check
        // In a full implementation, this would traverse the inheritance hierarchy
        child_name == base_name
    }
}

impl TypeConstraint for InheritanceConstraint {
    fn check(&self, type_: &Type) -> bool {
        if let Type::Object(class_name) = type_ {
            if let Type::Object(base_name) = &self.base_type {
                return class_name == base_name || self.is_subclass(class_name, base_name);
            }
        }
        false
    }
}
