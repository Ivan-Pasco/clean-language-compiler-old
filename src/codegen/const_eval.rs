//! Compile-time constant evaluation for state initializers
//!
//! This module provides constant folding for primitive expressions used in
//! state: block initializers. If an expression can be evaluated at compile time,
//! it will be folded into a constant value that can be directly emitted in
//! the WASM global section.

use crate::ast::Value;
use crate::hir::{HirBinaryOp, HirExpression, HirUnaryOp};

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

    /// Returns true if this constant can be directly emitted as a WASM const expr
    /// (strings need runtime allocation, so they return false)
    pub fn is_wasm_const(&self) -> bool {
        matches!(
            self,
            ConstValue::Integer(_) | ConstValue::Float(_) | ConstValue::Boolean(_)
        )
    }
}

/// Try to evaluate an HIR expression at compile time
///
/// This function attempts to fold constant expressions during compilation.
/// Supported operations:
/// - Literals (integer, float, boolean, string)
/// - Unary negation and not
/// - Binary arithmetic (+, -, *, /, %)
///
/// Returns `ConstValue::NotConst` for any expression that requires runtime evaluation
/// (variables, function calls, property access, etc.)
pub fn try_const_eval(expr: &HirExpression) -> ConstValue {
    match expr {
        HirExpression::Literal { value, .. } => eval_literal(value),

        HirExpression::UnaryOp { op, operand, .. } => {
            let val = try_const_eval(operand);
            eval_unary(op, val)
        }

        HirExpression::BinaryOp {
            left, op, right, ..
        } => {
            let l = try_const_eval(left);
            let r = try_const_eval(right);
            eval_binary(op, l, r)
        }

        // All other expressions require runtime evaluation
        HirExpression::Variable { .. }
        | HirExpression::Call { .. }
        | HirExpression::MethodCall { .. }
        | HirExpression::FieldAccess { .. }
        | HirExpression::Index { .. }
        | HirExpression::Assignment { .. }
        | HirExpression::Array { .. }
        | HirExpression::Constructor { .. }
        | HirExpression::Cast { .. }
        | HirExpression::NamespaceCall { .. }
        | HirExpression::StaticMethodCall { .. }
        | HirExpression::OnError { .. }
        | HirExpression::Conditional { .. }
        | HirExpression::BaseCall { .. }
        | HirExpression::Range { .. }
        | HirExpression::ObjectLiteral { .. } => ConstValue::NotConst,
    }
}

/// Evaluate a literal value to a constant
fn eval_literal(value: &Value) -> ConstValue {
    match value {
        Value::Integer(n) => ConstValue::Integer(*n),
        Value::Number(f) => ConstValue::Float(*f),
        Value::Boolean(b) => ConstValue::Boolean(*b),
        Value::String(s) => ConstValue::String(s.clone()),
        Value::Integer8(n) => ConstValue::Integer(*n as i64),
        Value::Integer8u(n) => ConstValue::Integer(*n as i64),
        Value::Integer16(n) => ConstValue::Integer(*n as i64),
        Value::Integer16u(n) => ConstValue::Integer(*n as i64),
        Value::Integer32(n) => ConstValue::Integer(*n as i64),
        Value::Integer64(n) => ConstValue::Integer(*n),
        Value::Number32(f) => ConstValue::Float(*f as f64),
        Value::Number64(f) => ConstValue::Float(*f),
        Value::None | Value::Void | Value::Matrix(_) | Value::List(_) | Value::Pairs(_) => {
            ConstValue::NotConst
        }
    }
}

/// Evaluate a unary operation on a constant value
fn eval_unary(op: &HirUnaryOp, val: ConstValue) -> ConstValue {
    match (op, val) {
        (HirUnaryOp::Negate, ConstValue::Integer(n)) => ConstValue::Integer(-n),
        (HirUnaryOp::Negate, ConstValue::Float(f)) => ConstValue::Float(-f),
        (HirUnaryOp::Not, ConstValue::Boolean(b)) => ConstValue::Boolean(!b),
        _ => ConstValue::NotConst,
    }
}

