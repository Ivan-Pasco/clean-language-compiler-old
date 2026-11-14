//! Plugin block header attributes (`html var="card":`).
//!
//! A plugin declares the `name="string"` attributes a block accepts before
//! its colon in `[blocks.<name>].attributes`; the parser passes them to the
//! plugin as block attributes. Ivan-Pasco/cleen-framework-old#26.

#![cfg(test)]

use clean_language_compiler::ast::{FrameworkAttribute, Statement};
use clean_language_compiler::error::CompilerError;
use clean_language_compiler::lexer::specification_lexer::{SourceCode, SpecificationLexer};
use clean_language_compiler::parser::SpecificationParser;
use std::collections::HashMap;

fn parse(source: &str) -> Result<clean_language_compiler::ast::Program, CompilerError> {
    let source_code = SourceCode::new(source.to_string(), "test.cln".to_string());
    let tokens = SpecificationLexer::new(&source_code)
        .tokenize()
        .expect("source should tokenize");
    let mut parser = SpecificationParser::with_plugin_keywords(
        tokens,
        vec!["html".to_string(), "page".to_string()],
    );
    let mut declared = HashMap::new();
    declared.insert("html".to_string(), vec!["var".to_string()]);
    parser.set_block_header_attributes(declared);
    parser.parse_program()
}

fn blocks(statements: &[Statement]) -> Vec<(String, String, Vec<FrameworkAttribute>)> {
    statements
        .iter()
        .filter_map(|stmt| match stmt {
            Statement::FrameworkBlock {
                name,
                content,
                attributes,
                ..
            } => Some((name.clone(), content.clone(), attributes.clone())),
            _ => None,
        })
        .collect()
}

fn attr(attributes: &[FrameworkAttribute], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|a| a.name == name)
        .and_then(|a| a.value.clone())
}

#[test]
fn header_attribute_in_start_block() {
    let program =
        parse("start:\n\thtml var=\"card\":\n\t\t<p>hi</p>\n\tprint(card)\n").expect("parses");
    let start = program.start_function.expect("start block");
    let found = blocks(&start.body);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "html");
    assert_eq!(found[0].1, "<p>hi</p>");
    assert_eq!(attr(&found[0].2, "var").as_deref(), Some("card"));
    assert_eq!(
        start.body.len(),
        2,
        "print(card) stays a statement after the block"
    );
}

#[test]
fn header_attribute_in_function_body() {
    let program = parse(
        "functions:\n\tstring card()\n\t\thtml var=\"out\":\n\t\t\t<b>x</b>\n\t\treturn out\n",
    )
    .expect("parses");
    let body = &program.functions[0].body;
    let found = blocks(body);
    assert_eq!(found.len(), 1);
    assert_eq!(attr(&found[0].2, "var").as_deref(), Some("out"));
    assert_eq!(found[0].1, "<b>x</b>");
    assert_eq!(body.len(), 2);
}

#[test]
fn header_attribute_at_top_level() {
    let program = parse("html var=\"page\":\n\t<p>top</p>\n").expect("parses");
    let found = blocks(&program.statements);
    assert_eq!(found.len(), 1);
    assert_eq!(attr(&found[0].2, "var").as_deref(), Some("page"));
    assert_eq!(found[0].1, "<p>top</p>");
}

#[test]
fn undeclared_header_attribute_is_syn002() {
    let error = parse("start:\n\thtml var=\"a\" id=\"b\":\n\t\t<p></p>\n").unwrap_err();
    let text = format!("{}", error);
    assert!(
        text.contains("`id` is not a header attribute of the `html` block"),
        "{}",
        text
    );
    assert_eq!(
        error.context().and_then(|c| c.error_code.as_deref()),
        Some("SYN002")
    );
}

/// A block keyword followed by a name keeps reading the rest of the header
/// line as body text (`page Home at "/":`), as before.
#[test]
fn plugin_declaration_header_unchanged() {
    let program = parse("page Home at \"/\":\n\t<p></p>\n").expect("parses");
    let found = blocks(&program.statements);
    assert_eq!(found.len(), 1);
    assert_eq!(attr(&found[0].2, "model_name").as_deref(), Some("Home"));
    assert!(found[0].1.starts_with("at \"/\":"), "{}", found[0].1);
}

#[test]
fn header_attribute_needs_a_string() {
    let error = parse("start:\n\thtml var=card:\n\t\t<p></p>\n").unwrap_err();
    let text = format!("{}", error);
    assert!(text.contains("needs a string value"), "{}", text);
}

/// Text after the colon stays in the body for the plugin to read, and the
/// body ends where the block's indentation ends: the statements after a
/// statement-level block are not swallowed into it.
#[test]
fn header_text_after_colon_keeps_following_statements() {
    let program =
        parse("start:\n\thtml: var=\"card\"\n\t\t<p>hi</p>\n\tprint(\"end\")\n").expect("parses");
    let start = program.start_function.expect("start block");
    let found = blocks(&start.body);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].1, "var=\"card\"\n<p>hi</p>");
    assert!(found[0].2.is_empty());
    assert_eq!(
        start.body.len(),
        2,
        "print(\"end\") stays a statement after the block"
    );
}
