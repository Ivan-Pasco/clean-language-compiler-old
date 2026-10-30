//! List methods implemented as native WASM functions.
//!
//! All functions operate on the standard list layout
//! (see `foundation/spec/platform/MEMORY_MODEL.md`):
//!
//! ```text
//! [length: i32 @0][capacity: i32 @4][type_id: i32 @8][flags: i32 @12][elements @16]
//! ```
//!
//! `capacity` counts 4-byte slots, so an f64 element occupies two of them.
//! Element stride is 4 bytes for i32-represented values (integer, boolean,
//! string and object pointers) and 8 bytes for `number` (f64).
//!
//! [`ListClass::register_functions`] is called from
//! `ListManager::register_functions`, which runs after the native memory
//! operations (`__malloc`) and `list_behavior.rs` (`list.add`) are registered.
//!
//! Element-typed variants are selected by the MIR builder from the receiver's
//! element type: `list.<method>` for i32 elements, `list.<method>.f64` for
//! `list<number>`, and `list.<method>.string` where strings need byte
//! comparison. Variants that return an f64 element use an internal
//! `__list_*_f64` name so the `list.` namespace return-type heuristic (i32)
//! never applies to them.

use crate::codegen::native_stdlib::list_ops::{gen_reverse_i32, gen_slice_i32};
use crate::codegen::CodeGenerator;
use crate::codegen::LIST_TYPE_ID;
use crate::error::CompilerError;
use crate::stdlib::list_behavior::BEHAVIOR_LINE;
use crate::stdlib::{register_stdlib_function, register_stdlib_function_with_locals};
use crate::types::WasmType;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// Byte offset of the first element.
const DATA: i32 = 16;

fn mem(offset: u64, align: u32) -> MemArg {
    MemArg {
        offset,
        align,
        memory_index: 0,
    }
}

/// Element representation of a list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Elem {
    /// 4-byte scalar compared by value (integer, boolean, object pointer).
    I32,
    /// 8-byte `number`.
    F64,
    /// 4-byte pointer to a length-prefixed string, compared by bytes.
    Str,
}

impl Elem {
    fn stride(self) -> i32 {
        match self {
            Elem::F64 => 8,
            Elem::I32 | Elem::Str => 4,
        }
    }

    fn wasm(self) -> WasmType {
        match self {
            Elem::F64 => WasmType::F64,
            Elem::I32 | Elem::Str => WasmType::I32,
        }
    }

    fn load(self) -> Instruction<'static> {
        match self {
            Elem::F64 => Instruction::F64Load(mem(DATA as u64, 3)),
            Elem::I32 | Elem::Str => Instruction::I32Load(mem(DATA as u64, 2)),
        }
    }

    fn store(self) -> Instruction<'static> {
        match self {
            Elem::F64 => Instruction::F64Store(mem(DATA as u64, 3)),
            Elem::I32 | Elem::Str => Instruction::I32Store(mem(DATA as u64, 2)),
        }
    }
}

/// Push the address `list + index * stride` (element offset is folded into the
/// load/store `MemArg`).
fn elem_addr(list_local: u32, index_local: u32, elem: Elem) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(list_local),
        Instruction::LocalGet(index_local),
        Instruction::I32Const(elem.stride()),
        Instruction::I32Mul,
        Instruction::I32Add,
    ]
}

/// Allocate a list of `len_local` elements with the same type id and behavior
/// flags as `src_local`, storing the pointer in `dst_local`. Capacity is set to
/// exactly the allocated slot count so in-place appends grow the list instead
/// of writing past the block.
fn emit_alloc_like(
    malloc: u32,
    src_local: u32,
    len_local: u32,
    dst_local: u32,
    stride: i32,
) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(len_local),
        Instruction::I32Const(stride),
        Instruction::I32Mul,
        Instruction::I32Const(DATA),
        Instruction::I32Add,
        Instruction::Call(malloc),
        Instruction::LocalSet(dst_local),
        // length
        Instruction::LocalGet(dst_local),
        Instruction::LocalGet(len_local),
        Instruction::I32Store(mem(0, 2)),
        // capacity in 4-byte slots
        Instruction::LocalGet(dst_local),
        Instruction::LocalGet(len_local),
        Instruction::I32Const(stride / 4),
        Instruction::I32Mul,
        Instruction::I32Store(mem(4, 2)),
        // type id
        Instruction::LocalGet(dst_local),
        Instruction::LocalGet(src_local),
        Instruction::I32Load(mem(8, 2)),
        Instruction::I32Store(mem(8, 2)),
        // behavior flags
        Instruction::LocalGet(dst_local),
        Instruction::LocalGet(src_local),
        Instruction::I32Load(mem(12, 2)),
        Instruction::I32Store(mem(12, 2)),
    ]
}

