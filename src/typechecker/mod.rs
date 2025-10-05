//! Type Checker - Stage 5 of the Clean Language compiler pipeline
//!
//! This module implements type inference and checking, transforming Resolved HIR
//! into TAST (Typed Abstract Syntax Tree) where all types are fully resolved.
//!
//! Features:
//! - Constraint-based type inference using Hindley-Milner algorithm
//! - Support for polymorphic types and generics
//! - Type checking for all language constructs
//! - Method resolution with inheritance
//! - Built-in type operations and conversions
//! - Comprehensive error reporting with type information

use crate::error::CompilerError;
use crate::resolver::*;
use std::collections::HashMap;

pub mod constraint_solver;
pub mod tast;
pub mod type_inference;

pub use constraint_solver::{ConstraintSolver, TypeVarId};
pub use tast::{
    BinaryOperator, ConcreteType, TastBlock, TastClass, TastExpression, TastFunction, TastProgram,
    TastStatement, TypeConstraint, UnaryOperator, Visibility,
};
pub use type_inference::{BuiltinTypes, InferenceResult, TypeInference};

/// Type checker - Stage 5 of the pipeline  
///
/// This is the main interface for type checking that wraps the constraint-based
/// type inference engine. It transforms resolved HIR into TAST.
#[derive(Debug)]
#[allow(dead_code)]
pub struct TypeChecker {
    /// The underlying type inference engine
    inference_engine: Option<TypeInference<'static>>,
}

/// Type checking result
#[derive(Debug)]
pub struct TypeCheckResult {
    pub tast: TastProgram,
    pub type_env: HashMap<SymbolId, ConcreteType>,
    pub warnings: Vec<CompilerError>,
}

impl TypeChecker {
    /// Create a new type checker
    pub fn new() -> Self {
        Self {
            inference_engine: None,
        }
    }

    /// Type check a resolved HIR program using constraint-based type inference
    pub fn check(resolved_hir: ResolvedHirProgram) -> Result<TypeCheckResult, Vec<CompilerError>> {
        // Clone symbol table to satisfy borrow checker (TypeInference needs a reference
        // but infer_types consumes the resolved_hir)
        let symbol_table = resolved_hir.symbol_table.clone();

        // Create type inference engine with symbol table from resolution
        let inference_engine = TypeInference::new(&symbol_table);

        // Perform type inference
        let inference_result = inference_engine.infer_types(resolved_hir);

        if !inference_result.errors.is_empty() {
            Err(inference_result.errors)
        } else {
            Ok(TypeCheckResult {
                tast: inference_result.tast,
                type_env: inference_result.type_env,
                warnings: inference_result.warnings,
            })
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::SourceLocation;

    #[allow(dead_code)]
    fn test_location() -> SourceLocation {
        SourceLocation {
            file: "test.cln".to_string(),
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_typechecker_creation() {
        let checker = TypeChecker::new();
        assert!(checker.inference_engine.is_none());
    }

    #[test]
    fn test_concrete_type_assignability() {
        // Integer can be assigned to Number
        assert!(ConcreteType::Integer.is_assignable_to(&ConcreteType::Number));

        // Number cannot be assigned to Integer
        assert!(!ConcreteType::Number.is_assignable_to(&ConcreteType::Integer));

        // Same types are assignable
        assert!(ConcreteType::String.is_assignable_to(&ConcreteType::String));

        // Null can be assigned to any type
        assert!(ConcreteType::Null.is_assignable_to(&ConcreteType::String));
    }

    #[test]
    fn test_common_supertype() {
        // Integer and Number -> Number
        let result = ConcreteType::Integer.common_supertype(&ConcreteType::Number);
        assert_eq!(result, ConcreteType::Number);

        // Same types return same type
        let result = ConcreteType::String.common_supertype(&ConcreteType::String);
        assert_eq!(result, ConcreteType::String);

        // Different incompatible types -> Union
        let result = ConcreteType::String.common_supertype(&ConcreteType::Boolean);
        if let ConcreteType::Union(types) = result {
            assert_eq!(types.len(), 2);
            assert!(types.contains(&ConcreteType::String));
            assert!(types.contains(&ConcreteType::Boolean));
        } else {
            panic!("Expected union type");
        }
    }
}
