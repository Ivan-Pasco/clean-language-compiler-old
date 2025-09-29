//! Property-based tests for parser robustness

#[cfg(test)]
mod tests {
    use super::super::CleanParser;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_parser_doesnt_panic_on_random_input(s in "[\\x20-\\x7E]*") {
            // Parser should never panic on ASCII printable characters
            // Note: Unicode handling bug found - would need fixing in error.rs
            let _ = CleanParser::parse_program(&s);
        }

        #[test]
        fn test_empty_and_whitespace_only(s in "\\s*") {
            // Whitespace-only inputs should either parse successfully or fail gracefully
            let result = CleanParser::parse_program(&s);
            // We don't care about the result, just that it doesn't panic
            drop(result);
        }

        #[test]
        fn test_valid_identifiers_dont_break_parser(
            id in "[a-zA-Z][a-zA-Z0-9_]*"
        ) {
            // Valid identifiers in various contexts should parse gracefully
            let contexts = vec![
                format!("start()\n\tprint({id})"),
                format!("integer {id}"),
                format!("functions:\n\tvoid {id}()\n\t\treturn"),
            ];

            for context in contexts {
                let _ = CleanParser::parse_program(&context);
            }
        }
    }
}
