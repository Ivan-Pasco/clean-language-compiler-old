//! Type helpers — type conversion and type mapping utilities.

use super::*;

impl MirBuilder {
    /// Convert ConcreteType to MirType
    pub(super) fn convert_concrete_type(&self, concrete_type: &ConcreteType) -> MirType {
        MirType::from_concrete_type(concrete_type)
    }

    /// Get the AnyTypeTag for a given ConcreteType
    pub(super) fn get_any_type_tag(concrete_type: &ConcreteType) -> AnyTypeTag {
        match concrete_type {
            ConcreteType::Integer => AnyTypeTag::Integer,
            ConcreteType::Number => AnyTypeTag::Number,
            ConcreteType::Boolean => AnyTypeTag::Boolean,
            ConcreteType::String => AnyTypeTag::String,
            ConcreteType::Array(_) | ConcreteType::Matrix(_) => AnyTypeTag::List,
            ConcreteType::Class { .. } | ConcreteType::Interface { .. } => AnyTypeTag::Object,
            ConcreteType::Null | ConcreteType::Undefined => AnyTypeTag::Null,
            // For any other types, default to Integer (they'll be stored as i32)
            _ => AnyTypeTag::Integer,
        }
    }

    /// Box a value to any type - emits a BoxAny instruction.
    ///
    /// For native Clean collections (`List<T>` / `Pairs<K,V>`) the raw value
    /// pointer can't be stored directly in the box: `__json_stringify_value`
    /// and friends expect their `List`-tagged and `Object`-tagged payloads
    /// to follow the JSON-tree layout (`[count][boxed_elem]…` /
    /// `[count][key,boxed_val]…`), but Clean's typed-collection heap stores
    /// raw elements at a different stride. Normalize the value at box time
    /// by routing it through `__json_from_cln_list` / `__json_from_cln_pairs`
    /// — the box's payload is then a real JSON tree node, and every Any
    /// consumer (json.encode, json.dataToText, embedded `any` object
    /// literals) sees the same canonical representation.
    pub(super) fn emit_box_any(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        source_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        let type_tag = Self::get_any_type_tag(source_type);
        let mir_source_type = self.convert_concrete_type(source_type);

        // Normalize raw Clean collections into the JSON-tree layout before
        // boxing. The converter result is stored in a fresh local (i32) and
        // becomes the BoxAny payload. For primitive sources we skip this step.
        let normalized_value_id = match source_type {
            ConcreteType::Array(elem_type) => {
                let elem_tag = Self::get_any_type_tag(elem_type) as i64;
                self.emit_cln_collection_to_json(
                    context,
                    value_id,
                    "__json_from_cln_list",
                    elem_tag,
                    location,
                )
            }
            ConcreteType::Pairs(_key_type, val_type) => {
                let val_tag = Self::get_any_type_tag(val_type) as i64;
                self.emit_cln_collection_to_json(
                    context,
                    value_id,
                    "__json_from_cln_pairs",
                    val_tag,
                    location,
                )
            }
            _ => value_id,
        };

        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;

        // Register the result as a temp local with Any type (boxed value)
        // NOTE: Boxed values must have MirType::Any, not MirType::I32
        // This ensures proper type tracking for Any variables
        self.register_temp_local(context, result_id, MirType::Any, location.clone());

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation: MirOperation::BoxAny {
                value: MirOperand::Value(normalized_value_id),
                type_tag,
                source_type: mir_source_type,
            },
            location: location.clone(),
        };