/// Allocate a fresh list (type id `LIST_TYPE_ID`, no behavior flags) holding
/// `len_local` i32 elements.
fn emit_alloc_fresh_i32(malloc: u32, len_local: u32, dst_local: u32) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(len_local),
        Instruction::I32Const(4),
        Instruction::I32Mul,
        Instruction::I32Const(DATA),
        Instruction::I32Add,
        Instruction::Call(malloc),
        Instruction::LocalSet(dst_local),
        Instruction::LocalGet(dst_local),
        Instruction::LocalGet(len_local),
        Instruction::I32Store(mem(0, 2)),
        Instruction::LocalGet(dst_local),
        Instruction::LocalGet(len_local),
        Instruction::I32Store(mem(4, 2)),
        Instruction::LocalGet(dst_local),
        Instruction::I32Const(LIST_TYPE_ID as i32),
        Instruction::I32Store(mem(8, 2)),
        Instruction::LocalGet(dst_local),
        Instruction::I32Const(0),
        Instruction::I32Store(mem(12, 2)),
    ]
}

/// Copy `words_local` 4-byte words from `src_local + 16` to
/// `dst_local + 16 + dst_word_offset_local * 4`, using `i_local` as counter.
fn emit_copy_words(
    src_local: u32,
    dst_local: u32,
    dst_word_offset_local: Option<u32>,
    words_local: u32,
    i_local: u32,
) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::I32Const(0),
        Instruction::LocalSet(i_local),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(i_local),
        Instruction::LocalGet(words_local),
        Instruction::I32GeS,
        Instruction::BrIf(1),
        Instruction::LocalGet(dst_local),
        Instruction::LocalGet(i_local),
    ];
    if let Some(off) = dst_word_offset_local {
        v.push(Instruction::LocalGet(off));
        v.push(Instruction::I32Add);
    }
    v.extend([
        Instruction::I32Const(4),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalGet(src_local),
        Instruction::LocalGet(i_local),
        Instruction::I32Const(4),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::I32Load(mem(DATA as u64, 2)),
        Instruction::I32Store(mem(DATA as u64, 2)),
        Instruction::LocalGet(i_local),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(i_local),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
    ]);
    v
}

/// Trap when the list in `list_local` is empty (spec: `.first()`, `.last()`
/// and `.peek()` halt on an empty list). Leaves the length in `len_local`.
fn emit_halt_if_empty(list_local: u32, len_local: u32) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(list_local),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalTee(len_local),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::Unreachable,
        Instruction::End,
    ]
}

/// `first` / `last` / `peek`: params (list); local 1 = len.
fn gen_end_element(elem: Elem, last: bool) -> Vec<Instruction<'static>> {
    let mut v = emit_halt_if_empty(0, 1);
    v.push(Instruction::LocalGet(0));
    if last {
        v.extend([
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::I32Const(elem.stride()),
            Instruction::I32Mul,
            Instruction::I32Add,
        ]);
    }
    v.push(elem.load());
    v
}

/// `peek` for `list<number>`: the front element of a `.line` list, otherwise
/// the last element (mirrors the i32 `list.peek` in `list_behavior.rs`).
/// Halts on an empty list. params (list); local 1 = len.
fn gen_peek_f64() -> Vec<Instruction<'static>> {
    let mut v = emit_halt_if_empty(0, 1);
    v.extend([
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(12, 2)),
        Instruction::I32Const(BEHAVIOR_LINE),
        Instruction::I32And,
        Instruction::If(BlockType::Result(ValType::F64)),
    ]);
    v.extend(gen_end_element(Elem::F64, false).into_iter().skip(7));
    v.push(Instruction::Else);
    v.extend(gen_end_element(Elem::F64, true).into_iter().skip(7));
    v.push(Instruction::End);
    v
}

/// Push `1` when the element value on the stack equals the needle in local 1.
/// Stack before: [element]. `str_cmp` is required for `Elem::Str`.
fn emit_equals_needle(elem: Elem, str_cmp: Option<u32>) -> Vec<Instruction<'static>> {
    match elem {
        Elem::I32 => vec![Instruction::LocalGet(1), Instruction::I32Eq],
        Elem::F64 => vec![Instruction::LocalGet(1), Instruction::F64Eq],
        Elem::Str => vec![
            Instruction::LocalGet(1),
            Instruction::Call(str_cmp.expect("string comparison helper registered")),
            Instruction::I32Eqz,
        ],
    }
}

/// `indexOf` / `lastIndexOf`: params (list, needle); locals 2 = len, 3 = i.
/// Returns the index of the first (or last) equal element, or -1.
fn gen_search(elem: Elem, from_end: bool, str_cmp: Option<u32>) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(2),
    ];
    if from_end {
        v.extend([
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(3),
        ]);
    } else {
        v.extend([Instruction::I32Const(0), Instruction::LocalSet(3)]);
    }
    v.extend([
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
    ]);
    if from_end {
        v.extend([
            Instruction::LocalGet(3),
            Instruction::I32Const(0),
            Instruction::I32LtS,
            Instruction::BrIf(1),
        ]);
    } else {
        v.extend([
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeS,
            Instruction::BrIf(1),
        ]);
    }
    v.extend(elem_addr(0, 3, elem));
    v.push(elem.load());
    v.extend(emit_equals_needle(elem, str_cmp));
    v.extend([
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(3),
        Instruction::Return,
        Instruction::End,
        Instruction::LocalGet(3),
        Instruction::I32Const(if from_end { -1 } else { 1 }),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        Instruction::I32Const(-1),
    ]);
    v
}

