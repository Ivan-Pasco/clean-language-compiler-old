//! frame-data.ebnf §8 `typed_raw_query`: `db.queryAs(Type):` followed by an
//! indented `sql:` / `params:` body is an expression block, like the untyped
//! `db.query:` form. The call form used to be rejected with
//! "Unsupported statement type: Colon".

use clean_language_compiler::ast::{Expression, Statement};
use clean_language_compiler::lexer::specification_lexer::{SourceCode, SpecificationLexer};
use clean_language_compiler::parser::SpecificationParser;

fn start_body(source: &str) -> Vec<Statement> {
    let code = SourceCode::new(source.to_string(), "raw_query.cln".to_string());
    let tokens = SpecificationLexer::new(&code).tokenize().expect("lexes");
    let mut parser = SpecificationParser::new(tokens, "raw_query.cln".to_string());
    let program = parser.parse_program().expect("parses");
    program
        .start_function
        .map(|f| f.body)
        .or_else(|| {
            program
                .functions
                .into_iter()
                .find(|f| f.name == "start")
                .map(|f| f.body)
        })
        .expect("start: body")
}

fn query_of(stmt: &Statement) -> (String, String, String) {
    match stmt {
        Statement::VariableDecl {
            initializer:
                Some(Expression::OrmQuery {
                    model,
                    verb,
                    content,
                    ..
                }),
            ..
        } => (model.clone(), verb.clone(), content.clone()),
        other => panic!("expected a query block initializer, got {:?}", other),
    }
}

#[test]
fn typed_and_untyped_raw_query_blocks_parse() {
    let body = start_body(
        "start:\n\tlist<Item> typed = db.queryAs(Item):\n\t\tsql: \"SELECT * FROM items\"\n\t\tparams: []\n\tstring r = db.query:\n\t\tsql: \"SELECT 1\"\n\t\tparams: []\n\tprintl(r)\n",
    );
    let (model, verb, content) = query_of(&body[0]);
    assert_eq!(model, "db");
    assert_eq!(verb, "queryAs(Item)");
    assert!(
        content.contains("sql: \"SELECT * FROM items\""),
        "{}",
        content
    );
    let (model, verb, _) = query_of(&body[1]);
    assert_eq!((model.as_str(), verb.as_str()), ("db", "query"));
    assert_eq!(body.len(), 3, "the statement after the blocks is kept");
}
