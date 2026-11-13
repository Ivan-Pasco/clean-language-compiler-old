//! CLASS008: `ClassName.method()` only reaches methods that need no object.
//!
//! A method needs an object when its body uses the object it runs on — reads
//! or writes a field, or calls a method of the object that needs one — and
//! `toString()` always does. Called through the class name there is no
//! object to run on (the call would read fields of a null object), so such
//! a call is an error. Methods that use no instance member stay callable as
//! `ClassName.method()` (15-classes-and-objects.md, Static Methods).

use super::tast::*;
use crate::error::CompilerError;
use crate::resolver::SymbolId;
use std::collections::{HashMap, HashSet};

/// Every CLASS008 call in `program`.
pub fn check(program: &TastProgram) -> Vec<CompilerError> {
    let needs_object = methods_needing_an_object(program);
    let mut errors = Vec::new();
    walk_program(program, &mut |e| {
        if let TastExpressionKind::StaticMethodCall {
            class_name,
            method_name,
            method_symbol,
            ..
        } = &e.kind
        {
            let target = if needs_object.contains_key(method_symbol) {
                Some(*method_symbol)
            } else {
                find_method(program, class_name, method_name)
            };
            if target.is_some_and(|m| needs_object.get(&m) == Some(&true)) {
                errors.push(class008(class_name, method_name, &e.location));
            }
        }
        true
    });
    errors
}

fn class008(
    class_name: &str,
    method_name: &str,
    location: &crate::ast::SourceLocation,
) -> CompilerError {
    let var: String = class_name
        .chars()
        .next()
        .map(|c| c.to_lowercase().collect())
        .unwrap_or_default();
    CompilerError::semantic_error_with_code(
        format!(
            "'{method_name}()' needs a {class_name} object: create one (`{class_name} {var} = {class_name}()`) and call `{var}.{method_name}()`"
        ),
        Some(format!(
            "`{class_name}.{method_name}()` has no object to run on; only methods that use no field or method of the object can be called through the class name"
        )),
        Some(location.clone()),
        "CLASS008",
    )
}

/// The method `class_name.method_name` names: the class's own or the
/// nearest ancestor's.
fn find_method(program: &TastProgram, class_name: &str, method_name: &str) -> Option<SymbolId> {
    let mut class = program.classes.iter().find(|c| c.name == class_name);
    let mut seen = HashSet::new();
    while let Some(c) = class.filter(|c| seen.insert(c.symbol_id)) {
        if let Some(m) = c.methods.iter().find(|m| m.name == method_name) {
            return Some(m.symbol_id);
        }
        class = c
            .parent_class
            .and_then(|parent| program.classes.iter().find(|p| p.symbol_id == parent));
    }
    None
}

/// For every class method, whether it needs an object.
fn methods_needing_an_object(program: &TastProgram) -> HashMap<SymbolId, bool> {
    // (uses the object directly, own methods it calls)
    let mut uses: HashMap<SymbolId, (bool, Vec<SymbolId>)> = HashMap::new();
    let method_ids: HashSet<SymbolId> = program
        .classes
        .iter()
        .flat_map(|c| c.methods.iter().map(|m| m.symbol_id))
        .collect();
    for method in program.classes.iter().flat_map(|c| c.methods.iter()) {
        let mut direct = method.name == "toString";
        let mut callees = Vec::new();
        walk_block(&method.body, &mut |e| match &e.kind {
            TastExpressionKind::Variable { name, .. } if name == "this" => {
                direct = true;
                true
            }
            TastExpressionKind::MethodCall {
                receiver,
                method_symbol,
                ..
            } if is_this(receiver) && method_ids.contains(method_symbol) => {
                callees.push(*method_symbol);
                false
            }
            _ => true,
        });
        uses.insert(method.symbol_id, (direct, callees));
    }
    let mut needs: HashMap<SymbolId, bool> = uses
        .iter()
        .map(|(id, (direct, _))| (*id, *direct))
        .collect();
    loop {
        let mut changed = false;
        for (id, (_, callees)) in &uses {
            if !needs[id] && callees.iter().any(|c| needs.get(c) == Some(&true)) {
                needs.insert(*id, true);
                changed = true;
            }
        }
        if !changed {
            return needs;
        }
    }
}

fn is_this(e: &TastExpression) -> bool {
    matches!(&e.kind, TastExpressionKind::Variable { name, .. } if name == "this")
}

