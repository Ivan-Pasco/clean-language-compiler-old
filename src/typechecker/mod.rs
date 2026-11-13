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
use std::collections::{HashMap, HashSet};

pub mod constraint_solver;
mod erase_optional;
mod instance_calls;
#[cfg(test)]
mod property_tests;
pub mod tast;
pub mod type_inference;

pub use tast::{ConcreteType, TastProgram};
pub use type_inference::TypeInference;

/// Type checker - Stage 5 of the pipeline
///
/// Entry point for type checking: wraps the constraint-based type inference
/// engine and transforms resolved HIR into TAST.
pub struct TypeChecker;

/// Type checking result
#[derive(Debug)]
pub struct TypeCheckResult {
    pub tast: TastProgram,
    pub warnings: Vec<CompilerError>,
}

impl TypeChecker {
    /// Type check a resolved HIR program using constraint-based type inference
    pub fn check(resolved_hir: ResolvedHirProgram) -> Result<TypeCheckResult, Vec<CompilerError>> {
        Self::check_project(resolved_hir, HashMap::new(), HashSet::new())
    }

    /// Like `check`, for a multi-file project.
    ///
    /// `required_counts` pre-seeds the required parameter count map, for
    /// bridge language aliases with optional parameters (param_defaults) that
    /// the resolver cannot encode in `SymbolKind::Function` alone.
    /// `plugin_generated_files` names the plugin-injected modules, whose code
    /// may call private functions of user modules.
    pub fn check_project(
        resolved_hir: ResolvedHirProgram,
        required_counts: HashMap<crate::resolver::SymbolId, usize>,
        plugin_generated_files: HashSet<String>,
    ) -> Result<TypeCheckResult, Vec<CompilerError>> {
        // Clone symbol table to satisfy borrow checker (TypeInference needs a reference
        // but infer_types consumes the resolved_hir)
        let symbol_table = resolved_hir.symbol_table.clone();

        // Create type inference engine with symbol table from resolution
        let mut inference_engine = TypeInference::new(&symbol_table);

        if !required_counts.is_empty() {
            inference_engine.seed_required_param_counts(required_counts);
        }
        inference_engine.set_plugin_generated_files(plugin_generated_files);

        // Perform type inference
        let inference_result = inference_engine.infer_types(resolved_hir);

        if !inference_result.errors.is_empty() {
            Err(inference_result.errors)
        } else {
            Ok(TypeCheckResult {
                tast: inference_result.tast,
                warnings: inference_result.warnings,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
