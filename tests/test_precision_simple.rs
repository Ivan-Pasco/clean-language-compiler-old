#[cfg(test)]
mod test {
    use clean_language_compiler::lexer::specification_lexer::{SourceCode, SpecificationLexer};
    use clean_language_compiler::lexer::specification_token::TokenKind;

    #[test]
    fn test_simple_precision() {
        let source = SourceCode::new("42:8".to_string(), "test.cln".to_string());
        let mut lexer = SpecificationLexer::new(&source);

        let result = lexer.tokenize();
        assert!(result.is_ok(), "Tokenization failed: {:?}", result.err());

        let tokens = result.unwrap();
        println!("Tokens: {:?}", tokens.tokens);

        // Should have Integer8Literal token
        assert!(tokens
            .tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::Integer8Literal(_))));
    }
}
