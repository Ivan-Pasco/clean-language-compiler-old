//! Optional primitives: the conversions of `MirOperation::OptionalConvert`.
//!
//! An `integer?`, `number?` or `boolean?` (of any width) is a pointer to a
//! 12-byte boxed-any block holding the plain value, or `0` for none
//! (`foundation/spec/type-system.md` §8, "Representation";
//! `BOXED_ANY_ABI.md` §1-2). The box tag is the value's kind: integers of
//! every width are tag 1 with the full i64 payload, numbers of both widths
//! tag 3 with an f64 payload, booleans tag 2. A box is never written after
//! it is made, so copying the pointer copies the value. Boxes come from the
//! module's own `__malloc`, so no host call is made per store.

use super::*;
use crate::mir::mir_types::OptionalConversion;
use wasm_encoder::MemArg;

/// The WASM value type a MIR primitive is held in.
fn value_kind(t: &MirType) -> ValType {
    match t {
        MirType::I64 | MirType::U64 => ValType::I64,
        MirType::F32 => ValType::F32,
        MirType::F64 => ValType::F64,
        _ => ValType::I32,
    }
}

/// The `any` conversion that reads a `target` value out of a box, and the
/// MIR type it leaves.
fn unbox_target(target: &MirType) -> (AnyUnboxTarget, MirType) {
    match target {
        MirType::Bool => (AnyUnboxTarget::Bool, MirType::Bool),
        MirType::F32 | MirType::F64 => (AnyUnboxTarget::F64, MirType::F64),
        MirType::I64 | MirType::U64 => (AnyUnboxTarget::I64, MirType::I64),
        _ => (AnyUnboxTarget::I32, MirType::I32),
    }
}

