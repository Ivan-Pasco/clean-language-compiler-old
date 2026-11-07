//! Endpoint tests (`tests:` block, grammar.ebnf `endpoint_test`) lower to a
//! `_test_http_request_clean` call. The request carries its `json(...)` body
//! and `header(...)`: `_test_http_request` (function-registry.toml) takes
//! method, path, body, header key and header value. Before compiler 0.33.174
//! the body and header were always sent as "".

use clean_language_compiler::ast::Value;
use clean_language_compiler::hir::hir_builder::HirBuilder;
use clean_language_compiler::hir::{HirExpression, HirStatement};
use clean_language_compiler::parse_to_ast;

/// The arguments of the `_test_http_request_clean` call the program's first
/// endpoint test lowers to.
fn request_arguments(source: &str) -> Vec<HirExpression> {
    let ast = parse_to_ast(source, "endpoint_test.cln").expect("parse");
    let hir = HirBuilder::new().build_hir(ast).expect("hir").hir;
    let test = hir.tests.first().expect("one endpoint test");
    match &test.body.statements[0] {
        HirStatement::VariableDeclaration {
            initializer:
                Some(HirExpression::Call {
                    function,
                    arguments,
                    ..
                }),
            ..
        } => {
            assert_eq!(function, "_test_http_request_clean");
            arguments.clone()
        }
        other => panic!("expected the request handle declaration, got {other:?}"),
    }
}

fn string_literal(expr: &HirExpression) -> Option<&str> {
    match expr {
        HirExpression::Literal {
            value: Value::String(s),
            ..
        } => Some(s),
        _ => None,
    }
}

#[test]
fn body_and_header_are_passed_to_the_request() {
    let args = request_arguments(
        "start:\n\tprint(\"x\")\n\ntests:\n\ttest \"creates\"\n\t\tPOST \"/api/users\" json(name: \"Alice\") header(\"Authorization\": \"Bearer t\")\n\t\tstatus = 201\n",
    );
    assert_eq!(args.len(), 5);
    assert_eq!(string_literal(&args[0]), Some("POST"));
    assert_eq!(string_literal(&args[1]), Some("/api/users"));
    match &args[2] {
        HirExpression::MethodCall {
            method, arguments, ..
        } => {
            assert_eq!(method, "encode");
            assert!(matches!(arguments[0], HirExpression::ObjectLiteral { .. }));
        }
        other => panic!("body should be json.encode of the fields, got {other:?}"),
    }
    assert_eq!(string_literal(&args[3]), Some("Authorization"));
    assert_eq!(string_literal(&args[4]), Some("Bearer t"));
}

#[test]
fn request_without_body_or_header_sends_empty_strings() {
    let args = request_arguments(
        "start:\n\tprint(\"x\")\n\ntests:\n\ttest \"health\"\n\t\tGET \"/api/health\"\n\t\tstatus = 200\n",
    );
    assert_eq!(args.len(), 5);
    assert_eq!(string_literal(&args[2]), Some(""));
    assert_eq!(string_literal(&args[3]), Some(""));
    assert_eq!(string_literal(&args[4]), Some(""));
}