        self.add_instruction(context, instruction);
        result_id
    }

    /// Emit a Call to one of the Clean-collection→JSON-tree converters
    /// (`__json_from_cln_list` / `__json_from_cln_pairs`) and return the
    /// ValueId of the converted JSON tree pointer.
    fn emit_cln_collection_to_json(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        converter_name: &str,
        type_tag: i64,
        location: &SourceLocation,
    ) -> ValueId {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, MirType::I32, location.clone());

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation: MirOperation::Call {
                function: MirOperand::NamedFunction {
                    name: converter_name.to_string(),
                    symbol_id: SymbolId(0),
                },
                arguments: vec![
                    MirOperand::Value(value_id),
                    MirOperand::Constant(MirConstant::Integer(type_tag)),
                ],
            },
            location: location.clone(),
        };
        self.add_instruction(context, instruction);
        result_id
    }

    /// Unbox an any value to a specific type - emits an UnboxAny instruction
    pub(super) fn emit_unbox_any(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        target_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;

        let mir_target_type = self.convert_concrete_type(target_type);
        self.register_temp_local(
            context,
            result_id,
            mir_target_type.clone(),
            location.clone(),
        );

        let operation = match target_type {
            ConcreteType::Number => MirOperation::UnboxAnyToF64 {
                value: MirOperand::Value(value_id),
            },
            // For Integer, Boolean, String, and most other types, use i32 unboxing
            _ => MirOperation::UnboxAnyToI32 {
                value: MirOperand::Value(value_id),
            },
        };

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation,
            location: location.clone(),
        };

        self.add_instruction(context, instruction);
        result_id
    }

    /// Convert a boxed `any` value to a string via runtime type-tag dispatch.
    ///
    /// Unlike `emit_unbox_any` with a String target — which would emit a raw
    /// `UnboxAnyToI32` that just reads offset 4 of the any-struct as an i32 —
    /// this dispatches on the type tag and produces an actual length-prefixed
    /// string pointer regardless of whether the contained value was originally
    /// a string, number, integer, or boolean. This is the correct operand for
    /// `string_compare` when comparing `any` against a string literal.
    ///
    /// Result is registered as `Ptr(U8)` (the canonical string pointer type
    /// used elsewhere for host-function string results).
    pub(super) fn emit_any_to_string(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        location: &SourceLocation,
    ) -> ValueId {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;

        self.register_temp_local(
            context,
            result_id,
            MirType::Ptr(Box::new(MirType::U8)),
            location.clone(),
        );

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation: MirOperation::AnyToString {
                value: MirOperand::Value(value_id),
            },
            location: location.clone(),
        };

        self.add_instruction(context, instruction);
        result_id
    }

    /// Convert TAST literal to MIR constant. `expr_type` is the type the typechecker assigned
    /// to the surrounding expression — used to decide whether an integer literal should be
    /// emitted as 32-bit (`MirConstant::Integer`) or 64-bit (`MirConstant::Integer64`).
    /// Without this routing the codegen narrows every literal to i32 in `load_constant`,
    /// silently truncating values destined for `integer:64` slots.
    pub(super) fn convert_literal(
        &mut self,
        literal: &TastLiteral,
        expr_type: &ConcreteType,
    ) -> MirConstant {
        match literal {
            TastLiteral::Integer(i) => {
                if Self::is_i64_target(expr_type) {
                    MirConstant::Integer64(*i)
                } else {
                    MirConstant::Integer(*i)
                }
            }
            TastLiteral::Number(f) => MirConstant::Float(*f),
            TastLiteral::String(s) => {
                let index = self.get_string_index(s.clone());
                MirConstant::String(index)
            }
            TastLiteral::Boolean(b) => MirConstant::Boolean(*b),
            TastLiteral::Null => MirConstant::Null,
            TastLiteral::Undefined => MirConstant::Undefined,
        }
    }

    /// Convert TAST literal to its corresponding MIR type, using the surrounding expression's
    /// type to widen integer literals to i64 when appropriate.
    pub(super) fn convert_literal_type(
        &self,
        literal: &TastLiteral,
        expr_type: &ConcreteType,
    ) -> MirType {
        match literal {
            TastLiteral::Integer(_) => {
                if Self::is_i64_target(expr_type) {
                    MirType::I64
                } else {
                    MirType::I32 // Default integer type
                }
            }
            TastLiteral::Number(_) => MirType::F64, // Default float type
            TastLiteral::String(_) => MirType::Ptr(Box::new(MirType::I8)), // String as i8 pointer
            TastLiteral::Boolean(_) => MirType::Bool,
            TastLiteral::Null => MirType::Ptr(Box::new(MirType::Void)),
            TastLiteral::Undefined => MirType::Void,
        }
    }

    /// True when the expression's inferred type indicates a 64-bit signed integer destination.
    fn is_i64_target(expr_type: &ConcreteType) -> bool {
        matches!(
            expr_type,
            ConcreteType::IntegerSized {
                bits: 64,
                unsigned: false
            }
        )
    }

    /// Register a ValueId as a temporary local for codegen
    pub(super) fn register_temp_local(
        &self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        mir_type: MirType,
        location: SourceLocation,
    ) {
        let local = MirLocal {
            name: None, // Temporary values don't need names
            local_type: mir_type,
            is_mutable: false, // Temporary results are immutable
            location,
        };
        context.function.locals.insert(value_id, local);
    }

    /// Convert a value to string for print() calls
    /// Returns the ValueId of the string result
    pub(super) fn convert_value_to_string(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        value_type: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        use crate::typechecker::tast::ConcreteType;

        match value_type {
            ConcreteType::String => {
                // Already a string, use directly
                Ok(value_id)
            }
            ConcreteType::Integer => {
                // Convert integer to string using int_to_string
                let converted_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register as Ptr(U8) - int_to_string returns a string pointer
                self.register_temp_local(
                    context,
                    converted_id,
                    MirType::Ptr(Box::new(MirType::U8)),
                    location.clone(),
                );

                let symbol_id = self
                    .symbol_table
                    .lookup_symbol("int_to_string")
                    .unwrap_or_else(|| {
                        warn!("int_to_string not found in symbol table, using SymbolId(166)");
                        SymbolId(166)
                    });

                let conversion_instruction = MirInstruction {
                    dest: Some(converted_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(symbol_id),
                        arguments: vec![MirOperand::Value(value_id)],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, conversion_instruction);
                Ok(converted_id)
            }
            ConcreteType::Number => {
                // Convert float to string using float_to_string
                let converted_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register as Ptr(U8) - float_to_string returns a string pointer
                self.register_temp_local(
                    context,
                    converted_id,
                    MirType::Ptr(Box::new(MirType::U8)),
                    location.clone(),
                );

                let symbol_id = self
                    .symbol_table
                    .lookup_symbol("float_to_string")
                    .unwrap_or_else(|| {
                        warn!("float_to_string not found in symbol table, using SymbolId(167)");
                        SymbolId(167)
                    });

                let conversion_instruction = MirInstruction {
                    dest: Some(converted_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(symbol_id),
                        arguments: vec![MirOperand::Value(value_id)],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, conversion_instruction);
                Ok(converted_id)
            }
            ConcreteType::Boolean => {
                // Convert boolean to string using bool_to_string
                let converted_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register as Ptr(U8) - bool_to_string returns a string pointer
                self.register_temp_local(
                    context,
                    converted_id,
                    MirType::Ptr(Box::new(MirType::U8)),
                    location.clone(),
                );

                let symbol_id = self
                    .symbol_table
                    .lookup_symbol("bool_to_string")
                    .unwrap_or_else(|| {
                        warn!("bool_to_string not found in symbol table, using SymbolId(165)");
                        SymbolId(165)
                    });

                let conversion_instruction = MirInstruction {
                    dest: Some(converted_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(symbol_id),
                        arguments: vec![MirOperand::Value(value_id)],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, conversion_instruction);
                Ok(converted_id)
            }
            _ => {
                // For other types (objects, arrays, etc.), use the value as-is for now
                // In a complete implementation, these would also have toString() methods
                Ok(value_id)
            }
        }
    }

    /// Convert MirType back to ConcreteType for type inference
    /// This is the inverse of MirType::from_concrete_type()
    pub(super) fn mir_type_to_concrete(mir_type: &MirType) -> ConcreteType {
        match mir_type {
            MirType::I32 => ConcreteType::Integer,
            MirType::F64 => ConcreteType::Number,
            MirType::Bool => ConcreteType::Boolean,
            MirType::Void => ConcreteType::Undefined,
            MirType::Ptr(inner) => {
                match **inner {
                    MirType::I8 => ConcreteType::String,
                    // Ptr(U8) is used for string pointers returned from host functions
                    // (e.g., substring, trim, toString conversions). These are strings,
                    // not null values. This is critical for correct method chaining:
                    // e.g., line.substring(...).trim() — the receiver of .trim() is Ptr(U8).
                    MirType::U8 => ConcreteType::String,
                    MirType::Void => ConcreteType::Null,
                    _ => ConcreteType::Null, // Fallback for other pointer types
                }
            }
            MirType::StringTuple => ConcreteType::String,
            MirType::Function {
                parameters,
                return_type,
            } => ConcreteType::Function {
                parameters: parameters.iter().map(Self::mir_type_to_concrete).collect(),
                return_type: Box::new(Self::mir_type_to_concrete(return_type)),
                is_background: false,
            },
            // For types that can't be precisely converted back, use safe defaults
            MirType::I8 | MirType::I16 | MirType::I64 => ConcreteType::Integer,
            MirType::U8 | MirType::U16 | MirType::U32 | MirType::U64 => ConcreteType::Integer,
            MirType::F32 => ConcreteType::Number,
            MirType::Array(_, _) => ConcreteType::Array(Box::new(ConcreteType::Integer)),
            MirType::Struct(_) => ConcreteType::Null,
            MirType::Any => ConcreteType::Any, // Boxed any type
        }
    }

    /// Infer the result type of a binary operation
    pub(super) fn infer_binary_operation_type(
        &self,
        left_type: &ConcreteType,
        right_type: &ConcreteType,
        operator: &BinaryOperator,
    ) -> MirType {
        trace!(
            operator = ?operator,
            left_type = ?left_type,
            right_type = ?right_type,
            "Binary operation type inference"
        );

        // NOTE: Comparison and logical operations always return i32 (boolean)
        match operator {
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::LessThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::LessThanOrEqual
            | BinaryOperator::GreaterThanOrEqual
            | BinaryOperator::And
            | BinaryOperator::Or => {
                trace!("Comparison/logical op -> I32");
                return MirType::I32; // Boolean result
            }
            _ => {}
        }

        // For arithmetic and other operations, infer from operand types
        let result = match (left_type, right_type) {
            // Arithmetic operations between numeric types
            (ConcreteType::Integer, ConcreteType::Integer) => MirType::I32,
            (ConcreteType::Number, ConcreteType::Number) => MirType::F64,
            (ConcreteType::Number, ConcreteType::Integer) => MirType::F64,
            (ConcreteType::Integer, ConcreteType::Number) => MirType::F64,

            // Boolean operations
            (ConcreteType::Boolean, ConcreteType::Boolean) => MirType::Bool,

            // String operations (concatenation) - result is string
            // NOTE: Strings are i32 pointers to [len|content] structure in memory
            (ConcreteType::String, ConcreteType::String) => MirType::I32,
            (ConcreteType::String, _) => MirType::I32, // String + any = String
            (_, ConcreteType::String) => MirType::I32, // Any + String = String

            // Array operations (if supported) - result is array pointer
            (ConcreteType::Array(elem_type), ConcreteType::Array(_)) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Same type operations - use the type's MIR representation
            (left, right) if left == right => {
                // Same type operations - use the type's MIR representation
                MirType::from_concrete_type(left)
            }

            // Mixed types or unknown - use left operand type as fallback
            // This handles cases like Class operations, Function operations, etc.
            (left, _) => MirType::from_concrete_type(left),
        };
        trace!(result = ?result, "Type inference result");
        result
    }

    /// Infer the result type of a unary operation
    pub(super) fn infer_unary_operation_type(&self, operand_type: &ConcreteType) -> MirType {
        match operand_type {
            // Numeric operations preserve type
            ConcreteType::Integer => MirType::I32,
            ConcreteType::Number => MirType::F64,

            // Boolean operations preserve type
            ConcreteType::Boolean => MirType::Bool,

            // String operations preserve type
            // Use Ptr(I8) consistent with from_concrete_type and string literal types.
            // Using I32 here caused load_string_argument_for_print to treat the string pointer as
            // an integer and call int_to_string on it, producing garbage memory address output.
            ConcreteType::String => MirType::Ptr(Box::new(MirType::I8)),

            // Array operations preserve pointer type
            ConcreteType::Array(elem_type) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Matrix operations preserve pointer type
            ConcreteType::Matrix(elem_type) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Function, Class, and other complex types - use from_concrete_type
            // This handles all remaining ConcreteType variants properly
            other => MirType::from_concrete_type(other),
        }
    }

    /// Convert TAST binary operator to MIR binary operator
    ///
    /// Note: Some operators cannot be directly represented in MIR and should be
    /// handled specially in build_expression (Power, Concatenate).
    pub(super) fn convert_binary_op(&self, op: &BinaryOperator) -> MirBinaryOp {
        match op {
            // Arithmetic operators
            BinaryOperator::Add => MirBinaryOp::Add,
            BinaryOperator::Subtract => MirBinaryOp::Sub,
            BinaryOperator::Multiply => MirBinaryOp::Mul,
            BinaryOperator::Divide => MirBinaryOp::Div,
            BinaryOperator::Modulo => MirBinaryOp::Rem,

            // Comparison operators
            BinaryOperator::Equal => MirBinaryOp::Eq,
            BinaryOperator::NotEqual => MirBinaryOp::Ne,
            BinaryOperator::LessThan => MirBinaryOp::Lt,
            BinaryOperator::GreaterThan => MirBinaryOp::Gt,
            BinaryOperator::LessThanOrEqual => MirBinaryOp::Le,
            BinaryOperator::GreaterThanOrEqual => MirBinaryOp::Ge,
            BinaryOperator::Is => MirBinaryOp::Eq, // Identity is equality for value types
            BinaryOperator::IsNot => MirBinaryOp::Ne, // IsNot is not-equal for value types

            // Logical operators (And/Or work on booleans, lowered to i32.and/i32.or in WASM)
            // The type system ensures these are used on boolean types
            BinaryOperator::And => MirBinaryOp::And,
            BinaryOperator::Or => MirBinaryOp::Or,

            // Bitwise operators (And/Or work on integers, lowered to i32.and/i32.or in WASM)
            // The type system ensures these are used on integer types
            BinaryOperator::BitwiseAnd => MirBinaryOp::And,
            BinaryOperator::BitwiseOr => MirBinaryOp::Or,
            BinaryOperator::BitwiseXor => MirBinaryOp::Xor,
            BinaryOperator::LeftShift => MirBinaryOp::Shl,
            BinaryOperator::RightShift => MirBinaryOp::Shr,

            // CRITICAL: These operators should NEVER reach here - they must be handled in build_expression
            BinaryOperator::Power => {
                panic!("BUG: Power operator should be handled in build_expression as runtime function call, not converted to MIR operator")
            }
            BinaryOperator::Concatenate => {
                panic!("BUG: String concatenation should be handled in build_expression as string.concat call, not converted to MIR operator")
            }
            // BOOK: null-coalescing - NullCoalesce must be handled in build_expression with select instruction
            BinaryOperator::NullCoalesce => {
                panic!("BUG: NullCoalesce operator should be handled in build_expression with select instruction, not converted to MIR operator")
            }
        }
    }

    /// Convert TAST unary operator to MIR unary operator
    ///
    /// Note: Some unary operators (Plus, Increment, Decrement) cannot be directly
    /// represented in MIR and should be handled specially in build_expression.
    pub(super) fn convert_unary_op(&self, op: &UnaryOperator) -> MirUnaryOp {
        match op {
            // Direct unary operators
            UnaryOperator::Negate => MirUnaryOp::Neg,
            UnaryOperator::Not => MirUnaryOp::Not,
            UnaryOperator::BitwiseNot => MirUnaryOp::BitNot,
            // BOOK: required-operator - Postfix ! assertion for null check
            UnaryOperator::Required => MirUnaryOp::Required,

            // CRITICAL: These operators should NEVER reach here - they must be handled in build_expression
            UnaryOperator::Plus => {
                panic!("BUG: Unary plus should be handled in build_expression as no-op, not converted to MIR operator")
            }
            UnaryOperator::PreIncrement => {
                panic!("BUG: Pre-increment should be desugared to assignment (x = x + 1) in build_expression, not converted to MIR operator")
            }
            UnaryOperator::PostIncrement => {
                panic!("BUG: Post-increment should be desugared to assignment (temp = x; x = x + 1; temp) in build_expression, not converted to MIR operator")
            }
            UnaryOperator::PreDecrement => {
                panic!("BUG: Pre-decrement should be desugared to assignment (x = x - 1) in build_expression, not converted to MIR operator")
            }
            UnaryOperator::PostDecrement => {
                panic!("BUG: Post-decrement should be desugared to assignment (temp = x; x = x - 1; temp) in build_expression, not converted to MIR operator")
            }
        }
    }

    /// Get or create string pool index
    pub(super) fn get_string_index(&mut self, string: String) -> usize {
        if let Some(&index) = self.string_indices.get(&string) {
            index
        } else {
            let index = self.string_pool.len();
            self.string_pool.push(string.clone());
            self.string_indices.insert(string, index);
            index
        }
    }
}
