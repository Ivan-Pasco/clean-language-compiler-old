//! The pest fragment parser (`CleanParser`, used for plugin-emitted
//! fragments) must report what it cannot parse instead of substituting a
//! default: a variable named after the raw interpolation text, a 32-bit
//! width, an `any` type, a `string` parameter type, or a silently dropped
//! function.

use clean_language_compiler::parser::grammar::{
    CleanParser, SingleExpressionParse, SingleStatementParse,
};

/// Interpolations are parsed as full expressions; the old fallback (a
/// variable named after the raw text) is gone, and real interpolations keep
/// their expression tree.
#[test]
fn interpolation_keeps_its_expression() {
    match CleanParser::parse_single_expression("\"value {a.b + 1}\"") {
        SingleExpressionParse::Expression(e) => {
            let text = format!("{:?}", e);
            assert!(text.contains("Binary"), "{}", text);
            assert!(!text.contains("Variable(\"a.b + 1\")"), "{}", text);
        }
        SingleExpressionParse::ParseError { message, .. } => panic!("must parse: {}", message),
        SingleExpressionParse::StatementNotExpression => panic!("must parse as an expression"),
    }
}

#[test]
fn invalid_precision_width_is_an_error() {
    for src in [
        "integer:7 x = 1\n",
        "integer:abc x = 1\n",
        "number:16 x = 1.0\n",
    ] {
        assert!(
            matches!(
                CleanParser::parse_single_statement(src),
                SingleStatementParse::ParseError { .. }
            ),
            "`{}` must not parse",
            src
        );
    }
    assert!(matches!(
        CleanParser::parse_single_statement("integer:16u x = 1\n"),
        SingleStatementParse::Statement(_)
    ));
}

#[test]
fn untyped_constructor_parameter_is_an_error() {
    let src = "class Person\n\tstring name\n\n\tconstructor(name)\n\t\tthis.name = name\n";
    assert!(
        CleanParser::parse_program(src).is_err(),
        "an untyped constructor parameter must not default to string"
    );
}
