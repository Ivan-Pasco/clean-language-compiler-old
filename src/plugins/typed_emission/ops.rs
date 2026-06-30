//! Typed-emission op-code lookup tables.
//!
//! These tables mirror `foundation/spec/plugins/contracts/typed-emission-ops.toml`
//! exactly. The numbering is STABLE FOREVER — a new op gets the next available
//! integer; no slot is ever reused or renumbered.
//!
//! Plugins call `_expr_binop_op(ctx, op_code, lhs, rhs)` and
//! `_expr_unop_op(ctx, op_code, operand)` using these codes instead of
//! LP-string operator names, enabling ABI-stable numeric dispatch.
//!
//! See: foundation/spec/plugins/contracts/typed-emission.md §3.9–3.10

use crate::ast::{BinaryOperator, UnaryOperator};

// ── Binary operator table (mirrors [binop] in typed-emission-ops.toml) ───────

/// Maximum valid binary op code (inclusive). Update when appending new slots.
pub const BINOP_MAX_CODE: i32 = 15;

/// Map a stable integer op code to a `BinaryOperator` variant.
///
/// Returns `None` for any code outside the range `0..=BINOP_MAX_CODE`.
/// The bridge emits `PLUGIN_INVALID_BINOP_CODE` and returns handle 0 on
/// invalid codes.
///
/// Table (matches `[binop]` in typed-emission-ops.toml, numbering is frozen):
/// ```text
///  0  "+"         additive_op
///  1  "-"         additive_op
///  2  "*"         multiplicative_op
///  3  "/"         multiplicative_op
///  4  "%"         multiplicative_op
///  5  "=="        comparison_op
///  6  "!="        comparison_op
///  7  "<"         comparison_op
///  8  ">"         comparison_op
///  9  "<="        comparison_op
/// 10  ">="        comparison_op
/// 11  "and"       logical_op
/// 12  "or"        logical_op
/// 13  "default"   default_op
/// 14  "^"         power_op (right-associative)
/// 15  "is"        comparison_op (type test)
/// ```
pub fn binop_from_code(code: i32) -> Option<BinaryOperator> {
    Some(match code {
        0 => BinaryOperator::Add,
        1 => BinaryOperator::Subtract,
        2 => BinaryOperator::Multiply,
        3 => BinaryOperator::Divide,
        4 => BinaryOperator::Modulo,
        5 => BinaryOperator::Equal,
        6 => BinaryOperator::NotEqual,
        7 => BinaryOperator::Less,
        8 => BinaryOperator::Greater,
        9 => BinaryOperator::LessEqual,
        10 => BinaryOperator::GreaterEqual,
        11 => BinaryOperator::And,
        12 => BinaryOperator::Or,
        13 => BinaryOperator::Default,
        14 => BinaryOperator::Power,
        15 => BinaryOperator::Is,
        _ => return None,
    })
}

// ── Unary operator table (mirrors [unop] in typed-emission-ops.toml) ─────────

/// Maximum valid unary op code (inclusive). Update when appending new slots.
pub const UNOP_MAX_CODE: i32 = 1;

/// Map a stable integer op code to a `UnaryOperator` variant.
///
/// Returns `None` for any code outside the range `0..=UNOP_MAX_CODE`.
/// The bridge emits `PLUGIN_INVALID_UNOP_CODE` and returns handle 0 on
/// invalid codes.
///
/// Table (matches `[unop]` in typed-emission-ops.toml, numbering is frozen):
/// ```text
///  0  "-"         unary_op (negate)
///  1  "not"       unary_op (logical not)
/// ```
pub fn unop_from_code(code: i32) -> Option<UnaryOperator> {
    Some(match code {
        0 => UnaryOperator::Negate,
        1 => UnaryOperator::Not,
        _ => return None,
    })
}

// ── Round-trip helpers (used in tests) ───────────────────────────────────────

/// Return the string representation of a binary op code (for diagnostics).
/// Returns `"<unknown>"` for out-of-range codes.
#[allow(dead_code)]
pub fn binop_code_to_str(code: i32) -> &'static str {
    match code {
        0 => "+",
        1 => "-",
        2 => "*",
        3 => "/",
        4 => "%",
        5 => "==",
        6 => "!=",
        7 => "<",
        8 => ">",
        9 => "<=",
        10 => ">=",
        11 => "and",
        12 => "or",
        13 => "default",
        14 => "^",
        15 => "is",
        _ => "<unknown>",
    }
}

/// Return the string representation of a unary op code (for diagnostics).
/// Returns `"<unknown>"` for out-of-range codes.
#[allow(dead_code)]
pub fn unop_code_to_str(code: i32) -> &'static str {
    match code {
        0 => "-",
        1 => "not",
        _ => "<unknown>",
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Every defined binop code maps to a variant.
    #[test]
    fn all_binop_codes_map_to_variants() {
        for code in 0..=BINOP_MAX_CODE {
            assert!(
                binop_from_code(code).is_some(),
                "binop_from_code({code}) returned None — table gap"
            );
        }
    }

    /// Every defined unop code maps to a variant.
    #[test]
    fn all_unop_codes_map_to_variants() {
        for code in 0..=UNOP_MAX_CODE {
            assert!(
                unop_from_code(code).is_some(),
                "unop_from_code({code}) returned None — table gap"
            );
        }
    }

    /// Codes just past the valid range return None (no accidental extension).
    #[test]
    fn out_of_range_codes_return_none() {
        assert!(binop_from_code(BINOP_MAX_CODE + 1).is_none());
        assert!(binop_from_code(-1).is_none());
        assert!(unop_from_code(UNOP_MAX_CODE + 1).is_none());
        assert!(unop_from_code(-1).is_none());
    }

    /// Round-trip: binop_code_to_str agrees with map_binary_op on the same string.
    #[test]
    fn binop_str_round_trip_agrees_with_lp_string_table() {
        use crate::plugins::typed_emission::arena::map_binary_op;
        for code in 0..=BINOP_MAX_CODE {
            let str_from_code = binop_code_to_str(code);
            let via_code = binop_from_code(code).expect("valid code");
            let via_str = map_binary_op(str_from_code).unwrap_or_else(|| {
                panic!(
                    "map_binary_op({str_from_code:?}) returned None \
                     — LP-string table is missing an op that the code table defines"
                )
            });
            assert_eq!(
                via_code, via_str,
                "binop code {code} -> {str_from_code:?}: code table and LP table disagree"
            );
        }
    }

    /// Round-trip: unop_code_to_str agrees with map_unary_op on the same string.
    #[test]
    fn unop_str_round_trip_agrees_with_lp_string_table() {
        use crate::plugins::typed_emission::arena::map_unary_op;
        for code in 0..=UNOP_MAX_CODE {
            let str_from_code = unop_code_to_str(code);
            let via_code = unop_from_code(code).expect("valid code");
            let via_str = map_unary_op(str_from_code).unwrap_or_else(|| {
                panic!(
                    "map_unary_op({str_from_code:?}) returned None \
                     — LP-string table is missing an op that the code table defines"
                )
            });
            assert_eq!(
                via_code, via_str,
                "unop code {code} -> {str_from_code:?}: code table and LP table disagree"
            );
        }
    }
}
