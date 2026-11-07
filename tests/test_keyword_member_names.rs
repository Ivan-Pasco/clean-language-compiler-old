//! grammar.ebnf `member_name`: after `.` any keyword is an ordinary name, in
//! source files and in plugin fragments (`_emit_stmt_from_source`), while
//! keywords stay reserved as plain identifiers.

use clean_language_compiler::ast::{AssignmentTarget, Expression, Statement};
use clean_language_compiler::lexer::specification_lexer::{SourceCode, SpecificationLexer};
use clean_language_compiler::parser::grammar::{CleanParser, SingleStatementParse};
use clean_language_compiler::parser::SpecificationParser;

fn parse(source: &str) -> Result<Vec<Statement>, String> {
    let code = SourceCode::new(source.to_string(), "kw.cln".to_string());
    let tokens = SpecificationLexer::new(&code)
        .tokenize()
        .map_err(|e| format!("{:?}", e))?;
    let mut parser = SpecificationParser::new(tokens);
    let program = parser.parse_program().map_err(|e| e.to_string())?;
    Ok(program
        .start_function
        .map(|f| f.body)
        .or_else(|| {
            program
                .functions
                .into_iter()
                .find(|f| f.name == "start")
                .map(|f| f.body)
        })
        .unwrap_or_default())
}

#[test]
fn keywords_are_member_and_method_names_after_dot() {
    let body = parse(
        "start:\n\tstring t = auth.tenant.require()\n\tany k = item.type\n\tany s = job.state\n\tcounter.start()\n\tjob.step = 2\n",
    )
    .expect("keyword member names parse");
    assert_eq!(body.len(), 5);
    match &body[4] {
        Statement::Assignment {
            target: AssignmentTarget::Property { object, path },
            ..
        } => assert_eq!(
            (object.as_str(), path.as_slice()),
            ("job", &["step".to_string()][..])
        ),
        other => panic!("expected a property assignment, got {:?}", other),
    }
    match &body[0] {
        Statement::VariableDecl {
            initializer: Some(Expression::MethodCall { method, .. }),
            ..
        } => assert_eq!(method, "require"),
        other => panic!("expected a method call initializer, got {:?}", other),
    }
}

#[test]
fn keywords_are_member_names_in_plugin_fragments() {
    for fragment in [
        "string tid = auth.tenant.require()",
        "integer n = obj.require(1) + obj.state.length()",
    ] {
        assert!(
            matches!(
                CleanParser::parse_single_statement(fragment),
                SingleStatementParse::Statement(_)
            ),
            "fragment `{}` must parse",
            fragment
        );
    }
}

#[test]
fn keywords_stay_reserved_as_plain_identifiers() {
    assert!(parse("start:\n\tinteger require = 1\n").is_err());
    assert!(parse("start:\n\tinteger while = 1\n").is_err());
}
