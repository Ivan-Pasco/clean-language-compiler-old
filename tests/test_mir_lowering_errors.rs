//! MIR lowering errors reach the caller.
//!
//! A function whose body fails to lower used to have its errors stored as
//! "warnings": only Type and Validation errors were returned, every other
//! error was dropped together with the function, and the build went on to
//! fail later with an unrelated "function not found" in code generation.
//! Every lowering error is now returned from `MirBuilder::build`.

use clean_language_compiler::ast::SourceLocation;
use clean_language_compiler::mir::MirBuilder;
use clean_language_compiler::resolver::{GlobalSymbolTable, ScopeId, SymbolId};
use clean_language_compiler::typechecker::tast::{
    ConcreteType, TastBlock, TastExpression, TastExpressionKind, TastFunction, TastLiteral,
    TastProgram, TastStatement, Visibility,
};
use std::collections::HashMap;
use std::sync::Arc;

fn location(line: usize) -> SourceLocation {
    SourceLocation {
        file: "lowering.cln".to_string(),
        line,
        column: 1,
        byte_start: None,
        byte_end: None,
    }
}

fn function(symbol: usize, name: &str, statements: Vec<TastStatement>) -> TastFunction {
    TastFunction {
        symbol_id: SymbolId(symbol),
        name: name.to_string(),
        parameters: Vec::new(),
        return_type: ConcreteType::Undefined,
        body: TastBlock {
            statements,
            scope_id: ScopeId(0),
            return_type: ConcreteType::Undefined,
            location: location(1),
        },
        generic_params: Vec::new(),
        constraints: Vec::new(),
        is_background: false,
        is_static: false,
        visibility: Visibility::Public,
        location: location(1),
    }
}

#[test]
fn a_codegen_error_while_lowering_a_function_fails_the_build() {
    // `thing.draw()` on a capability that has no `draw` method: the MIR
    // builder reports a Codegen error (the type checker normally rejects
    // this first).
    let receiver = TastExpression {
        kind: TastExpressionKind::Literal {
            value: TastLiteral::Null,
        },
        expr_type: ConcreteType::Interface {
            symbol_id: SymbolId(900),
            type_args: Vec::new(),
        },
        location: location(3),
    };
    let call = TastExpression {
        kind: TastExpressionKind::MethodCall {
            receiver: Box::new(receiver),
            method_name: "draw".to_string(),
            method_symbol: SymbolId(0),
            arguments: Vec::new(),
            type_args: Vec::new(),
        },
        expr_type: ConcreteType::Undefined,
        location: location(3),
    };
    let broken = function(
        800,
        "broken",
        vec![TastStatement::Expression {
            expression: call,
            location: location(3),
        }],
    );
    let program = TastProgram {
        functions: vec![function(801, "fine", Vec::new()), broken],
        classes: Vec::new(),
        start_function: None,
        tests: Vec::new(),
        state: None,
        watch_blocks: Vec::new(),
        type_env: HashMap::new(),
        location: location(1),
        symbol_table: Arc::new(GlobalSymbolTable::new()),
        externals: Vec::new(),
        vtable_descriptors: HashMap::new(),
    };

    let mut builder = MirBuilder::new(program.symbol_table.clone());
    let errors = builder
        .build(program)
        .expect_err("a function that fails to lower must fail the build");
    assert!(
        errors
            .iter()
            .any(|e| e.to_string().contains("has no method 'draw'")),
        "the lowering error is reported, got: {:?}",
        errors
    );
}