/// Visit every expression of `program` (see `walk_block`).
fn walk_program(program: &TastProgram, f: &mut dyn FnMut(&TastExpression) -> bool) {
    let functions = program
        .functions
        .iter()
        .chain(program.start_function.iter())
        .chain(
            program
                .classes
                .iter()
                .flat_map(|c| c.methods.iter().chain(c.constructors.iter())),
        );
    for function in functions {
        walk_block(&function.body, f);
    }
    for test in &program.tests {
        walk_block(&test.body, f);
    }
    for watch in &program.watch_blocks {
        walk_block(&watch.body, f);
    }
    for class in &program.classes {
        for value in class.fields.iter().filter_map(|f| f.default_value.as_ref()) {
            walk_expr(value, f);
        }
        for invariant in &class.invariants {
            walk_expr(invariant, f);
        }
    }
    if let Some(state) = &program.state {
        for computed in &state.computed {
            walk_block(&computed.body, f);
        }
        for declaration in &state.declarations {
            walk_expr(&declaration.initializer, f);
        }
        for rule in &state.rules {
            walk_expr(rule, f);
        }
    }
}

/// Visit every expression of `block`, outermost first. `f` returns whether
/// to visit the receiver of a method call.
fn walk_block(block: &TastBlock, f: &mut dyn FnMut(&TastExpression) -> bool) {
    for statement in &block.statements {
        walk_stmt(statement, f);
    }
}

fn walk_stmt(statement: &TastStatement, f: &mut dyn FnMut(&TastExpression) -> bool) {
    match statement {
        TastStatement::Expression { expression, .. }
        | TastStatement::Print { expression, .. }
        | TastStatement::LaterAssignment { expression, .. }
        | TastStatement::Background { expression, .. } => walk_expr(expression, f),
        TastStatement::VariableDeclaration { initializer, .. } => {
            if let Some(value) = initializer {
                walk_expr(value, f);
            }
        }
        TastStatement::Assignment { target, value, .. } => {
            walk_expr(target, f);
            walk_expr(value, f);
        }
        TastStatement::Return { value, .. } => {
            if let Some(value) = value {
                walk_expr(value, f);
            }
        }
        TastStatement::If {
            condition,
            then_block,
            else_block,
            ..
        } => {
            walk_expr(condition, f);
            walk_block(then_block, f);
            if let Some(else_block) = else_block {
                walk_block(else_block, f);
            }
        }
        TastStatement::For { iterable, body, .. } => {
            walk_expr(iterable, f);
            walk_block(body, f);
        }
        TastStatement::While {
            condition, body, ..
        } => {
            walk_expr(condition, f);
            walk_block(body, f);
        }
        TastStatement::Require { condition, .. } | TastStatement::Ensure { condition, .. } => {
            walk_expr(condition, f)
        }
        TastStatement::Break { .. } | TastStatement::Continue { .. } => {}
    }
}

fn walk_expr(expression: &TastExpression, f: &mut dyn FnMut(&TastExpression) -> bool) {
    let visit_receiver = f(expression);
    match &expression.kind {
        TastExpressionKind::Literal { .. } | TastExpressionKind::Variable { .. } => {}
        TastExpressionKind::BinaryOperation { left, right, .. } => {
            walk_expr(left, f);
            walk_expr(right, f);
        }
        TastExpressionKind::UnaryOperation { operand, .. } => walk_expr(operand, f),
        TastExpressionKind::FunctionCall {
            function,
            arguments,
            ..
        } => {
            walk_expr(function, f);
            arguments.iter().for_each(|a| walk_expr(a, f));
        }
        TastExpressionKind::MethodCall {
            receiver,
            arguments,
            ..
        } => {
            if visit_receiver {
                walk_expr(receiver, f);
            }
            arguments.iter().for_each(|a| walk_expr(a, f));
        }
        TastExpressionKind::StaticMethodCall { arguments, .. }
        | TastExpressionKind::BaseCall { arguments, .. } => {
            arguments.iter().for_each(|a| walk_expr(a, f))
        }
        TastExpressionKind::PropertyAccess { object, .. } => walk_expr(object, f),
        TastExpressionKind::ArrayLiteral { elements, .. } => {
            elements.iter().for_each(|e| walk_expr(e, f))
        }
        TastExpressionKind::ArrayAccess { array, index } => {
            walk_expr(array, f);
            walk_expr(index, f);
        }
        TastExpressionKind::ObjectLiteral { fields } => {
            fields.iter().for_each(|field| walk_expr(&field.value, f))
        }
        TastExpressionKind::OnError {
            expression,
            fallback,
        } => {
            walk_expr(expression, f);
            walk_expr(fallback, f);
        }
        TastExpressionKind::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_expr(condition, f);
            walk_expr(then_expr, f);
            walk_expr(else_expr, f);
        }
        TastExpressionKind::Range {
            start, end, step, ..
        } => {
            walk_expr(start, f);
            walk_expr(end, f);
            if let Some(step) = step {
                walk_expr(step, f);
            }
        }
    }
}