/// `contains` built on an `indexOf` variant: params (list, needle).
fn gen_contains_via(index_of: u32) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0),
        Instruction::LocalGet(1),
        Instruction::Call(index_of),
        Instruction::I32Const(0),
        Instruction::I32GeS,
    ]
}

/// `reverse` for 8-byte elements: params (list); locals 1 = len, 2 = dst, 3 = i.
fn gen_reverse_f64(malloc: u32) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(1),
    ];
    v.extend(emit_alloc_like(malloc, 0, 1, 2, 8));
    v.extend([
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::I32GeS,
        Instruction::BrIf(1),
        // dst[i] = src[len - 1 - i] (bit copy)
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32Const(8),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::LocalGet(1),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalGet(3),
        Instruction::I32Sub,
        Instruction::I32Const(8),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::I64Load(mem(DATA as u64, 3)),
        Instruction::I64Store(mem(DATA as u64, 3)),
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        Instruction::LocalGet(2),
    ]);
    v
}

/// Clamp `start` (local 1) to [0, len] and `end` (local 2) to [start, len],
/// with the length in `len_local`.
fn emit_clamp_range(len_local: u32) -> Vec<Instruction<'static>> {
    vec![
        // start = max(start, 0)
        Instruction::LocalGet(1),
        Instruction::I32Const(0),
        Instruction::LocalGet(1),
        Instruction::I32Const(0),
        Instruction::I32GtS,
        Instruction::Select,
        Instruction::LocalSet(1),
        // start = min(start, len)
        Instruction::LocalGet(1),
        Instruction::LocalGet(len_local),
        Instruction::LocalGet(1),
        Instruction::LocalGet(len_local),
        Instruction::I32LtS,
        Instruction::Select,
        Instruction::LocalSet(1),
        // end = min(end, len)
        Instruction::LocalGet(2),
        Instruction::LocalGet(len_local),
        Instruction::LocalGet(2),
        Instruction::LocalGet(len_local),
        Instruction::I32LtS,
        Instruction::Select,
        Instruction::LocalSet(2),
        // end = max(end, start)
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32GtS,
        Instruction::Select,
        Instruction::LocalSet(2),
    ]
}

/// `slice` for 8-byte elements: params (list, start, end);
/// locals 3 = len / new_len, 4 = dst, 5 = i (word counter), 6 = words, 7 = src_base.
fn gen_slice_f64(malloc: u32) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(3),
    ];
    v.extend(emit_clamp_range(3));
    v.extend([
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32Sub,
        Instruction::LocalSet(3),
    ]);
    v.extend(emit_alloc_like(malloc, 0, 3, 4, 8));
    v.extend([
        // words = new_len * 2; src_base = list + start * 8
        Instruction::LocalGet(3),
        Instruction::I32Const(2),
        Instruction::I32Mul,
        Instruction::LocalSet(6),
        Instruction::LocalGet(0),
        Instruction::LocalGet(1),
        Instruction::I32Const(8),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalSet(7),
    ]);
    v.extend(emit_copy_words(7, 4, None, 6, 5));
    v.push(Instruction::LocalGet(4));
    v
}

/// Push `1` when element `a` (below) is greater than element `b` (top).
fn emit_greater(elem: Elem, str_cmp: Option<u32>) -> Vec<Instruction<'static>> {
    match elem {
        Elem::I32 => vec![Instruction::I32GtS],
        Elem::F64 => vec![Instruction::F64Gt],
        Elem::Str => vec![
            Instruction::Call(str_cmp.expect("string comparison helper registered")),
            Instruction::I32Const(0),
            Instruction::I32GtS,
        ],
    }
}

