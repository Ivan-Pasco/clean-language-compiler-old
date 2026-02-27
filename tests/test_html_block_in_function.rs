//! Tests for html: (plugin block) keywords inside function bodies
//!
//! Verifies that plugin block keywords like `html:` are correctly parsed
//! inside function bodies, producing FrameworkBlock AST nodes with raw
//! HTML content preserved via byte-position extraction.

#![cfg(test)]

use clean_language_compiler::ast::Statement;
use clean_language_compiler::lexer::specification_lexer::{SourceCode, SpecificationLexer};
use clean_language_compiler::parser::SpecificationParser;

/// Helper: find FrameworkBlock statements inside a function's body.
/// Returns empty vec if parsing fails (e.g., when plugin keywords are not registered).
fn find_framework_blocks_in_functions(
    source: &str,
    keywords: Vec<String>,
) -> Vec<(String, String)> {
    let source_code = SourceCode::new(source.to_string(), "test.cln".to_string());
    let mut lexer = SpecificationLexer::new(&source_code);
    let tokens = lexer
        .tokenize()
        .expect("Lexer should tokenize HTML content without errors");

    let mut parser =
        SpecificationParser::with_plugin_keywords(tokens, "test.cln".to_string(), keywords);
    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let mut blocks = Vec::new();

    // Check function bodies for FrameworkBlock statements
    for func in &program.functions {
        for stmt in &func.body {
            if let Statement::FrameworkBlock { name, content, .. } = stmt {
                blocks.push((name.clone(), content.clone()));
            }
        }
    }

    // Also check top-level statements
    for stmt in &program.statements {
        if let Statement::FrameworkBlock { name, content, .. } = stmt {
            blocks.push((name.clone(), content.clone()));
        }
    }

    blocks
}

// ─── Test 1: Basic html: block in function body ─────────────────────────

#[test]
fn test_html_block_in_function_basic() {
    let source = "functions:\n\tstring test_basic()\n\t\thtml:\n\t\t\t<div class=\"card\">\n\t\t\t\t<h1>Hello</h1>\n\t\t\t</div>\n";

    let blocks = find_framework_blocks_in_functions(source, vec!["html".to_string()]);

    assert_eq!(blocks.len(), 1, "Should find exactly one html: block");
    assert_eq!(blocks[0].0, "html", "Block name should be 'html'");

    let content = &blocks[0].1;
    assert!(
        content.contains("<div"),
        "Content should contain raw '<div': got '{}'",
        content
    );
    assert!(
        content.contains("</div>"),
        "Content should contain raw '</div>': got '{}'",
        content
    );
    assert!(
        content.contains("<h1>Hello</h1>"),
        "Content should contain '<h1>Hello</h1>': got '{}'",
        content
    );
}

// ─── Test 2: html: block with variable interpolation syntax ─────────────

#[test]
fn test_html_block_with_interpolation() {
    let source =
        "functions:\n\tstring test_interp(string name)\n\t\thtml:\n\t\t\t<p>Hello {name}!</p>\n";

    let blocks = find_framework_blocks_in_functions(source, vec!["html".to_string()]);

    assert_eq!(blocks.len(), 1);
    let content = &blocks[0].1;
    assert!(
        content.contains("{name}"),
        "Content should preserve interpolation syntax: got '{}'",
        content
    );
    assert!(
        content.contains("<p>"),
        "Content should contain raw '<p>': got '{}'",
        content
    );
}

// ─── Test 3: html: block with raw (unescaped) interpolation ─────────────

#[test]
fn test_html_block_with_raw_interpolation() {
    let source =
        "functions:\n\tstring test_raw(string nav_html)\n\t\thtml:\n\t\t\t<nav>{!nav_html}</nav>\n";

    let blocks = find_framework_blocks_in_functions(source, vec!["html".to_string()]);

    assert_eq!(blocks.len(), 1);
    let content = &blocks[0].1;
    assert!(
        content.contains("{!nav_html}"),
        "Content should preserve raw interpolation: got '{}'",
        content
    );
}

// ─── Test 5: Top-level component blocks still work (regression) ─────────

#[test]
fn test_top_level_component_block_regression() {
    let source =
        "component:\n\thtml:\n\t\t<div class=\"card\">\n\t\t\t<h1>Still works</h1>\n\t\t</div>\n";

    let blocks = find_framework_blocks_in_functions(
        source,
        vec!["html".to_string(), "component".to_string()],
    );

    // component: is a top-level framework block
    assert!(
        blocks.iter().any(|(name, _)| name == "component"),
        "Should find component: framework block"
    );
}

// ─── Test: Lexer handles HTML characters without errors ──────────────────

#[test]
fn test_lexer_handles_html_characters() {
    // Verify the lexer can tokenize HTML content (even though tokens are weird)
    let source = "<div class=\"card\">\n\t<h1>Hello</h1>\n\t<p>Test</p>\n</div>";
    let source_code = SourceCode::new(source.to_string(), "test.cln".to_string());
    let mut lexer = SpecificationLexer::new(&source_code);

    let result = lexer.tokenize();
    assert!(
        result.is_ok(),
        "Lexer should tokenize HTML content without errors: {:?}",
        result.err()
    );
}

// ─── Test: html: block requires plugin keyword registration ──────────────

#[test]
fn test_html_block_requires_plugin_keyword() {
    // Without plugin keywords, html: inside a function body is NOT recognized
    // as a framework block. Plugin keywords must be registered via plugins: block.
    // This enforces separation of concerns — HTML knowledge lives in plugins only.
    let source = "functions:\n\tstring test()\n\t\thtml:\n\t\t\t<div>Hello</div>\n";

    let blocks = find_framework_blocks_in_functions(source, Vec::new());

    assert_eq!(
        blocks.len(),
        0,
        "html: without plugin keywords should NOT be detected as framework block"
    );
}

// ─── Test: Multi-line HTML with various tags ─────────────────────────────

#[test]
fn test_html_block_multiline_complex() {
    let source = "functions:\n\tstring render()\n\t\thtml:\n\t\t\t<div class=\"container\">\n\t\t\t\t<header>\n\t\t\t\t\t<h1>Title</h1>\n\t\t\t\t</header>\n\t\t\t\t<main>\n\t\t\t\t\t<p>Content</p>\n\t\t\t\t</main>\n\t\t\t</div>\n";

    let blocks = find_framework_blocks_in_functions(source, vec!["html".to_string()]);

    assert_eq!(blocks.len(), 1);
    let content = &blocks[0].1;
    assert!(content.contains("<header>"), "Should contain <header>");
    assert!(content.contains("</header>"), "Should contain </header>");
    assert!(content.contains("<main>"), "Should contain <main>");
    assert!(content.contains("</main>"), "Should contain </main>");
    assert!(
        content.contains("<div class=\"container\">"),
        "Should contain div with class"
    );
}
