//! Conversions out of a boxed `any` value.
//!
//! A boxed `any` is a 12-byte block `[tag:i32][payload:8 bytes]`
//! (`foundation/spec/platform/BOXED_ANY_ABI.md` §1-3):
//!
//! | tag | payload at offset 4                         |
//! |-----|---------------------------------------------|
//! | 0   | null — unused                               |
//! | 1   | integer — the full i64                      |
//! | 2   | boolean — i32 0 or 1                        |
//! | 3   | number — the f64                            |
//! | 4   | string — i32 pointer to a length-prefixed string |
//! | 5   | list — i32 pointer to a JSON-tree array     |
//! | 6   | object — i32 pointer to a JSON-tree object or a class instance |
//!
//! Every conversion reads the tag once and dispatches through
//! [`MirCodeGenerator::emit_any_tag_switch`]. A null pointer (what `json.get`
//! and other `any`-returning bridges give on a miss) reads as the null tag, so
//! no conversion dereferences address 0.
//!
//! Conversions follow `foundation/spec/type-system.md` §7 and
//! `stdlib-reference.md` §3: numbers truncate toward zero and saturate at the
//! integer range (`trunc_sat`, so NaN and out-of-range values never trap),
//! booleans convert to 0/1, strings are parsed, and null converts to zero.
//! A list or object has no numeric or boolean value; converting one traps
//! with a message naming the target type.

use super::*;
use wasm_encoder::{Instruction, MemArg, ValType};

/// The type an `any` value is converted to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AnyUnboxTarget {
    /// `integer`, and every i32-represented target (lists, pairs and other
    /// pointers): a list or object yields its payload pointer.
    I32,
    /// `integer:64`.
    I64,
    /// `number`.
    F64,
    /// `boolean`.
    Bool,
}

impl AnyUnboxTarget {
    fn val_type(self) -> ValType {
        match self {
            AnyUnboxTarget::I32 | AnyUnboxTarget::Bool => ValType::I32,
            AnyUnboxTarget::I64 => ValType::I64,
            AnyUnboxTarget::F64 => ValType::F64,
        }
    }

    fn type_name(self) -> &'static str {
        match self {
            AnyUnboxTarget::I32 => "integer",
            AnyUnboxTarget::I64 => "integer:64",
            AnyUnboxTarget::F64 => "number",
            AnyUnboxTarget::Bool => "boolean",
        }
    }
}

/// `local.get ptr; <load> offset=4` — read the payload of the box in `ptr`.
fn payload(ptr_local: u32, load: fn(MemArg) -> Instruction<'static>) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(ptr_local),
        load(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }),
    ]
}

fn i32_payload(ptr_local: u32) -> Vec<Instruction<'static>> {
    payload(ptr_local, Instruction::I32Load)
}

fn i64_payload(ptr_local: u32) -> Vec<Instruction<'static>> {
    payload(ptr_local, Instruction::I64Load)
}

fn f64_payload(ptr_local: u32) -> Vec<Instruction<'static>> {
    payload(ptr_local, Instruction::F64Load)
}

fn then(
    mut instructions: Vec<Instruction<'static>>,
    more: &[Instruction<'static>],
) -> Vec<Instruction<'static>> {
    instructions.extend_from_slice(more);
    instructions
}