/// `sort`: returns a new list sorted ascending (stable insertion sort).
/// params (list); locals 1 = len, 2 = dst, 3 = i, 4 = j, 5 = words, 6 = key.
fn gen_sort(malloc: u32, elem: Elem, str_cmp: Option<u32>) -> Vec<Instruction<'static>> {
    let stride = elem.stride();
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(1),
    ];
    v.extend(emit_alloc_like(malloc, 0, 1, 2, stride));
    v.extend([
        Instruction::LocalGet(1),
        Instruction::I32Const(stride / 4),
        Instruction::I32Mul,
        Instruction::LocalSet(5),
    ]);
    v.extend(emit_copy_words(0, 2, None, 5, 3));
    v.extend([
        Instruction::I32Const(1),
        Instruction::LocalSet(3),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::I32GeS,
        Instruction::BrIf(1),
    ]);
    // key = dst[i]
    v.extend(elem_addr(2, 3, elem));
    v.push(elem.load());
    v.push(Instruction::LocalSet(6));
    // j = i - 1
    v.extend([
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(4),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::BrIf(1),
    ]);
    // stop shifting once dst[j] <= key
    v.extend(elem_addr(2, 4, elem));
    v.push(elem.load());
    v.push(Instruction::LocalGet(6));
    v.extend(emit_greater(elem, str_cmp));
    v.extend([Instruction::I32Eqz, Instruction::BrIf(1)]);
    // dst[j + 1] = dst[j]
    v.extend(elem_addr(2, 4, elem));
    v.extend([Instruction::I32Const(stride), Instruction::I32Add]);
    v.extend(elem_addr(2, 4, elem));
    v.push(elem.load());
    v.push(elem.store());
    v.extend([
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
    ]);
    // dst[j + 1] = key
    v.extend(elem_addr(2, 4, elem));
    v.extend([Instruction::I32Const(stride), Instruction::I32Add]);
    v.push(Instruction::LocalGet(6));
    v.push(elem.store());
    v.extend([
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        Instruction::LocalGet(2),
    ]);
    v
}

/// `insert(index, value)`: params (list, index, value);
/// locals 3 = old_len, 4 = list (possibly moved), 5 = new_len, 6 = j.
///
/// Appends through `list.add` / `list.add_f64` (which grows the block and
/// honours behavior flags), then rotates the new element into `index`.
/// Returns the possibly moved list pointer; the MIR builder writes it back
/// into the receiver. Halts when `index` is outside `0..=length`.
fn gen_insert(elem: Elem, list_add: u32) -> Vec<Instruction<'static>> {
    let stride = elem.stride();
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(3),
        Instruction::LocalGet(1),
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::LocalGet(1),
        Instruction::LocalGet(3),
        Instruction::I32GtS,
        Instruction::I32Or,
        Instruction::If(BlockType::Empty),
        Instruction::Unreachable,
        Instruction::End,
        Instruction::LocalGet(0),
        Instruction::LocalGet(2),
        Instruction::Call(list_add),
        Instruction::LocalSet(4),
        Instruction::LocalGet(4),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(5),
        // A `.unique` list rejected the value: nothing to place.
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::I32Eq,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::Return,
        Instruction::End,
        // j = new_len - 1; while j > index: list[j] = list[j - 1]; j--
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(6),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(6),
        Instruction::LocalGet(1),
        Instruction::I32LeS,
        Instruction::BrIf(1),
    ];
    v.extend(elem_addr(4, 6, elem));
    v.extend(elem_addr(4, 6, elem));
    v.extend([Instruction::I32Const(stride), Instruction::I32Sub]);
    v.push(elem.load());
    v.push(elem.store());
    v.extend([
        Instruction::LocalGet(6),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::LocalSet(6),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
    ]);
    v.extend(elem_addr(4, 1, elem));
    v.push(Instruction::LocalGet(2));
    v.push(elem.store());
    v.push(Instruction::LocalGet(4));
    v
}

/// Which element a remove operation takes out of the list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RemoveAt {
    /// `.remove(index)`: params (list, index). Halts when `index` is outside
    /// `0..length`.
    Index,
    /// `.pop()` / `.removeLast()`: params (list). Halts on an empty list.
    Last,
    /// Index-less `.remove()`: params (list). The front element of a `.line`
    /// list, otherwise the last one; an empty list yields the zero value.
    Behavior,
}

/// Remove one element in place, shifting the following elements down one
/// slot, and return it.
///
/// Locals: 1 = index (a parameter for [`RemoveAt::Index`]), 2 = len, 3 = j,
/// 4 = removed element (i32 or f64).
fn gen_remove(elem: Elem, at: RemoveAt) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(2),
    ];
    match at {
        RemoveAt::Index => {}
        RemoveAt::Last => v.extend([
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::LocalSet(1),
        ]),
        RemoveAt::Behavior => {
            v.extend([
                Instruction::LocalGet(2),
                Instruction::I32Eqz,
                Instruction::If(BlockType::Empty),
            ]);
            v.push(match elem {
                Elem::F64 => Instruction::F64Const(0.0),
                Elem::I32 | Elem::Str => Instruction::I32Const(0),
            });
            v.extend([
                Instruction::Return,
                Instruction::End,
                Instruction::LocalGet(0),
                Instruction::I32Load(mem(12, 2)),
                Instruction::I32Const(BEHAVIOR_LINE),
                Instruction::I32And,
                Instruction::If(BlockType::Result(ValType::I32)),
                Instruction::I32Const(0),
                Instruction::Else,
                Instruction::LocalGet(2),
                Instruction::I32Const(1),
                Instruction::I32Sub,
                Instruction::End,
                Instruction::LocalSet(1),
            ]);
        }
    }
    v.extend(emit_halt_if_out_of_bounds(1, 2));
    v.extend(elem_addr(0, 1, elem));
    v.push(elem.load());
    v.push(Instruction::LocalSet(4));
    // j = index; while j < len - 1: list[j] = list[j + 1]; j++
    v.extend([
        Instruction::LocalGet(1),
        Instruction::LocalSet(3),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::I32GeS,
        Instruction::BrIf(1),
    ]);
    v.extend(elem_addr(0, 3, elem));
    v.extend(elem_addr(0, 3, elem));
    v.extend([Instruction::I32Const(elem.stride()), Instruction::I32Add]);
    v.push(elem.load());
    v.push(elem.store());
    v.extend([
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // length = len - 1
        Instruction::LocalGet(0),
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::I32Store(mem(0, 2)),
        Instruction::LocalGet(4),
    ]);
    v
}

