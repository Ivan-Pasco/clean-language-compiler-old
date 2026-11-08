//! Drop the `T?` wrapper of strings, lists, pairs maps, matrices and class
//! instances from a type-checked program.
//!
//! `T?` is a compile-time distinction: the type checker uses it to keep none
//! out of plain types (type-system.md §8). At runtime a `T?` is stored
//! exactly like a `T` (none is the zero value / null pointer), so the MIR
//! builder works on the plain type. Without this pass a `pairs<string,
//! integer>?` declaration was not recognised as a pairs map and its literal
//! was built as something else.

use super::tast::*;

/// Replace every `T?` of a non-primitive `T` in `program` with `T`.
pub fn erase_optional(program: &mut TastProgram) {
    for function in &mut program.functions {
        function_(function);
    }
    for class in &mut program.classes {
        for field in &mut class.fields {
            ty(&mut field.field_type);
            if let Some(value) = &mut field.default_value {
                expr(value);
            }
        }
        for method in class
            .methods
            .iter_mut()
            .chain(class.constructors.iter_mut())
        {
            function_(method);
        }
        for invariant in &mut class.invariants {
            expr(invariant);
        }
    }
    if let Some(start) = &mut program.start_function {
        function_(start);
    }
    for test in &mut program.tests {
        block(&mut test.body);
    }
    if let Some(state) = &mut program.state {
        for declaration in &mut state.declarations {
            ty(&mut declaration.state_type);
            expr(&mut declaration.initializer);
            if let Some(guard) = &mut declaration.guard {
                expr(&mut guard.condition);
            }
        }
        for computed in &mut state.computed {
            ty(&mut computed.computed_type);
            block(&mut computed.body);
        }
        for rule in &mut state.rules {
            expr(rule);
        }
    }
    for watch in &mut program.watch_blocks {
        block(&mut watch.body);
    }
    for t in program.type_env.values_mut() {
        ty(t);
    }
    for external in &mut program.externals {
        for parameter in &mut external.parameters {
            parameter_(parameter);
        }
        ty(&mut external.return_type);
    }
}

/// `T?` → `T`, inside composite types too. `integer?`, `number?` and
/// `boolean?` stay: the MIR builder tests them for none and skips the test
/// for their plain forms, which never hold none.
pub fn ty(t: &mut ConcreteType) {
    while let ConcreteType::Optional(inner) = t {
        if is_primitive(inner) {
            return;
        }
        *t = std::mem::replace(inner.as_mut(), ConcreteType::Unknown);
    }
    match t {
        ConcreteType::Array(inner) | ConcreteType::Matrix(inner) => ty(inner),
        ConcreteType::Pairs(key, value) => {
            ty(key);
            ty(value);
        }
        ConcreteType::Function {
            parameters,
            return_type,
            ..
        } => {
            parameters.iter_mut().for_each(ty);
            ty(return_type);
        }
        ConcreteType::Class { type_args, .. } | ConcreteType::Interface { type_args, .. } => {
            type_args.iter_mut().for_each(ty)
        }
        ConcreteType::Union(members) => members.iter_mut().for_each(ty),
        ConcreteType::Generic { bounds, .. } => bounds.iter_mut().for_each(ty),
        _ => {}
    }
}

fn is_primitive(t: &ConcreteType) -> bool {
    matches!(
        t,
        ConcreteType::Integer
            | ConcreteType::IntegerSized { .. }
            | ConcreteType::Number
            | ConcreteType::NumberSized { .. }
            | ConcreteType::Boolean
    )
}

fn function_(function: &mut TastFunction) {
    for parameter in &mut function.parameters {
        parameter_(parameter);
    }
    ty(&mut function.return_type);
    block(&mut function.body);
}

fn parameter_(parameter: &mut TastParameter) {
    ty(&mut parameter.param_type);
    if let Some(value) = &mut parameter.default_value {
        expr(value);
    }
}

fn block(block: &mut TastBlock) {
    ty(&mut block.return_type);
    for statement in &mut block.statements {
        stmt(statement);
    }
}

fn stmt(statement: &mut TastStatement) {
    match statement {
        TastStatement::Expression { expression, .. }
        | TastStatement::Print { expression, .. }
        | TastStatement::LaterAssignment { expression, .. }
        | TastStatement::Background { expression, .. } => expr(expression),
        TastStatement::VariableDeclaration {
            var_type,
            initializer,
            ..
        } => {
            ty(var_type);
            if let Some(value) = initializer {
                expr(value);
            }
        }
        TastStatement::Assignment { target, value, .. } => {
            expr(target);
            expr(value);
        }
        TastStatement::Return {
            value, return_type, ..
        } => {
            ty(return_type);
            if let Some(value) = value {
                expr(value);
            }
        }
        TastStatement::If {
            condition,
            then_block,
            else_block,
            result_type,
            ..
        } => {
            expr(condition);
            block(then_block);
            if let Some(else_block) = else_block {
                block(else_block);
            }
            ty(result_type);
        }
        TastStatement::For { iterable, body, .. } => {
            expr(iterable);
            block(body);
        }
        TastStatement::While {
            condition, body, ..
        } => {
            expr(condition);
            block(body);
        }
        TastStatement::Require { condition, .. } | TastStatement::Ensure { condition, .. } => {
            expr(condition)
        }
        TastStatement::Break { .. } | TastStatement::Continue { .. } => {}
    }
}

fn expr(expression: &mut TastExpression) {
    ty(&mut expression.expr_type);
    match &mut expression.kind {
        TastExpressionKind::Literal { .. } | TastExpressionKind::Variable { .. } => {}
        TastExpressionKind::BinaryOperation { left, right, .. } => {
            expr(left);
            expr(right);
        }
        TastExpressionKind::UnaryOperation { operand, .. } => expr(operand),
        TastExpressionKind::FunctionCall {
            function,
            arguments,
            type_args,
        } => {
            expr(function);
            arguments.iter_mut().for_each(expr);
            type_args.iter_mut().for_each(ty);
        }
        TastExpressionKind::MethodCall {
            receiver,
            arguments,
            type_args,
            ..
        } => {
            expr(receiver);
            arguments.iter_mut().for_each(expr);
            type_args.iter_mut().for_each(ty);
        }
        TastExpressionKind::StaticMethodCall {
            arguments,
            type_args,
            ..
        } => {
            arguments.iter_mut().for_each(expr);
            type_args.iter_mut().for_each(ty);
        }
        TastExpressionKind::PropertyAccess { object, .. } => expr(object),
        TastExpressionKind::ArrayLiteral {
            elements,
            element_type,
        } => {
            elements.iter_mut().for_each(expr);
            ty(element_type);
        }
        TastExpressionKind::ArrayAccess { array, index } => {
            expr(array);
            expr(index);
        }
        TastExpressionKind::ObjectLiteral { fields } => {
            for field in fields {
                expr(&mut field.value);
            }
        }
        TastExpressionKind::OnError {
            expression,
            fallback,
        } => {
            expr(expression);
            expr(fallback);
        }
        TastExpressionKind::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr(condition);
            expr(then_expr);
            expr(else_expr);
        }
        TastExpressionKind::BaseCall { arguments, .. } => arguments.iter_mut().for_each(expr),
        TastExpressionKind::Range {
            start, end, step, ..
        } => {
            expr(start);
            expr(end);
            if let Some(step) = step {
                expr(step);
            }
        }
    }
}