impl MirCodeGenerator<'_> {
    /// Emit `MirOperation::OptionalConvert` and store its result in `dest`.
    pub(super) fn emit_optional_convert(
        &mut self,
        value: &MirOperand,
        conversion: &OptionalConversion,
        dest: Option<ValueId>,
    ) -> Result<(), CompilerError> {
        let source_type = self.get_operand_mir_type(value).unwrap_or(MirType::I32);
        self.load_operand(value)?;
        match conversion {
            OptionalConversion::Box { target } => {
                self.emit_primitive_conversion(&source_type, target);
                self.emit_optional_box(target)?;
            }
            OptionalConversion::FromAny { target } => {
                // none for a null pointer or a null box; otherwise convert
                // the value as an `any` converts to `target`, and box it.
                let ptr = self.alloc_temp_local(ValType::I32);
                self.current_instructions.extend([
                    Instruction::LocalSet(ptr),
                    Instruction::LocalGet(ptr),
                    Instruction::If(BlockType::Result(ValType::I32)),
                    Instruction::LocalGet(ptr),
                    Instruction::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::If(BlockType::Result(ValType::I32)),
                    Instruction::LocalGet(ptr),
                ]);
                let (unbox, unboxed) = unbox_target(target);
                self.emit_any_unbox(unbox)?;
                self.emit_primitive_conversion(&unboxed, target);
                self.emit_optional_box(target)?;
                self.current_instructions.extend([
                    Instruction::Else,
                    Instruction::I32Const(0),
                    Instruction::End,
                    Instruction::Else,
                    Instruction::I32Const(0),
                    Instruction::End,
                ]);
            }
            OptionalConversion::Unbox { target } => {
                let (unbox, unboxed) = unbox_target(target);
                self.emit_any_unbox(unbox)?;
                self.emit_primitive_conversion(&unboxed, target);
            }
            OptionalConversion::ToJson => {
                let ptr = self.alloc_temp_local(ValType::I32);
                self.current_instructions.extend([
                    Instruction::LocalTee(ptr),
                    Instruction::If(BlockType::Result(ValType::I32)),
                    Instruction::LocalGet(ptr),
                    Instruction::Else,
                    Instruction::I32Const(0),
                ]);
                self.emit_box_value_in(AnyTypeTag::Null, &MirType::I32, BoxAllocator::Native)?;
                self.current_instructions.push(Instruction::End);
            }
        }
        match dest {
            Some(dest) => self.store_to_local(dest)?,
            None => self.current_instructions.push(Instruction::Drop),
        }
        Ok(())
    }

    /// With a `target` value on the stack, box it: leave the box pointer.
    fn emit_optional_box(&mut self, target: &MirType) -> Result<(), CompilerError> {
        let (tag, source) = match target {
            MirType::Bool => (AnyTypeTag::Boolean, MirType::Bool),
            MirType::F32 => (AnyTypeTag::Number, MirType::F32),
            MirType::F64 => (AnyTypeTag::Number, MirType::F64),
            // The i64 payload holds the bits of an `integer:64u` as well.
            MirType::I64 | MirType::U64 => (AnyTypeTag::Integer, MirType::I64),
            other => (AnyTypeTag::Integer, other.clone()),
        };
        self.emit_box_value_in(tag, &source, BoxAllocator::Native)
    }

    /// Convert the primitive on the stack from `from` to `to`: integers
    /// widen (sign-extending, zero-extending the unsigned 32-bit form) or
    /// wrap, numbers truncate toward zero and saturate when they become
    /// integers, and a sized integer keeps only its own bits.
    fn emit_primitive_conversion(&mut self, from: &MirType, to: &MirType) {
        if from == to {
            return;
        }
        let unsigned32 = matches!(from, MirType::U8 | MirType::U16 | MirType::U32);
        let instruction = match (value_kind(from), value_kind(to)) {
            (ValType::I32, ValType::I64) if unsigned32 => Some(Instruction::I64ExtendI32U),
            (ValType::I32, ValType::I64) => Some(Instruction::I64ExtendI32S),
            (ValType::I32, ValType::F64) if unsigned32 => Some(Instruction::F64ConvertI32U),
            (ValType::I32, ValType::F64) => Some(Instruction::F64ConvertI32S),
            (ValType::I32, ValType::F32) if unsigned32 => Some(Instruction::F32ConvertI32U),
            (ValType::I32, ValType::F32) => Some(Instruction::F32ConvertI32S),
            (ValType::I64, ValType::I32) => Some(Instruction::I32WrapI64),
            (ValType::I64, ValType::F64) if *from == MirType::U64 => {
                Some(Instruction::F64ConvertI64U)
            }
            (ValType::I64, ValType::F64) => Some(Instruction::F64ConvertI64S),
            (ValType::I64, ValType::F32) if *from == MirType::U64 => {
                Some(Instruction::F32ConvertI64U)
            }
            (ValType::I64, ValType::F32) => Some(Instruction::F32ConvertI64S),
            (ValType::F64, ValType::I32) => Some(Instruction::I32TruncSatF64S),
            (ValType::F64, ValType::I64) => Some(Instruction::I64TruncSatF64S),
            (ValType::F64, ValType::F32) => Some(Instruction::F32DemoteF64),
            (ValType::F32, ValType::F64) => Some(Instruction::F64PromoteF32),
            (ValType::F32, ValType::I32) => Some(Instruction::I32TruncSatF32S),
            (ValType::F32, ValType::I64) => Some(Instruction::I64TruncSatF32S),
            _ => None,
        };
        if let Some(instruction) = instruction {
            self.current_instructions.push(instruction);
        }
        // A sized integer below 32 bits holds only its own bits,
        // sign-extended for the signed forms.
        let narrow: &[Instruction<'static>] = match to {
            MirType::I8 => &[
                Instruction::I32Const(24),
                Instruction::I32Shl,
                Instruction::I32Const(24),
                Instruction::I32ShrS,
            ],
            MirType::I16 => &[
                Instruction::I32Const(16),
                Instruction::I32Shl,
                Instruction::I32Const(16),
                Instruction::I32ShrS,
            ],
            MirType::U8 => &[Instruction::I32Const(0xFF), Instruction::I32And],
            MirType::U16 => &[Instruction::I32Const(0xFFFF), Instruction::I32And],
            MirType::Bool if value_kind(from) == ValType::I32 => {
                &[Instruction::I32Const(0), Instruction::I32Ne]
            }
            _ => &[],
        };
        self.current_instructions.extend_from_slice(narrow);
    }
}