/// Trap unless `0 <= index < len` (spec: `.get`, `.set` and `.remove(index)`
/// halt on an out-of-bounds index).
fn emit_halt_if_out_of_bounds(index_local: u32, len_local: u32) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(index_local),
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::LocalGet(index_local),
        Instruction::LocalGet(len_local),
        Instruction::I32GeS,
        Instruction::I32Or,
        Instruction::If(BlockType::Empty),
        Instruction::Unreachable,
        Instruction::End,
    ]
}

/// `get(index)`: params (list, index); local 2 = len. Halts when `index` is
/// outside `0..length`.
fn gen_get(elem: Elem) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(2),
    ];
    v.extend(emit_halt_if_out_of_bounds(1, 2));
    v.extend(elem_addr(0, 1, elem));
    v.push(elem.load());
    v
}

/// `set(index, value)`: params (list, index, value); local 3 = len. Stores
/// in place; halts when `index` is outside `0..length`.
fn gen_set(elem: Elem) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(3),
    ];
    v.extend(emit_halt_if_out_of_bounds(1, 3));
    v.extend(elem_addr(0, 1, elem));
    v.push(Instruction::LocalGet(2));
    v.push(elem.store());
    v
}

/// `concat(a, b)`: params (a, b); locals 2 = la, 3 = lb, 4 = total, 5 = dst,
/// 6 = i, 7 = words_a, 8 = words_b.
fn gen_concat(malloc: u32, elem: Elem) -> Vec<Instruction<'static>> {
    let slots = elem.stride() / 4;
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(2),
        Instruction::LocalGet(1),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(3),
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::LocalSet(4),
    ];
    v.extend(emit_alloc_like(malloc, 0, 4, 5, elem.stride()));
    v.extend([
        Instruction::LocalGet(2),
        Instruction::I32Const(slots),
        Instruction::I32Mul,
        Instruction::LocalSet(7),
        Instruction::LocalGet(3),
        Instruction::I32Const(slots),
        Instruction::I32Mul,
        Instruction::LocalSet(8),
    ]);
    v.extend(emit_copy_words(0, 5, None, 7, 6));
    v.extend(emit_copy_words(1, 5, Some(7), 8, 6));
    v.push(Instruction::LocalGet(5));
    v
}

/// `list.range(start, end)`: params (start, end); locals 2 = len, 3 = dst, 4 = i.
/// Produces `start, start + 1, ..., end - 1` (empty when `end <= start`).
fn gen_range(malloc: u32) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::LocalGet(1),
        Instruction::LocalGet(0),
        Instruction::I32Sub,
        Instruction::LocalTee(2),
        Instruction::I32Const(0),
        Instruction::LocalGet(2),
        Instruction::I32Const(0),
        Instruction::I32GtS,
        Instruction::Select,
        Instruction::LocalSet(2),
    ];
    v.extend(emit_alloc_fresh_i32(malloc, 2, 3));
    v.extend([
        Instruction::I32Const(0),
        Instruction::LocalSet(4),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::LocalGet(2),
        Instruction::I32GeS,
        Instruction::BrIf(1),
    ]);
    v.extend(elem_addr(3, 4, Elem::I32));
    v.extend([
        Instruction::LocalGet(0),
        Instruction::LocalGet(4),
        Instruction::I32Add,
        Elem::I32.store(),
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        Instruction::LocalGet(3),
    ]);
    v
}

/// `list.fill(size, value)` for i32 elements: params (size, value);
/// locals 2 = len, 3 = dst, 4 = i.
fn gen_fill(malloc: u32) -> Vec<Instruction<'static>> {
    let mut v = vec![
        // len = max(size, 0)
        Instruction::LocalGet(0),
        Instruction::I32Const(0),
        Instruction::LocalGet(0),
        Instruction::I32Const(0),
        Instruction::I32GtS,
        Instruction::Select,
        Instruction::LocalSet(2),
    ];
    v.extend(emit_alloc_fresh_i32(malloc, 2, 3));
    v.extend([
        Instruction::I32Const(0),
        Instruction::LocalSet(4),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::LocalGet(2),
        Instruction::I32GeS,
        Instruction::BrIf(1),
    ]);
    v.extend(elem_addr(3, 4, Elem::I32));
    v.extend([
        Instruction::LocalGet(1),
        Elem::I32.store(),
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        Instruction::LocalGet(3),
    ]);
    v
}

