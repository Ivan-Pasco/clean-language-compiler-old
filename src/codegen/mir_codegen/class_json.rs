//! Class JSON runtime — what lets a class instance travel through `any`.
//!
//! Every class instance starts with an object header,
//! `CLASS_HEADER_MARK | class_id` (MEMORY_MODEL.md "Class instance"). A class
//! instance boxed into `any` keeps the instance as its payload (tag 6), so it
//! unboxes to the same instance; the header tells the runtime which class it
//! is. The JSON hosts only understand JSON-tree objects (BOXED_ANY_ABI §3.3),
//! so before a value reaches a JSON bridge the compiler converts every boxed
//! instance in it with three functions emitted here, after the MIR functions:
//!
//! | Function | Signature | Does |
//! |---|---|---|
//! | `__json_tree_instance` | `(instance) -> any` | the JSON object of the instance's own class (switch over the header calling `__json_tree_ClassName`), or a null box for a null instance |
//! | `__json_normalize` | `(any) -> any` | the same value with every boxed instance inside it replaced by its object; containers are copied only when something in them changed, so the caller's value is never modified |
//! | `__any_to_instance` | `(any) -> instance` | the instance an `any` holds, or 0 (null) when it holds anything else |
//!
//! The functions exist only in programs that declare classes; MIR bodies call
//! them by name, so `preregister_class_json_runtime` reserves their indices
//! before the MIR functions are generated.

use super::*;
use crate::mir::mir_types::{CLASS_HEADER_MARK, CLASS_HEADER_MARK_MASK};
use crate::types::WasmType;
use wasm_encoder::MemArg;

const TREE_INSTANCE: &str = "__json_tree_instance";
const NORMALIZE: &str = "__json_normalize";
const ANY_TO_INSTANCE: &str = "__any_to_instance";

/// The runtime functions in emission order.
const RUNTIME_FUNCTIONS: [&str; 3] = [TREE_INSTANCE, NORMALIZE, ANY_TO_INSTANCE];

fn mem(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}

/// Push a new 12-byte null box (tag 0, BOXED_ANY_ABI §2) into local
/// `box_local`. The allocator does not zero memory, so every word is written.
fn null_box(malloc: u32, box_local: u32) -> Vec<Instruction<'static>> {
    let mut code = vec![
        Instruction::I32Const(12),
        Instruction::Call(malloc),
        Instruction::LocalSet(box_local),
    ];
    for offset in [0, 4, 8] {
        code.extend([
            Instruction::LocalGet(box_local),
            Instruction::I32Const(0),
            Instruction::I32Store(mem(offset)),
        ]);
    }
    code
}

/// Leave 1 when the word at the address in `local` is an object header.
fn is_instance(local: u32) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(local),
        Instruction::I32Load(mem(0)),
        Instruction::I32Const(CLASS_HEADER_MARK_MASK as i32),
        Instruction::I32And,
        Instruction::I32Const(CLASS_HEADER_MARK as i32),
        Instruction::I32Eq,
    ]
}