/// Evaluate a binary operation on two constant values
fn eval_binary(op: &HirBinaryOp, left: ConstValue, right: ConstValue) -> ConstValue {
    match (op, left, right) {
        // Integer arithmetic
        (HirBinaryOp::Add, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Integer(l.wrapping_add(r))
        }
        (HirBinaryOp::Subtract, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Integer(l.wrapping_sub(r))
        }
        (HirBinaryOp::Multiply, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Integer(l.wrapping_mul(r))
        }
        (HirBinaryOp::Divide, ConstValue::Integer(l), ConstValue::Integer(r)) if r != 0 => {
            ConstValue::Integer(l / r)
        }
        (HirBinaryOp::Modulo, ConstValue::Integer(l), ConstValue::Integer(r)) if r != 0 => {
            ConstValue::Integer(l % r)
        }
        (HirBinaryOp::Power, ConstValue::Integer(l), ConstValue::Integer(r)) if r >= 0 => {
            ConstValue::Integer(l.wrapping_pow(r as u32))
        }

        // Float arithmetic
        (HirBinaryOp::Add, ConstValue::Float(l), ConstValue::Float(r)) => ConstValue::Float(l + r),
        (HirBinaryOp::Subtract, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Float(l - r)
        }
        (HirBinaryOp::Multiply, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Float(l * r)
        }
        (HirBinaryOp::Divide, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Float(l / r)
        }
        (HirBinaryOp::Power, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Float(l.powf(r))
        }

        // Mixed int/float - promote to float
        (HirBinaryOp::Add, ConstValue::Integer(l), ConstValue::Float(r)) => {
            ConstValue::Float(l as f64 + r)
        }
        (HirBinaryOp::Add, ConstValue::Float(l), ConstValue::Integer(r)) => {
            ConstValue::Float(l + r as f64)
        }
        (HirBinaryOp::Subtract, ConstValue::Integer(l), ConstValue::Float(r)) => {
            ConstValue::Float(l as f64 - r)
        }
        (HirBinaryOp::Subtract, ConstValue::Float(l), ConstValue::Integer(r)) => {
            ConstValue::Float(l - r as f64)
        }
        (HirBinaryOp::Multiply, ConstValue::Integer(l), ConstValue::Float(r)) => {
            ConstValue::Float(l as f64 * r)
        }
        (HirBinaryOp::Multiply, ConstValue::Float(l), ConstValue::Integer(r)) => {
            ConstValue::Float(l * r as f64)
        }
        (HirBinaryOp::Divide, ConstValue::Integer(l), ConstValue::Float(r)) => {
            ConstValue::Float(l as f64 / r)
        }
        (HirBinaryOp::Divide, ConstValue::Float(l), ConstValue::Integer(r)) => {
            ConstValue::Float(l / r as f64)
        }

        // String concatenation
        (HirBinaryOp::Add, ConstValue::String(l), ConstValue::String(r)) => {
            ConstValue::String(l + &r)
        }
        (HirBinaryOp::StringConcat, ConstValue::String(l), ConstValue::String(r)) => {
            ConstValue::String(l + &r)
        }

        // Boolean operations
        (HirBinaryOp::And, ConstValue::Boolean(l), ConstValue::Boolean(r)) => {
            ConstValue::Boolean(l && r)
        }
        (HirBinaryOp::Or, ConstValue::Boolean(l), ConstValue::Boolean(r)) => {
            ConstValue::Boolean(l || r)
        }

        // Comparison operations on integers
        (HirBinaryOp::Equal, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l == r)
        }
        (HirBinaryOp::NotEqual, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l != r)
        }
        (HirBinaryOp::Less, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l < r)
        }
        (HirBinaryOp::LessEqual, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l <= r)
        }
        (HirBinaryOp::Greater, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l > r)
        }
        (HirBinaryOp::GreaterEqual, ConstValue::Integer(l), ConstValue::Integer(r)) => {
            ConstValue::Boolean(l >= r)
        }

        // Comparison operations on floats
        (HirBinaryOp::Equal, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l == r)
        }
        (HirBinaryOp::NotEqual, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l != r)
        }
        (HirBinaryOp::Less, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l < r)
        }
        (HirBinaryOp::LessEqual, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l <= r)
        }
        (HirBinaryOp::Greater, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l > r)
        }
        (HirBinaryOp::GreaterEqual, ConstValue::Float(l), ConstValue::Float(r)) => {
            ConstValue::Boolean(l >= r)
        }

        // String comparison
        (HirBinaryOp::Equal, ConstValue::String(l), ConstValue::String(r)) => {
            ConstValue::Boolean(l == r)
        }
        (HirBinaryOp::NotEqual, ConstValue::String(l), ConstValue::String(r)) => {
            ConstValue::Boolean(l != r)
        }

        // Boolean comparison
        (HirBinaryOp::Equal, ConstValue::Boolean(l), ConstValue::Boolean(r)) => {
            ConstValue::Boolean(l == r)
        }
        (HirBinaryOp::NotEqual, ConstValue::Boolean(l), ConstValue::Boolean(r)) => {
            ConstValue::Boolean(l != r)
        }

        // Everything else cannot be const-evaluated
        _ => ConstValue::NotConst,
    }
}