/// Copy `len_local` bytes from `src_local` to `pos_local`, advancing
/// `pos_local`; `k_local` is scratch.
fn emit_copy_bytes(
    src_local: u32,
    len_local: u32,
    pos_local: u32,
    k_local: u32,
) -> Vec<Instruction<'static>> {
    vec![
        Instruction::I32Const(0),
        Instruction::LocalSet(k_local),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(k_local),
        Instruction::LocalGet(len_local),
        Instruction::I32GeS,
        Instruction::BrIf(1),
        Instruction::LocalGet(pos_local),
        Instruction::LocalGet(k_local),
        Instruction::I32Add,
        Instruction::LocalGet(src_local),
        Instruction::LocalGet(k_local),
        Instruction::I32Add,
        Instruction::I32Load8U(mem(0, 0)),
        Instruction::I32Store8(mem(0, 0)),
        Instruction::LocalGet(k_local),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(k_local),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        Instruction::LocalGet(pos_local),
        Instruction::LocalGet(len_local),
        Instruction::I32Add,
        Instruction::LocalSet(pos_local),
    ]
}

/// `list.join(items, sep)` / `string.join(items, sep)` for `list<string>`:
/// params (list, sep); locals 2 = len, 3 = total, 4 = i, 5 = dst, 6 = pos,
/// 7 = src, 8 = n, 9 = sep_len, 10 = k.
fn gen_join(malloc: u32) -> Vec<Instruction<'static>> {
    let mut v = vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(2),
        Instruction::LocalGet(1),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(9),
        // total = sum(len(item)) + sep_len * max(len - 1, 0)
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        Instruction::I32Const(0),
        Instruction::LocalSet(4),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::LocalGet(2),
        Instruction::I32GeS,
        Instruction::BrIf(1),
        Instruction::LocalGet(3),
    ];
    v.extend(elem_addr(0, 4, Elem::Str));
    v.extend([
        Elem::Str.load(),
        Instruction::I32Load(mem(0, 2)),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        Instruction::LocalGet(2),
        Instruction::I32Const(0),
        Instruction::I32GtS,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(3),
        Instruction::LocalGet(9),
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Sub,
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::End,
        // dst = malloc(4 + total); dst.len = total; pos = dst + 4
        Instruction::LocalGet(3),
        Instruction::I32Const(4),
        Instruction::I32Add,
        Instruction::Call(malloc),
        Instruction::LocalSet(5),
        Instruction::LocalGet(5),
        Instruction::LocalGet(3),
        Instruction::I32Store(mem(0, 2)),
        Instruction::LocalGet(5),
        Instruction::I32Const(4),
        Instruction::I32Add,
        Instruction::LocalSet(6),
        Instruction::I32Const(0),
        Instruction::LocalSet(4),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(4),
        Instruction::LocalGet(2),
        Instruction::I32GeS,
        Instruction::BrIf(1),
        // separator before every item but the first
        Instruction::LocalGet(4),
        Instruction::I32Const(0),
        Instruction::I32GtS,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(1),
        Instruction::I32Const(4),
        Instruction::I32Add,
        Instruction::LocalSet(7),
    ]);
    v.extend(emit_copy_bytes(7, 9, 6, 10));
    v.push(Instruction::End);
    // item bytes
    v.extend(elem_addr(0, 4, Elem::Str));
    v.extend([
        Elem::Str.load(),
        Instruction::LocalTee(7),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(8),
        Instruction::LocalGet(7),
        Instruction::I32Const(4),
        Instruction::I32Add,
        Instruction::LocalSet(7),
    ]);
    v.extend(emit_copy_bytes(7, 8, 6, 10));
    v.extend([
        Instruction::LocalGet(4),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        Instruction::LocalGet(5),
    ]);
    v
}

/// Byte-wise lexicographic comparison of two length-prefixed strings.
/// params (a, b); locals 2 = la, 3 = lb, 4 = n, 5 = i, 6 = ca, 7 = cb.
/// Returns -1, 0 or 1.
fn gen_str_cmp() -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(2),
        Instruction::LocalGet(1),
        Instruction::I32Load(mem(0, 2)),
        Instruction::LocalSet(3),
        // n = min(la, lb)
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32LtU,
        Instruction::Select,
        Instruction::LocalSet(4),
        Instruction::I32Const(0),
        Instruction::LocalSet(5),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(5),
        Instruction::LocalGet(4),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        Instruction::LocalGet(0),
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::I32Load8U(mem(4, 0)),
        Instruction::LocalSet(6),
        Instruction::LocalGet(1),
        Instruction::LocalGet(5),
        Instruction::I32Add,
        Instruction::I32Load8U(mem(4, 0)),
        Instruction::LocalSet(7),
        Instruction::LocalGet(6),
        Instruction::LocalGet(7),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(-1),
        Instruction::I32Const(1),
        Instruction::LocalGet(6),
        Instruction::LocalGet(7),
        Instruction::I32LtU,
        Instruction::Select,
        Instruction::Return,
        Instruction::End,
        Instruction::LocalGet(5),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Common prefix equal: the shorter string sorts first.
        Instruction::I32Const(-1),
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32GtU,
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32LtU,
        Instruction::Select,
    ]
}

