//! Compile-time constant evaluation for state initializers
//!
//! This module provides constant folding for primitive expressions used in
//! state: block initializers. If an expression can be evaluated at compile time,
//! it will be folded into a constant value that can be directly emitted in
//! the WASM global section.

use crate::typechecker::tast::{
    BinaryOperator, TastExpression, TastExpressionKind, TastLiteral, UnaryOperator,
};

/// Result of attempting to evaluate an expression at compile time
#[derive(Debug, Clone)]
pub enum ConstValue {
    /// Compile-time integer constant
    Integer(i64),
    /// Compile-time float constant
    Float(f64),
    /// Compile-time boolean constant
    Boolean(bool),
    /// Compile-time string constant (requires runtime allocation)
    String(String),
    /// Expression cannot be evaluated at compile time
    NotConst,
}

impl ConstValue {
    /// Returns true if this is a compile-time constant
    pub fn is_const(&self) -> bool {
        !matches!(self, ConstValue::NotConst)
    }
}

/// Try to evaluate a TAST expression at compile time
///
/// This is used during MIR building to evaluate state initializers.
/// Supported: literals, unary negation / not / plus, and binary arithmetic,
/// string concatenation, boolean logic and comparisons on constants. Returns
/// `ConstValue::NotConst` for anything that needs runtime evaluation.
pub fn try_const_eval_tast(expr: &TastExpression) -> ConstValue {
    match &expr.kind {
        TastExpressionKind::Literal { value } => eval_tast_literal(value),

        TastExpressionKind::UnaryOperation { operator, operand } => {
            let val = try_const_eval_tast(operand);
            eval_tast_unary(operator, val)
        }

        TastExpressionKind::BinaryOperation {
            operator,
            left,
            right,
        } => {
            let l = try_const_eval_tast(left);
            let r = try_const_eval_tast(right);
            eval_tast_binary(operator, l, r)
        }

        // All other TAST expressions require runtime evaluation
        TastExpressionKind::Variable { .. }
        | TastExpressionKind::FunctionCall { .. }
        | TastExpressionKind::MethodCall { .. }
        | TastExpressionKind::StaticMethodCall { .. }
        | TastExpressionKind::PropertyAccess { .. }
        | TastExpressionKind::ArrayLiteral { .. }
        | TastExpressionKind::ArrayAccess { .. }
        | TastExpressionKind::ObjectLiteral { .. }
        | TastExpressionKind::Cast { .. }
        | TastExpressionKind::TypeCheck { .. }
        | TastExpressionKind::Await { .. }
        | TastExpressionKind::OnError { .. }
        | TastExpressionKind::Conditional { .. }
        | TastExpressionKind::BaseCall { .. }
        | TastExpressionKind::Range { .. } => ConstValue::NotConst,
    }
}

/// Evaluate a TAST literal to a constant
fn eval_tast_literal(lit: &TastLiteral) -> ConstValue {
    match lit {
        TastLiteral::Integer(n) => ConstValue::Integer(*n),
        TastLiteral::Number(f) => ConstValue::Float(*f),
        TastLiteral::Boolean(b) => ConstValue::Boolean(*b),
        TastLiteral::String(s) => ConstValue::String(s.clone()),
        TastLiteral::Null | TastLiteral::Undefined => ConstValue::NotConst,
    }
}

/// Evaluate a TAST unary operation on a constant value
fn eval_tast_unary(op: &UnaryOperator, val: ConstValue) -> ConstValue {
    match (op, val) {
        (UnaryOperator::Negate, ConstValue::Integer(n)) => ConstValue::Integer(-n),
        (UnaryOperator::Negate, ConstValue::Float(f)) => ConstValue::Float(-f),
        (UnaryOperator::Not, ConstValue::Boolean(b)) => ConstValue::Boolean(!b),
        (UnaryOperator::Plus, v) => v, // Unary plus is identity
        _ => ConstValue::NotConst,
    }
}

