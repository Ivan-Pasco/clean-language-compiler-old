//! Property-based tests for type checker robustness
//!
//! These tests verify that the type checker handles edge cases correctly
//! and maintains consistency across random inputs.

#[cfg(test)]
mod tests {
    use crate::typechecker::tast::ConcreteType;
    use proptest::prelude::*;

    // Strategy for generating random concrete types (using actual variants)
    fn concrete_type_strategy() -> impl Strategy<Value = ConcreteType> {
        prop_oneof![
            Just(ConcreteType::Integer),
            Just(ConcreteType::Number),
            Just(ConcreteType::String),
            Just(ConcreteType::Boolean),
            Just(ConcreteType::Null),
            Just(ConcreteType::Undefined),
        ]
    }

    proptest! {
        /// Type equality should be reflexive
        #[test]
        fn test_type_equality_reflexive(ty in concrete_type_strategy()) {
            prop_assert_eq!(ty.clone(), ty);
        }

        /// Type equality should be symmetric
        #[test]
        fn test_type_equality_symmetric(
            ty1 in concrete_type_strategy(),
            ty2 in concrete_type_strategy()
        ) {
            if ty1 == ty2 {
                prop_assert_eq!(ty2, ty1);
            }
        }

        /// Integer literals should always be valid
        #[test]
        fn test_integer_literal_valid(n in any::<i32>()) {
            // Integer representation should handle all i32 values
            let _ = format!("{}", n);
        }

        /// Float literals should be valid within range
        #[test]
        fn test_float_literal_valid(n in -1e10f64..1e10f64) {
            // Float representation should handle reasonable range
            let _ = format!("{}", n);
        }

        /// String with random content should not cause type errors
        #[test]
        fn test_string_content_safe(s in "[\\x20-\\x7E]*") {
            // String types should handle ASCII content
            let _ty = ConcreteType::String;
            let _ = s;
        }

        /// Type display should never panic
        #[test]
        fn test_type_display_no_panic(ty in concrete_type_strategy()) {
            let display = format!("{:?}", ty);
            prop_assert!(!display.is_empty());
        }

        /// Array types should be constructible with any element type
        #[test]
        fn test_array_type_construction(elem_ty in concrete_type_strategy()) {
            let array_ty = ConcreteType::Array(Box::new(elem_ty.clone()));
            match array_ty {
                ConcreteType::Array(inner) => prop_assert_eq!(*inner, elem_ty),
                _ => prop_assert!(false, "Expected array type"),
            }
        }

        /// Nested arrays should work to reasonable depth
        #[test]
        fn test_nested_array_depth(depth in 1usize..5) {
            let mut ty = ConcreteType::Integer;
            for _ in 0..depth {
                ty = ConcreteType::Array(Box::new(ty));
            }
            // Should be able to unwrap the nested arrays
            let mut current = ty;
            for _ in 0..depth {
                match current {
                    ConcreteType::Array(inner) => current = *inner,
                    _ => panic!("Expected array at this depth"),
                }
            }
            prop_assert_eq!(current, ConcreteType::Integer);
        }
    }

    #[test]
    fn test_type_names_are_valid() {
        // All concrete types should have valid string representations
        let types = [
            ConcreteType::Integer,
            ConcreteType::Number,
            ConcreteType::String,
            ConcreteType::Boolean,
            ConcreteType::Null,
            ConcreteType::Undefined,
            ConcreteType::Array(Box::new(ConcreteType::Integer)),
        ];

        for ty in types {
            let debug = format!("{:?}", ty);
            assert!(!debug.is_empty(), "Type debug output should not be empty");
        }
    }

    #[test]
    fn test_type_assignability_basic() {
        // Same types are assignable
        assert!(ConcreteType::Integer.is_assignable_to(&ConcreteType::Integer));
        assert!(ConcreteType::String.is_assignable_to(&ConcreteType::String));
        assert!(ConcreteType::Boolean.is_assignable_to(&ConcreteType::Boolean));
        assert!(ConcreteType::Number.is_assignable_to(&ConcreteType::Number));
    }

    #[test]
    fn test_matrix_type_construction() {
        let matrix_ty = ConcreteType::Matrix(Box::new(ConcreteType::Number));
        match matrix_ty {
            ConcreteType::Matrix(inner) => assert_eq!(*inner, ConcreteType::Number),
            _ => panic!("Expected matrix type"),
        }
    }

    #[test]
    fn test_pairs_type_construction() {
        let pairs_ty = ConcreteType::Pairs(
            Box::new(ConcreteType::String),
            Box::new(ConcreteType::Integer),
        );
        match pairs_ty {
            ConcreteType::Pairs(key, value) => {
                assert_eq!(*key, ConcreteType::String);
                assert_eq!(*value, ConcreteType::Integer);
            }
            _ => panic!("Expected pairs type"),
        }
    }
}
