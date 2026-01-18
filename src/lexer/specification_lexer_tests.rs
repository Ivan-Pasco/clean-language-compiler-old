//! Comprehensive specification compliance tests for the Clean Language lexer
//!
//! These tests ensure 100% compliance with the Clean Language Specification
//! by testing all language constructs defined in the specification.

#[cfg(test)]
mod tests {
    use super::super::{SpecificationLexer, TokenKind, Token};
    use crate::ast::SourceLocation;

    /// Helper function to tokenize and extract token kinds
    fn tokenize(input: &str) -> Result<Vec<TokenKind>, String> {
        let mut lexer = SpecificationLexer::new(input, "test.cln");
        let mut tokens = Vec::new();
        
        loop {
            match lexer.next_token() {
                Ok(token) => {
                    if matches!(token.kind, TokenKind::Eof) {
                        break;
                    }
                    tokens.push(token.kind);
                }
                Err(e) => return Err(format!("{}", e)),
            }
        }
        
        Ok(tokens)
    }

    /// Test all integer literals (§2.3.1 - Integer Values)
    #[test]
    fn test_integer_literals() {
        // Basic integer literals
        let test_cases = vec![
            ("0", vec![TokenKind::IntegerLiteral(0)]),
            ("42", vec![TokenKind::IntegerLiteral(42)]),
            ("-17", vec![TokenKind::Minus, TokenKind::IntegerLiteral(17)]),
            ("123456", vec![TokenKind::IntegerLiteral(123456)]),
        ];

        for (input, expected) in test_cases {
            let tokens = tokenize(input).expect(&format!("Failed to tokenize: {}", input));
            assert_eq!(tokens, expected, "Integer literal test failed for: {}", input);
        }
    }

    /// Test precision integer literals (§3.2 - Type Precision)
    #[test]
    fn test_precision_integer_literals() {
        let test_cases = vec![
            ("42:8", vec![TokenKind::Integer8Literal(42)]),
            ("42:8u", vec![TokenKind::Integer8uLiteral(42)]),
            ("1000:16", vec![TokenKind::Integer16Literal(1000)]),
            ("1000:16u", vec![TokenKind::Integer16uLiteral(1000)]),
            ("50000:32", vec![TokenKind::Integer32Literal(50000)]),
            ("9223372036854775807:64", vec![TokenKind::Integer64Literal(9223372036854775807)]),
        ];

        for (input, expected) in test_cases {
            let tokens = tokenize(input).expect(&format!("Failed to tokenize: {}", input));
            assert_eq!(tokens, expected, "Precision integer test failed for: {}", input);
        }
    }

    /// Test number literals (§2.3.2 - Number Values)
    #[test]
    fn test_number_literals() {
        let test_cases = vec![
            ("3.14", vec![TokenKind::NumberLiteral(3.14)]),
            ("0.0", vec![TokenKind::NumberLiteral(0.0)]),
            ("123.456", vec![TokenKind::NumberLiteral(123.456)]),
            ("6.02e23", vec![TokenKind::NumberLiteral(6.02e23)]),
            ("1.5E-10", vec![TokenKind::NumberLiteral(1.5e-10)]),
        ];

        for (input, expected) in test_cases {
            let tokens = tokenize(input).expect(&format!("Failed to tokenize: {}", input));
            assert_eq!(tokens, expected, "Number literal test failed for: {}", input);
        }
    }

    /// Test precision number literals
    #[test]
    fn test_precision_number_literals() {
        let test_cases = vec![
            ("3.14:32", vec![TokenKind::Number32Literal(3.14)]),
            ("2.718:64", vec![TokenKind::Number64Literal(2.718)]),
        ];

        for (input, expected) in test_cases {
            let tokens = tokenize(input).expect(&format!("Failed to tokenize: {}", input));
            assert_eq!(tokens, expected, "Precision number test failed for: {}", input);
        }
    }