/// Evaluate a TAST binary operation on two constant values
fn eval_tast_binary(op: &BinaryOperator, left: ConstValue, right: ConstValue) -> ConstValue {
    match (op, left, right) {
        // Integer arithmetic
        (BinaryOperator::Add, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Integer(l.wrapping_add(r))
        }
        (BinaryOperator::Subtract, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Integer(l.wrapping_sub(r))
        }
        (BinaryOperator::Multiply, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Integer(l.wrapping_mul(r))
        }
        (BinaryOperator::Divide, ConstValue::Integer(l), ConstValue::Integer(r)) if r != 0 => {
            ConstValue::Integer(l / r)
        }
        (BinaryOperator::Modulo, ConstValue::Integer(l), ConstValue::Integer(r)) if r != 0 => {
            ConstValue::Integer(l % r)
        }
        (BinaryOperator::Power, ConstValue::Integer(l), ConstValue::Integer(r)) if r >= 0 => {
            ConstValue::Integer(l.wrapping_pow(r as u32))
        }

        // Float arithmetic
        (BinaryOperator::Add, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Float(l + r)
        }
        (BinaryOperator::Subtract, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Float(l - r)
        }
        (BinaryOperator::Multiply, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Float(l * r)
        }
        (BinaryOperator::Divide, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Float(l / r)
        }
        (BinaryOperator::Power, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Float(l.powf(r))
        }

        // Mixed int/float - promote to float
        (BinaryOperator::Add, ConstValue::Integer(l), ConstValue::Float(r)) => {
            ConstValue::Float(l as f64 + r)
        }
        (BinaryOperator::Add, ConstValue::Float(l), ConstValue::Integer(r)) => {
            ConstValue::Float(l + r as f64)
        }
        (BinaryOperator::Subtract, ConstValue::Integer(l), ConstValue::Float(r)) => {
            ConstValue::Float(l as f64 - r)
        }
        (BinaryOperator::Subtract, ConstValue::Float(l), ConstValue::Integer(r)) => {
            ConstValue::Float(l - r as f64)
        }
        (BinaryOperator::Multiply, ConstValue::Integer(l), ConstValue::Float(r)) => {
            ConstValue::Float(l as f64 * r)
        }
        (BinaryOperator::Multiply, ConstValue::Float(l), ConstValue::Integer(r)) => {
            ConstValue::Float(l * r as f64)
        }
        (BinaryOperator::Divide, ConstValue::Integer(l), ConstValue::Float(r)) => {
            ConstValue::Float(l as f64 / r)
        }
        (BinaryOperator::Divide, ConstValue::Float(l), ConstValue::Integer(r)) => {
            ConstValue::Float(l / r as f64)
        }

        // String concatenation
        (BinaryOperator::Add, ConstValue::String(l), ConstValue::String(r)) => {
            ConstValue::String(l + &r)
        }

        // Boolean operations
        (BinaryOperator::And, ConstValue::Boolean(l), ConstValue::Boolean(r)) => {
            ConstValue::Boolean(l && r)
        }
        (BinaryOperator::Or, ConstValue::Boolean(l), ConstValue::Boolean(r)) => {
            ConstValue::Boolean(l || r)
        }

        // Comparison operations on integers
        (BinaryOperator::Equal, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l == r)
        }
        (BinaryOperator::NotEqual, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l != r)
        }
        (BinaryOperator::LessThan, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l < r)
        }
        (BinaryOperator::LessThanOrEqual, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l <= r)
        }
        (BinaryOperator::GreaterThan, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l > r)
        }
        (BinaryOperator::GreaterThanOrEqual, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l >= r)
        }

        // Comparison operations on floats
        (BinaryOperator::Equal, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l == r)
        }
        (BinaryOperator::NotEqual, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l != r)
        }
        (BinaryOperator::LessThan, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l < r)
        }
        (BinaryOperator::LessThanOrEqual, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l <= r)
        }
        (BinaryOperator::GreaterThan, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l > r)
        }
        (BinaryOperator::GreaterThanOrEqual, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l >= r)
        }

        // String comparison
        (BinaryOperator::Equal, ConstValue::String(l), ConstValue::String(r)) => {
            ConstValue::Boolean(l == r)
        }
        (BinaryOperator::NotEqual, ConstValue::String(l), ConstValue::String(r)) => {
            ConstValue::Boolean(l != r)
        }

        // Boolean comparison
        (BinaryOperator::Equal, ConstValue::Boolean(l), ConstValue::Boolean(r)) => {
            ConstValue::Boolean(l == r)
        }
        (BinaryOperator::NotEqual, ConstValue::Boolean(l), ConstValue::Boolean(r)) => {
            ConstValue::Boolean(l != r)
        }

        // Everything else cannot be const-evaluated
        _ => ConstValue::NotConst,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::SourceLocation;
    use crate::resolver::SymbolId;
    use crate::typechecker::tast::ConcreteType;

    fn expr(kind: TastExpressionKind, expr_type: ConcreteType) -> TastExpression {
        TastExpression {
            kind,
            expr_type,
            location: SourceLocation::default(),
        }
    }

    fn literal(value: TastLiteral, expr_type: ConcreteType) -> TastExpression {
        expr(TastExpressionKind::Literal { value }, expr_type)
    }

    fn int(n: i64) -> TastExpression {
        literal(TastLiteral::Integer(n), ConcreteType::Integer)
    }

    fn binary(
        left: TastExpression,
        operator: BinaryOperator,
        right: TastExpression,
    ) -> TastExpression {
        let expr_type = left.expr_type.clone();
        expr(
            TastExpressionKind::BinaryOperation {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            },
            expr_type,
        )
    }

    #[test]
    fn test_integer_literal() {
        assert!(matches!(
            try_const_eval_tast(&int(42)),
            ConstValue::Integer(42)
        ));
    }

    #[test]
    fn test_float_literal() {
        let pi = literal(
            TastLiteral::Number(std::f64::consts::PI),
            ConcreteType::Number,
        );
        match try_const_eval_tast(&pi) {
            ConstValue::Float(f) => assert!((f - std::f64::consts::PI).abs() < 0.001),
            other => panic!("Expected float, got {:?}", other),
        }
    }

    #[test]
    fn test_integer_addition() {
        let sum = binary(int(2), BinaryOperator::Add, int(3));
        assert!(matches!(try_const_eval_tast(&sum), ConstValue::Integer(5)));
    }

    #[test]
    fn test_negation() {
        let neg = expr(
            TastExpressionKind::UnaryOperation {
                operator: UnaryOperator::Negate,
                operand: Box::new(int(42)),
            },
            ConcreteType::Integer,
        );
        assert!(matches!(
            try_const_eval_tast(&neg),
            ConstValue::Integer(-42)
        ));
    }

    #[test]
    fn test_string_concat() {
        let hello = literal(
            TastLiteral::String("Hello, ".to_string()),
            ConcreteType::String,
        );
        let world = literal(
            TastLiteral::String("World!".to_string()),
            ConcreteType::String,
        );
        match try_const_eval_tast(&binary(hello, BinaryOperator::Add, world)) {
            ConstValue::String(s) => assert_eq!(s, "Hello, World!"),
            other => panic!("Expected string, got {:?}", other),
        }
    }

    #[test]
    fn test_complex_expression() {
        // (2 + 3) * 4 = 20
        let product = binary(
            binary(int(2), BinaryOperator::Add, int(3)),
            BinaryOperator::Multiply,
            int(4),
        );
        assert!(matches!(
            try_const_eval_tast(&product),
            ConstValue::Integer(20)
        ));
    }

    #[test]
    fn test_variable_is_not_const() {
        let var = expr(
            TastExpressionKind::Variable {
                symbol_id: SymbolId(1),
                name: "x".to_string(),
            },
            ConcreteType::Integer,
        );
        assert!(matches!(try_const_eval_tast(&var), ConstValue::NotConst));
    }

    #[test]
    fn test_division_by_zero_is_not_const() {
        let div = binary(int(1), BinaryOperator::Divide, int(0));
        assert!(matches!(try_const_eval_tast(&div), ConstValue::NotConst));
    }
}
