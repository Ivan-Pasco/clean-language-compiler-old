//! Framework block `@attribute` prefixes (`@auth`, `@version("v2")`,
//! `@cache(ttl: 300)`).
//!
//! Grammar: foundation/spec/grammar/grammar.ebnf `framework_attribute`,
//! `framework_block`. Spec: 22-plugin-system.md "Block Attributes". The parser
//! collects attributes generically and hands them to the plugin; it never
//! interprets a name such as `auth` itself.

#![cfg(test)]

use clean_language_compiler::ast::{FrameworkAttribute, Program, Statement};
use clean_language_compiler::error::CompilerError;
use clean_language_compiler::lexer::specification_lexer::{SourceCode, SpecificationLexer};
use clean_language_compiler::parser::SpecificationParser;

fn parse(source: &str) -> Result<Program, CompilerError> {
    let source_code = SourceCode::new(source.to_string(), "test.cln".to_string());
    let tokens = SpecificationLexer::new(&source_code)
        .tokenize()
        .expect("source should tokenize");
    let mut parser = SpecificationParser::with_plugin_keywords(
        tokens,
        vec!["html".to_string(), "page".to_string()],
    );
    parser.parse_program()
}

fn blocks(statements: &[Statement]) -> Vec<(String, Vec<FrameworkAttribute>)> {
    statements
        .iter()
        .filter_map(|stmt| match stmt {
            Statement::FrameworkBlock {
                name, attributes, ..
            } => Some((name.clone(), attributes.clone())),
            _ => None,
        })
        .collect()
}

fn pairs(attributes: &[FrameworkAttribute]) -> Vec<(String, Option<String>)> {
    attributes
        .iter()
        .map(|a| (a.name.clone(), a.value.clone()))
        .collect()
}

#[test]
fn bare_attribute_before_top_level_block() {
    let program = parse(
        "@auth\nendpoints:\n\tGET \"/users\":\n\t\treturn json(\"x\")\n\nstart:\n\tprintl(\"ok\")\n",
    )
    .expect("parses");
    let found = blocks(&program.statements);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "endpoints");
    assert_eq!(pairs(&found[0].1), vec![("auth".to_string(), None)]);
    assert!(program.start_function.is_some());
}

#[test]
fn attribute_with_arguments_keeps_source_text() {
    let program = parse(
        "@version(\"v2\")\n@cache(ttl: 300)\nendpoints:\n\tGET \"/a\":\n\t\treturn json(\"x\")\n",
    )
    .expect("parses");
    let found = blocks(&program.statements);
    assert_eq!(found.len(), 1);
    assert_eq!(
        pairs(&found[0].1),
        vec![
            ("version".to_string(), Some("\"v2\"".to_string())),
            ("cache".to_string(), Some("ttl: 300".to_string())),
        ]
    );
}

#[test]
fn attribute_before_block_with_argument() {
    let program = parse("@auth\ndata User:\n\tstring name\n").expect("parses");
    let found = blocks(&program.statements);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "data");
    let attrs = pairs(&found[0].1);
    assert!(attrs.contains(&("auth".to_string(), None)));
}

#[test]
fn attribute_before_plugin_keyword_block() {
    let program = parse("@auth\nhtml:\n\t<p>hi</p>\n").expect("parses");
    let found = blocks(&program.statements);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "html");
    assert_eq!(pairs(&found[0].1), vec![("auth".to_string(), None)]);
}

#[test]
fn attribute_before_block_inside_start() {
    let program =
        parse("start:\n\t@auth\n\thtml:\n\t\t<p>hi</p>\n\tprintl(\"x\")\n").expect("parses");
    let start = program.start_function.expect("start block");
    let found = blocks(&start.body);
    assert_eq!(found.len(), 1);
    assert_eq!(pairs(&found[0].1), vec![("auth".to_string(), None)]);
    assert_eq!(start.body.len(), 2);
}

#[test]
fn attribute_not_followed_by_block_is_an_error() {
    let err = parse("@auth\nstart:\n\tprintl(\"x\")\n");
    assert!(err.is_err(), "an attribute must precede a framework block");
}