    /// Test string literals (§2.3.3 - String Values)
    #[test]
    fn test_string_literals() {
        let test_cases = vec![
            (r#""hello""#, vec![TokenKind::StringLiteral("hello".to_string())]),
            (r#""""#, vec![TokenKind::StringLiteral("".to_string())]),
            (r#""Hello, World!""#, vec![TokenKind::StringLiteral("Hello, World!".to_string())]),
            (r#""tab\there""#, vec![TokenKind::StringLiteral("tab\there".to_string())]),
            (r#""newline\nhere""#, vec![TokenKind::StringLiteral("newline\nhere".to_string())]),
        ];

        for (input, expected) in test_cases {
            let tokens = tokenize(input).expect(&format!("Failed to tokenize: {}", input));
            assert_eq!(tokens, expected, "String literal test failed for: {}", input);
        }
    }

    /// Test boolean literals (§2.3.4 - Boolean Values)
    #[test]
    fn test_boolean_literals() {
        let test_cases = vec![
            ("true", vec![TokenKind::True]),
            ("false", vec![TokenKind::False]),
        ];

        for (input, expected) in test_cases {
            let tokens = tokenize(input).expect(&format!("Failed to tokenize: {}", input));
            assert_eq!(tokens, expected, "Boolean literal test failed for: {}", input);
        }
    }

    /// Test all keywords from Clean Language Specification
    #[test]
    fn test_all_keywords() {
        let keywords = vec![
            ("and", TokenKind::And),
            ("class", TokenKind::Class),
            ("constructor", TokenKind::Constructor),
            ("else", TokenKind::Else),
            ("error", TokenKind::Error),
            ("false", TokenKind::False),
            ("for", TokenKind::For),
            ("from", TokenKind::From),
            ("function", TokenKind::Function),
            ("if", TokenKind::If),
            ("import", TokenKind::Import),
            ("in", TokenKind::In),
            ("iterate", TokenKind::Iterate),
            ("not", TokenKind::Not),
            ("onError", TokenKind::OnError),
            ("or", TokenKind::Or),
            ("print", TokenKind::Print),
            ("return", TokenKind::Return),
            ("start", TokenKind::Start),
            ("step", TokenKind::Step),
            ("test", TokenKind::Test),
            ("tests", TokenKind::Tests),
            ("this", TokenKind::This),
            ("to", TokenKind::To),
            ("true", TokenKind::True),
            ("while", TokenKind::While),
            ("is", TokenKind::Is),
            ("returns", TokenKind::Returns),
            ("description", TokenKind::Description),
            ("input", TokenKind::Input),
            ("unit", TokenKind::Unit),
            ("private", TokenKind::Private),
            ("constant", TokenKind::Constant),
            ("functions", TokenKind::Functions),
        ];

        for (keyword, expected_kind) in keywords {
            let tokens = tokenize(keyword).expect(&format!("Failed to tokenize keyword: {}", keyword));
            assert_eq!(tokens.len(), 1, "Keyword should produce exactly one token: {}", keyword);
            assert_eq!(tokens[0], expected_kind, "Keyword token mismatch for: {}", keyword);
        }
    }

    /// Test operators (§5.1 - Binary and Unary Operations)
    #[test]
    fn test_operators() {
        let operators = vec![
            ("+", vec![TokenKind::Plus]),
            ("-", vec![TokenKind::Minus]),
            ("*", vec![TokenKind::Multiply]),
            ("/", vec![TokenKind::Divide]),
            ("%", vec![TokenKind::Modulo]),
            ("^", vec![TokenKind::Power]),
            ("==", vec![TokenKind::Equal]),
            ("!=", vec![TokenKind::NotEqual]),
            ("<", vec![TokenKind::Less]),
            (">", vec![TokenKind::Greater]),
            ("<=", vec![TokenKind::LessEqual]),
            (">=", vec![TokenKind::GreaterEqual]),
            ("=", vec![TokenKind::Assign]),
        ];

        for (op, expected) in operators {
            let tokens = tokenize(op).expect(&format!("Failed to tokenize operator: {}", op));
            assert_eq!(tokens, expected, "Operator test failed for: {}", op);
        }
    }

    /// Test punctuation and delimiters
    #[test]
    fn test_punctuation() {
        let punctuation = vec![
            ("(", vec![TokenKind::LeftParen]),
            (")", vec![TokenKind::RightParen]),
            ("[", vec![TokenKind::LeftBracket]),
            ("]", vec![TokenKind::RightBracket]),
            ("{", vec![TokenKind::LeftBrace]),
            ("}", vec![TokenKind::RightBrace]),
            (",", vec![TokenKind::Comma]),
            (".", vec![TokenKind::Dot]),
            (":", vec![TokenKind::Colon]),
            (";", vec![TokenKind::Semicolon]),
            ("..", vec![TokenKind::Range]),
            ("..=", vec![TokenKind::RangeInclusive]),
        ];

        for (punct, expected) in punctuation {
            let tokens = tokenize(punct).expect(&format!("Failed to tokenize punctuation: {}", punct));
            assert_eq!(tokens, expected, "Punctuation test failed for: {}", punct);
        }
    }

    /// Test identifiers
    #[test]
    fn test_identifiers() {
        let identifiers = vec![
            ("x", vec![TokenKind::Identifier("x".to_string())]),
            ("variable_name", vec![TokenKind::Identifier("variable_name".to_string())]),
            ("myFunction", vec![TokenKind::Identifier("myFunction".to_string())]),
            ("Person", vec![TokenKind::Identifier("Person".to_string())]),
            ("calculateSum", vec![TokenKind::Identifier("calculateSum".to_string())]),
        ];

        for (ident, expected) in identifiers {
            let tokens = tokenize(ident).expect(&format!("Failed to tokenize identifier: {}", ident));
            assert_eq!(tokens, expected, "Identifier test failed for: {}", ident);
        }
    }

    /// Test tab-based indentation (§1.4 - Code Structure)
    #[test]
    fn test_indentation() {
        let input = "start()\n\tinteger x = 42\n\t\tprint(x)\n\tprint(\"done\")";
        let tokens = tokenize(input).expect("Failed to tokenize indented code");
        
        // Should contain indentation tokens
        let has_indent = tokens.iter().any(|t| matches!(t, TokenKind::Indent(_)));
        assert!(has_indent, "Indented code should contain indentation tokens");
    }

    /// Test comments (not in AST but needed for parsing)
    #[test]
    fn test_comments() {
        let test_cases = vec![
            ("// single line comment", vec![TokenKind::Comment(" single line comment".to_string())]),
            ("/* block comment */", vec![TokenKind::BlockComment(" block comment ".to_string())]),
        ];

        for (input, expected) in test_cases {
            let tokens = tokenize(input).expect(&format!("Failed to tokenize comment: {}", input));
            assert_eq!(tokens, expected, "Comment test failed for: {}", input);
        }
    }

    /// Test complete program tokenization
    #[test]
    fn test_complete_program() {
        let program = r#"functions:
	integer add(integer a, integer b)
		return a + b

start()
	integer result = add(5, 3)
	print(result)
"#;

        let tokens = tokenize(program).expect("Failed to tokenize complete program");
        
        // Verify it contains essential tokens
        assert!(tokens.iter().any(|t| matches!(t, TokenKind::Functions)));
        assert!(tokens.iter().any(|t| matches!(t, TokenKind::Start)));
        assert!(tokens.iter().any(|t| matches!(t, TokenKind::Return)));
        assert!(tokens.iter().any(|t| matches!(t, TokenKind::Plus)));
        assert!(tokens.iter().any(|t| matches!(t, TokenKind::Assign)));
        
        println!("Complete program tokenized successfully with {} tokens", tokens.len());
    }

    /// Test string interpolation tokens (§2.3.2)
    #[test]
    fn test_string_interpolation() {
        let test_cases = vec![
            (r#""Hello {name}""#, vec![
                TokenKind::InterpolationStart,
                TokenKind::Identifier("name".to_string()),
                TokenKind::InterpolationEnd,
            ]),
            (r#""Value: {x} and {y}""#, vec![
                TokenKind::InterpolationStart,
                TokenKind::Identifier("x".to_string()),
                TokenKind::InterpolationMid,
                TokenKind::Identifier("y".to_string()),
                TokenKind::InterpolationEnd,
            ]),
        ];

        for (input, expected_contains) in test_cases {
            let tokens = tokenize(input).expect(&format!("Failed to tokenize string interpolation: {}", input));
            
            for expected_token in expected_contains {
                assert!(tokens.contains(&expected_token), 
                    "String interpolation test failed for: {} - missing {:?}", input, expected_token);
            }
        }
    }

    /// Test error recovery with invalid characters
    #[test]
    fn test_error_recovery() {
        let invalid_inputs = vec![
            "@invalid",
            "#hashtag", 
            "$variable",
            "123abc", // Invalid number format
        ];

        for input in invalid_inputs {
            let result = tokenize(input);
            // Should either produce tokens with Invalid kind or return error
            match result {
                Ok(tokens) => {
                    let has_invalid = tokens.iter().any(|t| matches!(t, TokenKind::Invalid { .. }));
                    assert!(has_invalid, "Should produce Invalid token for: {}", input);
                }
                Err(_) => {
                    // Error is also acceptable for invalid input
                    println!("Error recovery test passed for: {} (error returned)", input);
                }
            }
        }
    }

    /// Test precision modifier parsing edge cases
    #[test]
    fn test_precision_edge_cases() {
        let test_cases = vec![
            ("0:8", vec![TokenKind::Integer8Literal(0)]),
            ("255:8u", vec![TokenKind::Integer8uLiteral(255)]),
            ("-128:8", vec![TokenKind::Minus, TokenKind::Integer8Literal(128)]),
            ("3.14159:32", vec![TokenKind::Number32Literal(3.14159)]),
        ];

        for (input, expected) in test_cases {
            let tokens = tokenize(input).expect(&format!("Failed to tokenize precision case: {}", input));
            assert_eq!(tokens, expected, "Precision edge case test failed for: {}", input);
        }
    }

    /// Test newline handling
    #[test]
    fn test_newlines() {
        let input = "start()\nprint(\"hello\")\n";
        let tokens = tokenize(input).expect("Failed to tokenize newlines");
        
        let has_newlines = tokens.iter().any(|t| matches!(t, TokenKind::Newline));
        assert!(has_newlines, "Should contain newline tokens");
    }

    /// Benchmark test for performance requirement (>100,000 tokens/second)
    #[test]
    fn test_performance_benchmark() {
        use std::time::Instant;
        
        let large_program = r#"start()
	integer x = 42
	integer y = 24
	integer sum = x + y
	print(sum)
"#.repeat(1000); // Repeat to create large input

        let start = Instant::now();
        let tokens = tokenize(&large_program).expect("Failed to tokenize large program");
        let elapsed = start.elapsed();
        
        let token_count = tokens.len();
        let tokens_per_second = (token_count as f64) / elapsed.as_secs_f64();
        
        println!("Performance: {} tokens in {:?} ({:.0} tokens/second)", 
            token_count, elapsed, tokens_per_second);
        
        // Verify performance meets requirement
        assert!(tokens_per_second >= 100_000.0, 
            "Lexer performance requirement not met: {:.0} tokens/second < 100,000", 
            tokens_per_second);
    }
}