/// List class implementation for Clean Language.
pub struct ListClass;

impl Default for ListClass {
    fn default() -> Self {
        Self::new()
    }
}

impl ListClass {
    pub fn new() -> Self {
        Self
    }

    /// Register every list method implementation. Must run after the native
    /// memory operations (`__malloc`) and `list_behavior.rs` (`list.add`).
    /// `list.length`, `list.size`, `list.isEmpty`, `list.isNotEmpty`,
    /// `list.get`, `list.contains`, `list.indexOf`, `list.add` and
    /// `list.peek` are registered by `list_behavior.rs` and the native aliases
    /// in `codegen_registration.rs`; `list.set`, `list.pop` and the remove
    /// family are (re)registered here.
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        let malloc = codegen
            .get_function_index("__malloc")
            .ok_or_else(|| missing("__malloc"))?;
        let list_add = codegen
            .get_function_index("list.add")
            .ok_or_else(|| missing("list.add"))?;
        let list_add_f64 = codegen
            .get_function_index("list.add_f64")
            .ok_or_else(|| missing("list.add_f64"))?;

        let i32x = |n: usize| vec![WasmType::I32; n];

        // Byte-wise string ordering shared by the string variants.
        let str_cmp = register_stdlib_function_with_locals(
            codegen,
            "__list_str_cmp",
            &i32x(2),
            Some(WasmType::I32),
            &i32x(6),
            gen_str_cmp(),
        )?;

        // first / last
        for (name, last) in [("list.first", false), ("list.last", true)] {
            register_stdlib_function_with_locals(
                codegen,
                name,
                &i32x(1),
                Some(WasmType::I32),
                &i32x(1),
                gen_end_element(Elem::I32, last),
            )?;
        }
        for (name, body) in [
            ("__list_first_f64", gen_end_element(Elem::F64, false)),
            ("__list_last_f64", gen_end_element(Elem::F64, true)),
            ("__list_peek_f64", gen_peek_f64()),
        ] {
            register_stdlib_function_with_locals(
                codegen,
                name,
                &i32x(1),
                Some(WasmType::F64),
                &i32x(1),
                body,
            )?;
            codegen.record_wasm_return_type(name, Some(WasmType::F64));
        }

        // indexOf / lastIndexOf / contains. `list.indexOf`, `list.contains`
        // (i32) and `list.indexOf.string` / `list.contains.string` already
        // have native implementations.
        for (name, elem, from_end) in [
            ("list.lastIndexOf", Elem::I32, true),
            ("list.lastIndexOf.string", Elem::Str, true),
            ("list.indexOf.f64", Elem::F64, false),
            ("list.lastIndexOf.f64", Elem::F64, true),
        ] {
            register_stdlib_function_with_locals(
                codegen,
                name,
                &[WasmType::I32, elem.wasm()],
                Some(WasmType::I32),
                &i32x(2),
                gen_search(elem, from_end, Some(str_cmp)),
            )?;
        }
        let index_of_f64 = codegen
            .get_function_index("list.indexOf.f64")
            .ok_or_else(|| missing("list.indexOf.f64"))?;
        register_stdlib_function(
            codegen,
            "list.contains.f64",
            &[WasmType::I32, WasmType::F64],
            Some(WasmType::I32),
            gen_contains_via(index_of_f64),
        )?;

        // reverse
        let reverse = register_stdlib_function_with_locals(
            codegen,
            "list.reverse",
            &i32x(1),
            Some(WasmType::I32),
            &i32x(3),
            gen_reverse_i32(malloc),
        )?;
        codegen.add_function_alias("list_reverse", reverse);
        register_stdlib_function_with_locals(
            codegen,
            "list.reverse.f64",
            &i32x(1),
            Some(WasmType::I32),
            &i32x(3),
            gen_reverse_f64(malloc),
        )?;

        // slice
        let slice = register_stdlib_function_with_locals(
            codegen,
            "list.slice",
            &i32x(3),
            Some(WasmType::I32),
            &i32x(3),
            gen_slice_i32(malloc),
        )?;
        codegen.add_function_alias("list_slice", slice);
        register_stdlib_function_with_locals(
            codegen,
            "list.slice.f64",
            &i32x(3),
            Some(WasmType::I32),
            &i32x(5),
            gen_slice_f64(malloc),
        )?;

        // sort
        for (name, elem) in [
            ("list.sort", Elem::I32),
            ("list.sort.f64", Elem::F64),
            ("list.sort.string", Elem::Str),
        ] {
            let idx = register_stdlib_function_with_locals(
                codegen,
                name,
                &i32x(1),
                Some(WasmType::I32),
                &[
                    WasmType::I32,
                    WasmType::I32,
                    WasmType::I32,
                    WasmType::I32,
                    WasmType::I32,
                    elem.wasm(),
                ],
                gen_sort(malloc, elem, Some(str_cmp)),
            )?;
            if elem == Elem::I32 {
                codegen.add_function_alias("list_sort", idx);
            }
        }