impl MirCodeGenerator<'_> {
    /// Look up a runtime function that a conversion calls.
    fn any_runtime_function(&self, name: &str) -> Result<u32, CompilerError> {
        self.wasm_generator
            .function_map
            .get(name)
            .copied()
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    format!("'{name}' is not registered; it is needed to convert an `any` value"),
                    None,
                    None,
                )
            })
    }

    /// Instructions that print `message` and trap.
    fn trap_with_message(
        &mut self,
        message: &str,
    ) -> Result<Vec<Instruction<'static>>, CompilerError> {
        let printl = self.any_runtime_function("printl")?;
        let base = self.wasm_generator.get_or_create_string_offset(message)?;
        Ok(vec![
            Instruction::I32Const(base as i32 + 4),
            Instruction::I32Const(message.len() as i32),
            Instruction::Call(printl),
            Instruction::Unreachable,
        ])
    }

    /// Emit a dispatch on the tag of the boxed `any` in `ptr_local`.
    ///
    /// Each arm runs for its tag and leaves one `result` value; `default`
    /// runs for every tag without an arm. A null pointer is dispatched as
    /// the null tag.
    pub(super) fn emit_any_tag_switch(
        &mut self,
        ptr_local: u32,
        result: ValType,
        arms: Vec<(AnyTypeTag, Vec<Instruction<'static>>)>,
        default: Vec<Instruction<'static>>,
    ) {
        let tag_local = self.alloc_temp_local(ValType::I32);
        let result = BlockType::Result(result);
        self.current_instructions.extend([
            Instruction::LocalGet(ptr_local),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(AnyTypeTag::Null.as_i32()),
            Instruction::Else,
            Instruction::LocalGet(ptr_local),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::End,
            Instruction::LocalSet(tag_local),
        ]);
        let arm_count = arms.len();
        for (tag, body) in arms {
            self.current_instructions.extend([
                Instruction::LocalGet(tag_local),
                Instruction::I32Const(tag.as_i32()),
                Instruction::I32Eq,
                Instruction::If(result),
            ]);
            self.current_instructions.extend(body);
            self.current_instructions.push(Instruction::Else);
        }
        self.current_instructions.extend(default);
        for _ in 0..arm_count {
            self.current_instructions.push(Instruction::End);
        }
    }

    /// With a boxed `any` pointer on the stack, convert it to `target`.
    pub(super) fn emit_any_unbox(&mut self, target: AnyUnboxTarget) -> Result<(), CompilerError> {
        let ptr = self.alloc_temp_local(ValType::I32);
        self.current_instructions.push(Instruction::LocalSet(ptr));

        let not_convertible = if target == AnyUnboxTarget::I32 {
            Vec::new()
        } else {
            self.trap_with_message(&format!(
                "runtime error: a list or object held in `any` cannot be converted to {}",
                target.type_name()
            ))?
        };
        let invalid_tag =
            self.trap_with_message("runtime error: invalid `any` value (unknown type tag)")?;

        let arms = match target {
            AnyUnboxTarget::I32 => {
                let string_to_int = self.any_runtime_function("__string_to_int")?;
                vec![
                    (AnyTypeTag::Null, vec![Instruction::I32Const(0)]),
                    // The low word of the i64 (wraps, like `integer:64` → `integer`).
                    (AnyTypeTag::Integer, i32_payload(ptr)),
                    (AnyTypeTag::Boolean, i32_payload(ptr)),
                    (
                        AnyTypeTag::Number,
                        then(f64_payload(ptr), &[Instruction::I32TruncSatF64S]),
                    ),
                    (
                        AnyTypeTag::String,
                        then(i32_payload(ptr), &[Instruction::Call(string_to_int)]),
                    ),
                    // An i32 target also receives lists, pairs and instances:
                    // the payload is the pointer.
                    (AnyTypeTag::List, i32_payload(ptr)),
                    (AnyTypeTag::Object, i32_payload(ptr)),
                ]
            }
            AnyUnboxTarget::I64 => {
                let string_to_int = self.any_runtime_function("__string_to_int")?;
                vec![
                    (AnyTypeTag::Null, vec![Instruction::I64Const(0)]),
                    (AnyTypeTag::Integer, i64_payload(ptr)),
                    (
                        AnyTypeTag::Boolean,
                        then(i32_payload(ptr), &[Instruction::I64ExtendI32U]),
                    ),
                    (
                        AnyTypeTag::Number,
                        then(f64_payload(ptr), &[Instruction::I64TruncSatF64S]),
                    ),
                    (
                        AnyTypeTag::String,
                        then(
                            i32_payload(ptr),
                            &[Instruction::Call(string_to_int), Instruction::I64ExtendI32S],
                        ),
                    ),
                    (AnyTypeTag::List, not_convertible.clone()),
                    (AnyTypeTag::Object, not_convertible),
                ]
            }
            AnyUnboxTarget::F64 => {
                let string_to_float = self.any_runtime_function("string_to_float")?;
                vec![
                    (AnyTypeTag::Null, vec![Instruction::F64Const(0.0)]),
                    (
                        AnyTypeTag::Integer,
                        then(i64_payload(ptr), &[Instruction::F64ConvertI64S]),
                    ),
                    (
                        AnyTypeTag::Boolean,
                        then(i32_payload(ptr), &[Instruction::F64ConvertI32U]),
                    ),
                    (AnyTypeTag::Number, f64_payload(ptr)),
                    (
                        AnyTypeTag::String,
                        then(i32_payload(ptr), &[Instruction::Call(string_to_float)]),
                    ),
                    (AnyTypeTag::List, not_convertible.clone()),
                    (AnyTypeTag::Object, not_convertible),
                ]
            }
            AnyUnboxTarget::Bool => {
                // "true" and "1" are true (stdlib-reference.md §3).
                let string_to_boolean = self.any_runtime_function("string.toBoolean")?;
                vec![
                    (AnyTypeTag::Null, vec![Instruction::I32Const(0)]),
                    (
                        AnyTypeTag::Integer,
                        then(
                            i64_payload(ptr),
                            &[Instruction::I64Const(0), Instruction::I64Ne],
                        ),
                    ),
                    (
                        AnyTypeTag::Boolean,
                        then(
                            i32_payload(ptr),
                            &[Instruction::I32Const(0), Instruction::I32Ne],
                        ),
                    ),
                    (
                        AnyTypeTag::Number,
                        then(
                            f64_payload(ptr),
                            &[Instruction::F64Const(0.0), Instruction::F64Ne],
                        ),
                    ),
                    (
                        AnyTypeTag::String,
                        then(i32_payload(ptr), &[Instruction::Call(string_to_boolean)]),
                    ),
                    (AnyTypeTag::List, not_convertible.clone()),
                    (AnyTypeTag::Object, not_convertible),
                ]
            }
        };
        self.emit_any_tag_switch(ptr, target.val_type(), arms, invalid_tag);
        Ok(())
    }

    /// With a boxed `any` pointer on the stack, leave its text: a string as
    /// is, a number, integer or boolean formatted, a list or object as JSON
    /// text, and null as `""`.
    ///
    /// Null includes a null pointer: `json.get` returns 0 on a miss, and the
    /// common loop `while item != "": item = json.get(arr, i.toString())`
    /// depends on a missing element reading as `""` (WASM-TRAP-JSON-ARRAY-ITER).
    pub(super) fn emit_any_to_string(&mut self) -> Result<(), CompilerError> {
        let float_to_string = self.any_runtime_function("float_to_string")?;
        let bool_to_string = self.any_runtime_function("bool_to_string")?;
        let int64_to_string = self.any_runtime_function("int64_to_string")?;
        let malloc = self.any_runtime_function("__malloc")?;
        // Arrays and objects stringify through the JSON bridge. The import
        // is registered whenever an `AnyToString` is reachable (see
        // `collect_used_function_names_from_mir`).
        let json_encode = self.any_runtime_function("_json_encode_v2")?;

        let ptr = self.alloc_temp_local(ValType::I32);
        self.current_instructions.push(Instruction::LocalSet(ptr));
        let empty = self.alloc_temp_local(ValType::I32);

        let arms = vec![
            (
                // A heap-allocated empty string.
                AnyTypeTag::Null,
                vec![
                    Instruction::I32Const(4),
                    Instruction::Call(malloc),
                    Instruction::LocalTee(empty),
                    Instruction::I32Const(0),
                    Instruction::I32Store(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::LocalGet(empty),
                ],
            ),
            (
                AnyTypeTag::Integer,
                then(i64_payload(ptr), &[Instruction::Call(int64_to_string)]),
            ),
            (
                AnyTypeTag::Boolean,
                then(i32_payload(ptr), &[Instruction::Call(bool_to_string)]),
            ),
            (
                AnyTypeTag::Number,
                then(f64_payload(ptr), &[Instruction::Call(float_to_string)]),
            ),
            (AnyTypeTag::String, i32_payload(ptr)),
        ];
        // Lists and objects: canonical JSON text (the host raises on an
        // invalid tag, BOXED_ANY_ABI §2).
        let default = vec![Instruction::LocalGet(ptr), Instruction::Call(json_encode)];
        self.emit_any_tag_switch(ptr, ValType::I32, arms, default);
        Ok(())
    }
}
