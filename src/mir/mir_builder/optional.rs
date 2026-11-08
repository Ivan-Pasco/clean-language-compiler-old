//! Optional primitives — `integer?`, `number?`, `boolean?` and their sized
//! forms (type-system.md §8, "Representation").
//!
//! Such a value is a pointer to a boxed-any block holding the plain value,
//! `0` for none, so none is distinct from every value (`0`, `0.0`, `false`
//! included). Its MIR type is `MirType::Any`: the box has the boxed-any
//! layout, so an optional primitive stored into an `any` is the same
//! pointer. Boxes are never written after they are made.
//!
//! The type checker wraps every value stored into an optional primitive
//! slot in `__optional_box` (`TypeInference::box_into_optional_primitive`),
//! lowered here. The none tests, `!`, `default` and `==` read the box.

use super::*;
use crate::mir::mir_types::OptionalConversion;

impl MirBuilder {
    /// A fresh temporary local of `local_type`.
    fn optional_temp(
        &self,
        context: &mut FunctionBuildContext,
        local_type: MirType,
        location: &SourceLocation,
    ) -> ValueId {
        let id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, id, local_type, location.clone());
        id
    }

    /// Emit `OptionalConvert` of `value_id` into a new local of `result_type`.
    fn emit_optional_convert(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        conversion: OptionalConversion,
        result_type: MirType,
        location: &SourceLocation,
    ) -> ValueId {
        let result = self.optional_temp(context, result_type, location);
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result),
                operation: MirOperation::OptionalConvert {
                    value: MirOperand::Value(value_id),
                    conversion,
                },
                location: location.clone(),
            },
        );
        result
    }

    /// Lower `__optional_box(value)` of type `target` (an optional
    /// primitive).
    pub(super) fn build_optional_box(
        &mut self,
        context: &mut FunctionBuildContext,
        value: &TastExpression,
        target: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        let value_id = self.build_expression(context, value)?;
        Ok(self.convert_into_optional(context, value_id, &value.expr_type, target, location))
    }

    /// `value_id` (of type `value_type`) as the optional primitive `target`:
    /// a plain value is boxed, an `any` or an optional primitive of another
    /// type converted, and none or a value of the type kept.
    pub(super) fn convert_into_optional(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        value_type: &ConcreteType,
        target: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        let Some(inner) = target.optional_primitive() else {
            return value_id;
        };
        let inner_mir = self.convert_concrete_type(inner);
        let value_mir = context
            .function
            .locals
            .get(&value_id)
            .map(|local| local.local_type.clone())
            .unwrap_or_else(|| self.convert_concrete_type(value_type));
        let conversion = match value_mir {
            // `none` is the null pointer already.
            _ if matches!(value_type, ConcreteType::Null) => return value_id,
            MirType::Ptr(ref pointee) if **pointee == MirType::Void => return value_id,
            MirType::Any if value_type == target => return value_id,
            MirType::Any => OptionalConversion::FromAny { target: inner_mir },
            _ => OptionalConversion::Box { target: inner_mir },
        };
        self.emit_optional_convert(context, value_id, conversion, MirType::Any, location)
    }

    /// The value of the optional primitive `value_id` as the plain
    /// primitive `target`; `0`, `0.0` or `false` when it is none.
    pub(super) fn emit_optional_unbox(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        target: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        let target = self.convert_concrete_type(target);
        self.emit_optional_convert(
            context,
            value_id,
            OptionalConversion::Unbox {
                target: target.clone(),
            },
            target,
            location,
        )
    }

    /// The boxed-any an optional primitive gives to JSON code: its box, or
    /// a null box for none.
    pub(super) fn emit_optional_json_value(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        location: &SourceLocation,
    ) -> ValueId {
        self.emit_optional_convert(
            context,
            value_id,
            OptionalConversion::ToJson,
            MirType::Any,
            location,
        )
    }

    /// An i32 that is 1 when the optional primitive (or `any`) `value_id`
    /// holds a value and 0 when it is none.
    pub(super) fn emit_optional_has_value(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        location: &SourceLocation,
    ) -> ValueId {
        let tag = self.emit_non_null_test(context, value_id, &ConcreteType::Any, location);
        self.optional_binary(
            context,
            MirBinaryOp::Ne,
            MirOperand::Value(tag),
            MirOperand::Constant(MirConstant::Integer(0)),
            location,
        )
    }

    /// `left op right` into a new i32 local.
    fn optional_binary(
        &mut self,
        context: &mut FunctionBuildContext,
        op: MirBinaryOp,
        left: MirOperand,
        right: MirOperand,
        location: &SourceLocation,
    ) -> ValueId {
        let result = self.optional_temp(context, MirType::I32, location);
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result),
                operation: MirOperation::BinaryOp { op, left, right },
                location: location.clone(),
            },
        );
        result
    }

    /// `value!` of an optional primitive: halt when it is none, otherwise
    /// its plain value.
    pub(super) fn build_optional_required(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        inner: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        let has_value = self.emit_optional_has_value(context, value_id, location);
        // `Required` traps on a zero operand.
        let checked = self.optional_temp(context, MirType::I32, location);
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(checked),
                operation: MirOperation::UnaryOp {
                    op: MirUnaryOp::Required,
                    operand: MirOperand::Value(has_value),
                },
                location: location.clone(),
            },
        );
        self.emit_optional_unbox(context, value_id, inner, location)
    }

    /// The MIR type two primitives are compared in: a number when either is
    /// one (an f64), a 64-bit integer when either is one, a boolean when both
    /// are.
    fn comparison_type(a: &MirType, b: &MirType) -> MirType {
        let float = |t: &MirType| matches!(t, MirType::F32 | MirType::F64);
        let wide = |t: &MirType| matches!(t, MirType::I64 | MirType::U64);
        if float(a) || float(b) {
            MirType::F64
        } else if wide(a) || wide(b) {
            MirType::I64
        } else if *a == MirType::Bool && *b == MirType::Bool {
            MirType::Bool
        } else {
            MirType::I32
        }
    }

    /// `value` as a `target`: unboxed when it is an optional primitive,
    /// converted when it is a plain primitive of another MIR type.
    fn optional_comparison_operand(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        expression: &TastExpression,
        target: &MirType,
        location: &SourceLocation,
    ) -> ValueId {
        if expression.expr_type.optional_primitive().is_some() {
            return self.emit_optional_convert(
                context,
                value_id,
                OptionalConversion::Unbox {
                    target: target.clone(),
                },
                target.clone(),
                location,
            );
        }
        let current = context
            .function
            .locals
            .get(&value_id)
            .map(|local| local.local_type.clone())
            .unwrap_or_else(|| self.convert_concrete_type(&expression.expr_type));
        if current == *target {
            return value_id;
        }
        // Box, then read the box as `target`: the conversion covers every
        // pair of primitive widths.
        let boxed = self.emit_optional_convert(
            context,
            value_id,
            OptionalConversion::Box { target: current },
            MirType::Any,
            location,
        );
        self.emit_optional_convert(
            context,
            boxed,
            OptionalConversion::Unbox {
                target: target.clone(),
            },
            target.clone(),
            location,
        )
    }

    /// `left == right`, `left != right` and `left is right` when an operand
    /// is an optional primitive: none equals none only, and two values
    /// compare as values (`integer? n = 0` makes `n == 0` true and
    /// `n == none` false). `None` when no operand is an optional primitive,
    /// or the other operand is neither none, a primitive nor an optional
    /// primitive.
    pub(super) fn build_optional_comparison(
        &mut self,
        context: &mut FunctionBuildContext,
        operator: &BinaryOperator,
        left: &TastExpression,
        right: &TastExpression,
        location: &SourceLocation,
    ) -> Result<Option<ValueId>, Vec<CompilerError>> {
        let negate = match operator {
            BinaryOperator::Equal | BinaryOperator::Is => false,
            BinaryOperator::NotEqual => true,
            _ => return Ok(None),
        };
        let is_none = |e: &TastExpression| {
            matches!(e.expr_type, ConcreteType::Null)
                || matches!(
                    e.kind,
                    TastExpressionKind::Literal {
                        value: TastLiteral::Null
                    }
                )
        };
        let left_optional = left.expr_type.optional_primitive().cloned();
        let right_optional = right.expr_type.optional_primitive().cloned();
        let (optional, other, optional_inner) = match (&left_optional, &right_optional) {
            (Some(inner), _) => (left, right, inner.clone()),
            (None, Some(inner)) => (right, left, inner.clone()),
            (None, None) => return Ok(None),
        };
        let other_is_none = is_none(other);
        if !other_is_none
            && other.expr_type.optional_primitive().is_none()
            && !other.expr_type.is_primitive_value()
        {
            return Ok(None);
        }

        // Evaluate in source order.
        let left_id = self.build_expression(context, left)?;
        let right_id = self.build_expression(context, right)?;
        let (optional_id, other_id) = if std::ptr::eq(optional, left) {
            (left_id, right_id)
        } else {
            (right_id, left_id)
        };

        let has_value = self.emit_optional_has_value(context, optional_id, location);
        let equal = if other_is_none {
            self.optional_binary(
                context,
                MirBinaryOp::Eq,
                MirOperand::Value(has_value),
                MirOperand::Constant(MirConstant::Integer(0)),
                location,
            )
        } else {
            let other_inner = other
                .expr_type
                .optional_primitive()
                .unwrap_or(&other.expr_type)
                .clone();
            let compare_type = Self::comparison_type(
                &self.convert_concrete_type(&optional_inner),
                &self.convert_concrete_type(&other_inner),
            );
            let a = self.optional_comparison_operand(
                context,
                optional_id,
                optional,
                &compare_type,
                location,
            );
            let b =
                self.optional_comparison_operand(context, other_id, other, &compare_type, location);
            let values_equal = self.optional_binary(
                context,
                MirBinaryOp::Eq,
                MirOperand::Value(a),
                MirOperand::Value(b),
                location,
            );
            if other.expr_type.optional_primitive().is_some() {
                // Both may be none: equal when both are none, or both hold
                // equal values.
                let other_has_value = self.emit_optional_has_value(context, other_id, location);
                let same_presence = self.optional_binary(
                    context,
                    MirBinaryOp::Eq,
                    MirOperand::Value(has_value),
                    MirOperand::Value(other_has_value),
                    location,
                );
                let no_value = self.optional_binary(
                    context,
                    MirBinaryOp::Eq,
                    MirOperand::Value(has_value),
                    MirOperand::Constant(MirConstant::Integer(0)),
                    location,
                );
                let none_or_equal = self.optional_binary(
                    context,
                    MirBinaryOp::Or,
                    MirOperand::Value(no_value),
                    MirOperand::Value(values_equal),
                    location,
                );
                self.optional_binary(
                    context,
                    MirBinaryOp::And,
                    MirOperand::Value(same_presence),
                    MirOperand::Value(none_or_equal),
                    location,
                )
            } else {
                self.optional_binary(
                    context,
                    MirBinaryOp::And,
                    MirOperand::Value(has_value),
                    MirOperand::Value(values_equal),
                    location,
                )
            }
        };
        if !negate {
            return Ok(Some(equal));
        }
        Ok(Some(self.optional_binary(
            context,
            MirBinaryOp::Eq,
            MirOperand::Value(equal),
            MirOperand::Constant(MirConstant::Integer(0)),
            location,
        )))
    }
}