        // insert — returns the possibly moved list pointer
        let insert = register_stdlib_function_with_locals(
            codegen,
            "list.insert",
            &i32x(3),
            Some(WasmType::I32),
            &i32x(4),
            gen_insert(Elem::I32, list_add),
        )?;
        codegen.add_function_alias("list_insert", insert);
        register_stdlib_function_with_locals(
            codegen,
            "list.insert.f64",
            &[WasmType::I32, WasmType::I32, WasmType::F64],
            Some(WasmType::I32),
            &i32x(4),
            gen_insert(Elem::F64, list_add_f64),
        )?;

        // remove(index), pop/removeLast and the behavior-aware `.remove()`.
        let i32_locals = i32x(3);
        let f64_locals = [WasmType::I32, WasmType::I32, WasmType::F64];
        for (name, elem, at) in [
            ("list.removeAt", Elem::I32, RemoveAt::Index),
            ("__list_removeAt_f64", Elem::F64, RemoveAt::Index),
            ("list.removeLast", Elem::I32, RemoveAt::Last),
            ("__list_removeLast_f64", Elem::F64, RemoveAt::Last),
            ("list.remove", Elem::I32, RemoveAt::Behavior),
            ("__list_remove_f64", Elem::F64, RemoveAt::Behavior),
        ] {
            let params = if at == RemoveAt::Index {
                i32x(2)
            } else {
                i32x(1)
            };
            // The index is a local for the parameterless forms.
            let mut locals: Vec<WasmType> = if at == RemoveAt::Index {
                Vec::new()
            } else {
                vec![WasmType::I32]
            };
            locals.extend_from_slice(if elem == Elem::F64 {
                &f64_locals
            } else {
                &i32_locals
            });
            register_stdlib_function_with_locals(
                codegen,
                name,
                &params,
                Some(elem.wasm()),
                &locals,
                gen_remove(elem, at),
            )?;
            if elem == Elem::F64 {
                codegen.record_wasm_return_type(name, Some(WasmType::F64));
            }
        }
        let pop = codegen
            .get_function_index("list.removeLast")
            .ok_or_else(|| missing("list.removeLast"))?;
        codegen.add_function_alias("list.pop", pop);

        // get / set with bounds checks; `list.set` replaces the earlier
        // registration in `list_ops.rs`.
        register_stdlib_function_with_locals(
            codegen,
            "__list_get_f64",
            &i32x(2),
            Some(WasmType::F64),
            &i32x(1),
            gen_get(Elem::F64),
        )?;
        codegen.record_wasm_return_type("__list_get_f64", Some(WasmType::F64));
        let set = register_stdlib_function_with_locals(
            codegen,
            "list.set",
            &i32x(3),
            None,
            &i32x(1),
            gen_set(Elem::I32),
        )?;
        codegen.add_function_alias("list_set", set);
        register_stdlib_function_with_locals(
            codegen,
            "list.set.f64",
            &[WasmType::I32, WasmType::I32, WasmType::F64],
            None,
            &i32x(1),
            gen_set(Elem::F64),
        )?;
        for name in ["list.set", "list.set.f64"] {
            codegen.record_wasm_return_type(name, None);
        }

        // concat
        let concat = register_stdlib_function_with_locals(
            codegen,
            "list.concat",
            &i32x(2),
            Some(WasmType::I32),
            &i32x(7),
            gen_concat(malloc, Elem::I32),
        )?;
        codegen.add_function_alias("list_concat", concat);
        register_stdlib_function_with_locals(
            codegen,
            "list.concat.f64",
            &i32x(2),
            Some(WasmType::I32),
            &i32x(7),
            gen_concat(malloc, Elem::F64),
        )?;

        // range / fill
        register_stdlib_function_with_locals(
            codegen,
            "list.range",
            &i32x(2),
            Some(WasmType::I32),
            &i32x(3),
            gen_range(malloc),
        )?;
        register_stdlib_function_with_locals(
            codegen,
            "list.fill",
            &i32x(2),
            Some(WasmType::I32),
            &i32x(3),
            gen_fill(malloc),
        )?;

        // join — `list.join(items, sep)` and `string.join(items, sep)` are
        // the same operation (stdlib-reference.md §5).
        let join = register_stdlib_function_with_locals(
            codegen,
            "list.join",
            &i32x(2),
            Some(WasmType::I32),
            &i32x(9),
            gen_join(malloc),
        )?;
        codegen.add_function_alias("string.join", join);
        codegen.add_function_alias("list_join", join);

        Ok(())
    }
}

fn missing(name: &str) -> CompilerError {
    CompilerError::codegen_error(
        format!(
            "list stdlib registration requires `{}` to be registered first",
            name
        ),
        None,
        None,
    )
}