impl MirCodeGenerator<'_> {
    /// Reserve the runtime's function indices, starting at `base` (the index
    /// right after the last MIR function).
    pub(super) fn preregister_class_json_runtime(&mut self, base: u32) {
        if self.class_json_trees.is_empty() {
            return;
        }
        for (offset, name) in RUNTIME_FUNCTIONS.iter().enumerate() {
            self.wasm_generator
                .function_map
                .insert((*name).to_string(), base + offset as u32);
        }
    }

    /// Emit the runtime at the indices reserved from `base`.
    pub(super) fn emit_class_json_runtime(&mut self, base: u32) -> Result<(), CompilerError> {
        if self.class_json_trees.is_empty() {
            return Ok(());
        }
        let malloc = self.runtime_function_index("__malloc")?;
        let memcpy = self.runtime_function_index("__memcpy")?;
        let tree_instance = base;
        let normalize = base + 1;

        let mut dispatch = Vec::new();
        for (class_id, tree_symbol) in &self.class_json_trees {
            let tree_index = *self
                .symbol_to_function_index
                .get(tree_symbol)
                .ok_or_else(|| {
                    CompilerError::codegen_error(
                        format!(
                            "class JSON builder SymbolId({}) has no WASM function index",
                            tree_symbol.0
                        ),
                        None,
                        None,
                    )
                })?;
            dispatch.push((crate::mir::class_header_word(*class_id), tree_index));
        }

        let bodies = [
            (
                TREE_INSTANCE,
                vec![WasmType::I32; 2],
                Self::tree_instance_body(malloc, &dispatch),
            ),
            (
                NORMALIZE,
                vec![WasmType::I32; 12],
                Self::normalize_body(malloc, memcpy, tree_instance, normalize),
            ),
            (
                ANY_TO_INSTANCE,
                vec![WasmType::I32],
                Self::any_to_instance_body(),
            ),
        ];
        for (offset, (name, locals, body)) in bodies.into_iter().enumerate() {
            let index = self.wasm_generator.register_function_with_locals(
                name,
                &[WasmType::I32],
                Some(WasmType::I32),
                &locals,
                &body,
            )?;
            if index != base + offset as u32 {
                return Err(CompilerError::codegen_error(
                    format!(
                        "{} was emitted at function index {} but calls were compiled for {}",
                        name,
                        index,
                        base + offset as u32
                    ),
                    None,
                    None,
                ));
            }
        }
        Ok(())
    }

    fn runtime_function_index(&self, name: &str) -> Result<u32, CompilerError> {
        self.wasm_generator
            .function_map
            .get(name)
            .copied()
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    format!("{name} is not registered; the class JSON runtime needs it"),
                    None,
                    None,
                )
            })
    }

    /// `__json_tree_instance(instance)`. Locals: 0 instance, 1 header, 2 box.
    fn tree_instance_body(malloc: u32, dispatch: &[(i32, u32)]) -> Vec<Instruction<'static>> {
        let mut code = vec![
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
        ];
        code.extend(null_box(malloc, 2));
        code.extend([
            Instruction::LocalGet(2),
            Instruction::Return,
            Instruction::End,
            Instruction::LocalGet(0),
            Instruction::I32Load(mem(0)),
            Instruction::LocalSet(1),
        ]);
        for (header, tree_index) in dispatch {
            code.extend([
                Instruction::LocalGet(1),
                Instruction::I32Const(*header),
                Instruction::I32Eq,
                Instruction::If(BlockType::Empty),
                Instruction::LocalGet(0),
                Instruction::Call(*tree_index),
                Instruction::Return,
                Instruction::End,
            ]);
        }
        // Not an instance of a class of this program: null.
        code.extend(null_box(malloc, 2));
        code.push(Instruction::LocalGet(2));
        code
    }

    /// `__json_normalize(box)`. Locals: 0 box, 1 tag, 2 payload, 3 count,
    /// 4 i, 5 child, 6 normalized child, 7 copy, 8 copy size, 9 slot,
    /// 10 new box, 11 first value offset, 12 stride.
    fn normalize_body(
        malloc: u32,
        memcpy: u32,
        tree_instance: u32,
        normalize: u32,
    ) -> Vec<Instruction<'static>> {
        use Instruction::*;
        let mut code = vec![LocalGet(0), I32Eqz, If(BlockType::Empty)];
        // An `any` holding null as a raw 0 (an `any` function returning null).
        code.extend(null_box(malloc, 10));
        code.extend([LocalGet(10), Return, End]);
        // Only objects (6) and arrays (5) can hold instances.
        code.extend([
            LocalGet(0),
            I32Load(mem(0)),
            LocalTee(1),
            I32Const(AnyTypeTag::Object.as_i32()),
            I32Eq,
            LocalGet(1),
            I32Const(AnyTypeTag::List.as_i32()),
            I32Eq,
            I32Or,
            I32Eqz,
            If(BlockType::Empty),
            LocalGet(0),
            Return,
            End,
            LocalGet(0),
            I32Load(mem(4)),
            LocalTee(2),
            I32Eqz,
            If(BlockType::Empty),
        ]);
        code.extend(null_box(malloc, 10));
        code.extend([
            LocalGet(10),
            Return,
            End,
            LocalGet(1),
            I32Const(AnyTypeTag::Object.as_i32()),
            I32Eq,
            If(BlockType::Empty),
        ]);
        // A boxed instance becomes the object of its class (whose own `any`
        // members may hold instances in turn).
        code.extend(is_instance(2));
        code.extend([
            If(BlockType::Empty),
            LocalGet(2),
            Call(tree_instance),
            Call(normalize),
            Return,
            End,
            // Object fragment: [count][(key, value) × count].
            I32Const(8),
            LocalSet(11),
            I32Const(8),
            LocalSet(12),
            Else,
            // Array fragment: [count][value × count].
            I32Const(4),
            LocalSet(11),
            I32Const(4),
            LocalSet(12),
            End,
            LocalGet(2),
            I32Load(mem(0)),
            LocalSet(3),
            I32Const(0),
            LocalSet(4),
            I32Const(0),
            LocalSet(7),
            Block(BlockType::Empty),
            Loop(BlockType::Empty),
            LocalGet(4),
            LocalGet(3),
            I32GeS,
            BrIf(1),
            // slot = payload + first + i * stride
            LocalGet(2),
            LocalGet(11),
            I32Add,
            LocalGet(4),
            LocalGet(12),
            I32Mul,
            I32Add,
            LocalTee(9),
            I32Load(mem(0)),
            LocalTee(5),
            Call(normalize),
            LocalTee(6),
            LocalGet(5),
            I32Ne,
            If(BlockType::Empty),
            // First change: copy the fragment, then patch the copy.
            LocalGet(7),
            I32Eqz,
            If(BlockType::Empty),
            I32Const(4),
            LocalGet(3),
            LocalGet(12),
            I32Mul,
            I32Add,
            LocalTee(8),
            Call(malloc),
            LocalSet(7),
            LocalGet(7),
            LocalGet(2),
            LocalGet(8),
            Call(memcpy),
            End,
            LocalGet(7),
            LocalGet(9),
            LocalGet(2),
            I32Sub,
            I32Add,
            LocalGet(6),
            I32Store(mem(0)),
            End,
            LocalGet(4),
            I32Const(1),
            I32Add,
            LocalSet(4),
            Br(0),
            End,
            End,
            // Unchanged: the caller's own value.
            LocalGet(7),
            I32Eqz,
            If(BlockType::Empty),
            LocalGet(0),
            Return,
            End,
            I32Const(12),
            Call(malloc),
            LocalTee(10),
            LocalGet(1),
            I32Store(mem(0)),
            LocalGet(10),
            LocalGet(7),
            I32Store(mem(4)),
            LocalGet(10),
            I32Const(0),
            I32Store(mem(8)),
            LocalGet(10),
        ]);
        code
    }

    /// `__any_to_instance(box)`. Locals: 0 box, 1 payload.
    fn any_to_instance_body() -> Vec<Instruction<'static>> {
        use Instruction::*;
        let mut code = vec![
            LocalGet(0),
            I32Eqz,
            If(BlockType::Empty),
            I32Const(0),
            Return,
            End,
            LocalGet(0),
            I32Load(mem(0)),
            I32Const(AnyTypeTag::Object.as_i32()),
            I32Ne,
            If(BlockType::Empty),
            I32Const(0),
            Return,
            End,
            LocalGet(0),
            I32Load(mem(4)),
            LocalTee(1),
            I32Eqz,
            If(BlockType::Empty),
            I32Const(0),
            Return,
            End,
        ];
        code.extend(is_instance(1));
        code.extend([
            If(BlockType::Result(ValType::I32)),
            LocalGet(1),
            Else,
            I32Const(0),
            End,
        ]);
        code
    }
}