// ============================================================================
// TAST Expression Evaluation
// ============================================================================

use crate::typechecker::tast::{
    BinaryOperator, TastExpression, TastExpressionKind, TastLiteral, UnaryOperator,
};

/// Try to evaluate a TAST expression at compile time
///
/// This is used during MIR building to evaluate state initializers.
/// It supports the same operations as the HIR evaluator.
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
        | TastExpressionKind::Lambda { .. }
        | TastExpressionKind::Cast { .. }
        | TastExpressionKind::TypeCheck { .. }
        | TastExpressionKind::Await { .. }
        | TastExpressionKind::AsyncBlock { .. }
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

    fn make_int_literal(n: i64) -> HirExpression {
        HirExpression::Literal {
            value: Value::Integer(n),
            location: SourceLocation::default(),
        }
    }

    fn make_float_literal(f: f64) -> HirExpression {
        HirExpression::Literal {
            value: Value::Number(f),
            location: SourceLocation::default(),
        }
    }

    fn make_string_literal(s: &str) -> HirExpression {
        HirExpression::Literal {
            value: Value::String(s.to_string()),
            location: SourceLocation::default(),
        }
    }

    #[test]
    fn test_integer_literal() {
        let expr = make_int_literal(42);
        match try_const_eval(&expr) {
            ConstValue::Integer(n) => assert_eq!(n, 42),
            _ => panic!("Expected integer"),
        }
    }

    #[test]
    fn test_float_literal() {
        let expr = make_float_literal(std::f64::consts::PI);
        match try_const_eval(&expr) {
            ConstValue::Float(f) => assert!((f - std::f64::consts::PI).abs() < 0.001),
            _ => panic!("Expected float"),
        }
    }

    #[test]
    fn test_integer_addition() {
        let expr = HirExpression::BinaryOp {
            left: Box::new(make_int_literal(2)),
            op: HirBinaryOp::Add,
            right: Box::new(make_int_literal(3)),
            location: SourceLocation::default(),
        };
        match try_const_eval(&expr) {
            ConstValue::Integer(n) => assert_eq!(n, 5),
            _ => panic!("Expected integer"),
        }
    }

    #[test]
    fn test_negation() {
        let expr = HirExpression::UnaryOp {
            op: HirUnaryOp::Negate,
            operand: Box::new(make_int_literal(42)),
            location: SourceLocation::default(),
        };
        match try_const_eval(&expr) {
            ConstValue::Integer(n) => assert_eq!(n, -42),
            _ => panic!("Expected integer"),
        }
    }

    #[test]
    fn test_string_concat() {
        let expr = HirExpression::BinaryOp {
            left: Box::new(make_string_literal("Hello, ")),
            op: HirBinaryOp::Add,
            right: Box::new(make_string_literal("World!")),
            location: SourceLocation::default(),
        };
        match try_const_eval(&expr) {
            ConstValue::String(s) => assert_eq!(s, "Hello, World!"),
            _ => panic!("Expected string"),
        }
    }

    #[test]
    fn test_complex_expression() {
        // Test: (2 + 3) * 4 = 20
        let inner = HirExpression::BinaryOp {
            left: Box::new(make_int_literal(2)),
            op: HirBinaryOp::Add,
            right: Box::new(make_int_literal(3)),
            location: SourceLocation::default(),
        };
        let expr = HirExpression::BinaryOp {
            left: Box::new(inner),
            op: HirBinaryOp::Multiply,
            right: Box::new(make_int_literal(4)),
            location: SourceLocation::default(),
        };
        match try_const_eval(&expr) {
            ConstValue::Integer(n) => assert_eq!(n, 20),
            _ => panic!("Expected integer"),
        }
    }

    #[test]
    fn test_variable_is_not_const() {
        let expr = HirExpression::Variable {
            name: "x".to_string(),
            location: SourceLocation::default(),
        };
        assert!(matches!(try_const_eval(&expr), ConstValue::NotConst));
    }
}
