//! Matrix methods and operators implemented as native WASM functions
//! (06-expressions.md "Matrix Operations", stdlib-reference.md "Matrix
//! Methods").
//!
//! A `matrix<T>` is a list of `list<T>` rows in the standard list layout
//! (see `list_class.rs`): the matrix holds one 4-byte row pointer per row,
//! every row holds its elements (4 bytes for `integer`, 8 for `number`).
//!
//! Registered only when the program calls a `matrix.*` function
//! (`MirCodeGenerator::generate`). Element-typed variants are chosen by the
//! MIR builder: `matrix.<op>` for `integer` elements and `matrix.<op>.f64`
//! for `number` elements; `transpose` also has a `.i64` variant for
//! `integer:64` and works on any element type (`.f64`, `.i64`, otherwise the
//! 4-byte body).
//!
//! Errors are catchable runtime errors (`__err_raise`): sizes that do not
//! match, a matrix that is not square, a singular matrix, rows of different
//! length and `integer` overflow. After raising, a function returns zero at
//! once; the MIR builder's `ErrorCheck` unwinds to the handler.
//!
//! `determinant()` and `inverse()` use fraction-free (Bareiss) elimination:
//! for a matrix of whole numbers every intermediate value is a whole number,
//! so `[[1, 2], [3, 4]].determinant()` is exactly `-2` and its inverse is
//! exactly `[[-2, 1], [1.5, -0.5]]`.

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::list_class::{elem_addr, emit_alloc_fresh, mem, missing, Elem, DATA};
use crate::stdlib::register_stdlib_function_with_locals;
use crate::types::WasmType;
use wasm_encoder::{BlockType, Instruction, ValType};

type Code = Vec<Instruction<'static>>;

/// The runtime function giving the column count of a matrix, or -1 when its
/// rows differ in length.
pub const MATRIX_COLS_CHECKED: &str = "__matrix_cols";

const MSG_SIZES: &str = "matrix sizes do not match";
const MSG_NOT_SQUARE: &str = "matrix is not square";
const MSG_SINGULAR: &str = "matrix is singular";
const MSG_RAGGED: &str = "matrix rows differ in length";
const MSG_OVERFLOW: &str = "integer overflow";

/// Names of the matrix functions that return a matrix (an i32 pointer).
pub const MATRIX_POINTER_FUNCTIONS: &[&str] = &[
    "matrix.transpose",
    "matrix.transpose.f64",
    "matrix.transpose.i64",
    "matrix.add",
    "matrix.add.f64",
    "matrix.sub",
    "matrix.sub.f64",
    "matrix.mul",
    "matrix.mul.f64",
    "matrix.inverse",
    "matrix.inverse.f64",
];

/// Names of the matrix functions that return an f64.
pub const MATRIX_NUMBER_FUNCTIONS: &[&str] = &["matrix.determinant", "matrix.determinant.f64"];

/// Names of the matrix functions that return an i32 count.
pub const MATRIX_COUNT_FUNCTIONS: &[&str] = &["matrix.rows", "matrix.cols"];

/// Function indices and message pointers the bodies call.
#[derive(Clone, Copy)]
struct Env {
    malloc: u32,
    err_raise: u32,
    cols_checked: u32,
    sizes: u32,
    not_square: u32,
    singular: u32,
    ragged: u32,
    overflow: u32,
}

/// Local-variable allocator for one function body.
struct Frame {
    params: u32,
    locals: Vec<WasmType>,
}

impl Frame {
    fn new(params: u32) -> Self {
        Self {
            params,
            locals: Vec::new(),
        }
    }

    fn local(&mut self, ty: WasmType) -> u32 {
        self.locals.push(ty);
        self.params + self.locals.len() as u32 - 1
    }

    fn i32(&mut self) -> u32 {
        self.local(WasmType::I32)
    }

    fn f64(&mut self) -> u32 {
        self.local(WasmType::F64)
    }

    fn i64(&mut self) -> u32 {
        self.local(WasmType::I64)
    }
}

fn get(local: u32) -> Instruction<'static> {
    Instruction::LocalGet(local)
}

fn set(local: u32) -> Instruction<'static> {
    Instruction::LocalSet(local)
}

/// `for counter in start..limit` (the body must not branch).
fn for_range(counter: u32, start: Code, limit: u32, body: Code) -> Code {
    let mut code = start;
    code.extend([
        set(counter),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        get(counter),
        get(limit),
        Instruction::I32GeS,
        Instruction::BrIf(1),
    ]);
    code.extend(body);
    code.extend([
        get(counter),
        Instruction::I32Const(1),
        Instruction::I32Add,
        set(counter),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
    ]);
    code
}

fn zero_to(counter: u32, limit: u32, body: Code) -> Code {
    for_range(counter, vec![Instruction::I32Const(0)], limit, body)
}

/// Raise `message` and return `zero` from the function.
fn raise_return(message: u32, err_raise: u32, zero: Instruction<'static>) -> Code {
    vec![
        Instruction::I32Const(message as i32),
        Instruction::Call(err_raise),
        zero,
        Instruction::Return,
    ]
}

/// Push the row pointer `matrix[index]`.
fn row(matrix: u32, index: u32) -> Code {
    vec![
        get(matrix),
        get(index),
        Instruction::I32Const(4),
        Instruction::I32Mul,
        Instruction::I32Add,
        Instruction::I32Load(mem(DATA as u64, 2)),
    ]
}

/// `matrix[index] = value_local`.
fn store_row(matrix: u32, index: u32, value: u32) -> Code {
    vec![
        get(matrix),
        get(index),
        Instruction::I32Const(4),
        Instruction::I32Mul,
        Instruction::I32Add,
        get(value),
        Instruction::I32Store(mem(DATA as u64, 2)),
    ]
}

fn length(list: u32) -> Code {
    vec![get(list), Instruction::I32Load(mem(0, 2))]
}

/// `dst = cols(matrix)`, raising "rows differ in length" for a ragged one.
fn cols_checked(env: Env, matrix: u32, dst: u32, zero: Instruction<'static>) -> Code {
    let mut code = vec![
        get(matrix),
        Instruction::Call(env.cols_checked),
        Instruction::LocalTee(dst),
        Instruction::I32Const(0),
        Instruction::I32LtS,
        Instruction::If(BlockType::Empty),
    ];
    code.extend(raise_return(env.ragged, env.err_raise, zero));
    code.push(Instruction::End);
    code
}

/// Raise "integer overflow" unless the i64 in `acc` fits an i32.
fn range_check(env: Env, acc: u32) -> Code {
    let mut code = vec![
        get(acc),
        get(acc),
        Instruction::I32WrapI64,
        Instruction::I64ExtendI32S,
        Instruction::I64Ne,
        Instruction::If(BlockType::Empty),
    ];
    code.extend(raise_return(
        env.overflow,
        env.err_raise,
        Instruction::I32Const(0),
    ));
    code.push(Instruction::End);
    code
}

/// `__matrix_cols(m)`: the length of every row, 0 for a matrix without rows,
/// -1 when the rows differ in length.
fn gen_cols_checked() -> (Frame, Code) {
    let mut f = Frame::new(1);
    let (rows, cols, i) = (f.i32(), f.i32(), f.i32());
    let mut code = length(0);
    code.push(set(rows));
    code.extend([
        get(rows),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Return,
        Instruction::End,
        get(0),
        Instruction::I32Load(mem(DATA as u64, 2)),
        Instruction::I32Load(mem(0, 2)),
        set(cols),
    ]);
    let mut body = row(0, i);
    body.extend([
        Instruction::I32Load(mem(0, 2)),
        get(cols),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(-1),
        Instruction::Return,
        Instruction::End,
    ]);
    code.extend(for_range(i, vec![Instruction::I32Const(1)], rows, body));
    code.push(get(cols));
    (f, code)
}

/// `matrix.cols(m)`: the length of the first row, 0 without rows.
fn gen_cols() -> Code {
    vec![
        get(0),
        Instruction::I32Load(mem(0, 2)),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Result(ValType::I32)),
        Instruction::I32Const(0),
        Instruction::Else,
        get(0),
        Instruction::I32Load(mem(DATA as u64, 2)),
        Instruction::I32Load(mem(0, 2)),
        Instruction::End,
    ]
}

/// `transpose`: the columns of `m` as rows.
fn gen_transpose(env: Env, elem: Elem) -> (Frame, Code) {
    let mut f = Frame::new(1);
    let (c, r, res, i, j, rp, newrow) = (
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
    );
    let mut code = cols_checked(env, 0, c, Instruction::I32Const(0));
    code.extend(length(0));
    code.push(set(r));
    code.extend(emit_alloc_fresh(env.malloc, c, res, 4));
    let mut copy = row(0, i);
    copy.push(set(rp));
    copy.extend(elem_addr(newrow, i, elem));
    copy.extend(elem_addr(rp, j, elem));
    copy.push(elem.load());
    copy.push(elem.store());
    let mut per_column = emit_alloc_fresh(env.malloc, r, newrow, elem.stride());
    per_column.extend(store_row(res, j, newrow));
    per_column.extend(zero_to(i, r, copy));
    code.extend(zero_to(j, c, per_column));
    code.push(get(res));
    (f, code)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
}

/// `a + b` / `a - b`: element by element, for matrices of the same size.
fn gen_add_sub(env: Env, elem: Elem, op: Op) -> (Frame, Code) {
    let mut f = Frame::new(2);
    let (ca, cb, r, res, i, j, rowa, rowb, newrow) = (
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
    );
    let acc = (elem == Elem::I32).then(|| f.i64());
    let zero = || Instruction::I32Const(0);
    let mut code = cols_checked(env, 0, ca, zero());
    code.extend(cols_checked(env, 1, cb, zero()));
    code.extend(length(0));
    code.push(set(r));
    code.extend([
        get(r),
        get(1),
        Instruction::I32Load(mem(0, 2)),
        Instruction::I32Ne,
        get(ca),
        get(cb),
        Instruction::I32Ne,
        Instruction::I32Or,
        Instruction::If(BlockType::Empty),
    ]);
    code.extend(raise_return(env.sizes, env.err_raise, zero()));
    code.push(Instruction::End);
    code.extend(emit_alloc_fresh(env.malloc, r, res, 4));

    let mut cell = Vec::new();
    if let Some(acc) = acc {
        cell.extend(elem_addr(rowa, j, elem));
        cell.extend([elem.load(), Instruction::I64ExtendI32S]);
        cell.extend(elem_addr(rowb, j, elem));
        cell.extend([
            elem.load(),
            Instruction::I64ExtendI32S,
            if op == Op::Add {
                Instruction::I64Add
            } else {
                Instruction::I64Sub
            },
            set(acc),
        ]);
        cell.extend(range_check(env, acc));
        cell.extend(elem_addr(newrow, j, elem));
        cell.extend([get(acc), Instruction::I32WrapI64, elem.store()]);
    } else {
        cell.extend(elem_addr(newrow, j, elem));
        cell.extend(elem_addr(rowa, j, elem));
        cell.push(elem.load());
        cell.extend(elem_addr(rowb, j, elem));
        cell.extend([
            elem.load(),
            if op == Op::Add {
                Instruction::F64Add
            } else {
                Instruction::F64Sub
            },
            elem.store(),
        ]);
    }
    let mut per_row = row(0, i);
    per_row.push(set(rowa));
    per_row.extend(row(1, i));
    per_row.push(set(rowb));
    per_row.extend(emit_alloc_fresh(env.malloc, ca, newrow, elem.stride()));
    per_row.extend(store_row(res, i, newrow));
    per_row.extend(zero_to(j, ca, cell));
    code.extend(zero_to(i, r, per_row));
    code.push(get(res));
    (f, code)
}

/// `a * b`: the matrix product; `a` must have as many columns as `b` rows.
fn gen_mul(env: Env, elem: Elem) -> (Frame, Code) {
    let mut f = Frame::new(2);
    let (ca, cb, ra, rb, res, i, j, k, rowa, rowb, newrow) = (
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
    );
    let acc = if elem == Elem::I32 { f.i64() } else { f.f64() };
    let zero = || Instruction::I32Const(0);
    let mut code = cols_checked(env, 0, ca, zero());
    code.extend(cols_checked(env, 1, cb, zero()));
    code.extend(length(0));
    code.push(set(ra));
    code.extend(length(1));
    code.push(set(rb));
    code.extend([
        get(ca),
        get(rb),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
    ]);
    code.extend(raise_return(env.sizes, env.err_raise, zero()));
    code.push(Instruction::End);
    code.extend(emit_alloc_fresh(env.malloc, ra, res, 4));

    // One step of the sum: acc += a[i][k] * b[k][j].
    let mut step = row(1, k);
    step.push(set(rowb));
    step.push(get(acc));
    step.extend(elem_addr(rowa, k, elem));
    step.push(elem.load());
    if elem == Elem::I32 {
        step.push(Instruction::I64ExtendI32S);
    }
    step.extend(elem_addr(rowb, j, elem));
    step.push(elem.load());
    if elem == Elem::I32 {
        step.extend([
            Instruction::I64ExtendI32S,
            Instruction::I64Mul,
            Instruction::I64Add,
            set(acc),
        ]);
        step.extend(range_check(env, acc));
    } else {
        step.extend([Instruction::F64Mul, Instruction::F64Add, set(acc)]);
    }
    let mut per_cell = vec![
        if elem == Elem::I32 {
            Instruction::I64Const(0)
        } else {
            Instruction::F64Const(0.0)
        },
        set(acc),
    ];
    per_cell.extend(zero_to(k, ca, step));
    per_cell.extend(elem_addr(newrow, j, elem));
    per_cell.push(get(acc));
    if elem == Elem::I32 {
        per_cell.push(Instruction::I32WrapI64);
    }
    per_cell.push(elem.store());

    let mut per_row = row(0, i);
    per_row.push(set(rowa));
    per_row.extend(emit_alloc_fresh(env.malloc, cb, newrow, elem.stride()));
    per_row.extend(store_row(res, i, newrow));
    per_row.extend(zero_to(j, cb, per_cell));
    code.extend(zero_to(i, ra, per_row));
    code.push(get(res));
    (f, code)
}

/// Address of `w[i][j]` in the working copy (`width` values of 8 bytes per row).
fn w_addr(w: u32, i: u32, width: u32, j: u32) -> Code {
    vec![
        get(w),
        get(i),
        get(width),
        Instruction::I32Mul,
        get(j),
        Instruction::I32Add,
        Instruction::I32Const(8),
        Instruction::I32Mul,
        Instruction::I32Add,
    ]
}

fn w_load(w: u32, i: u32, width: u32, j: u32) -> Code {
    let mut code = w_addr(w, i, width, j);
    code.push(Instruction::F64Load(mem(0, 3)));
    code
}

/// `determinant` (returns an f64) or `inverse` (returns a matrix of f64) of a
/// square matrix, by fraction-free elimination with partial pivoting on a
/// working copy of `n` rows and `width` values per row (`n` for the
/// determinant, `2 * n` for the inverse: the matrix next to the identity).
fn gen_gauss(env: Env, elem: Elem, inverse: bool) -> (Frame, Code) {
    let mut f = Frame::new(1);
    let (c, n, width, w, i, j, k, p, rp, res, newrow) = (
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
        f.i32(),
    );
    let (sign, prev, pivot, maxv, v, factor, tmp) = (
        f.f64(),
        f.f64(),
        f.f64(),
        f.f64(),
        f.f64(),
        f.f64(),
        f.f64(),
    );
    let zero = || {
        if inverse {
            Instruction::I32Const(0)
        } else {
            Instruction::F64Const(0.0)
        }
    };

    let mut code = cols_checked(env, 0, c, zero());
    code.extend(length(0));
    code.push(set(n));
    code.extend([
        get(n),
        get(c),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
    ]);
    code.extend(raise_return(env.not_square, env.err_raise, zero()));
    code.push(Instruction::End);

    // The empty matrix: determinant 1, inverse `[]`.
    code.extend([
        get(n),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
    ]);
    if inverse {
        code.extend(emit_alloc_fresh(env.malloc, n, res, 4));
        code.push(get(res));
    } else {
        code.push(Instruction::F64Const(1.0));
    }
    code.extend([Instruction::Return, Instruction::End]);

    // width = n (determinant) or 2n (inverse); the working copy.
    code.push(get(n));
    if inverse {
        code.extend([Instruction::I32Const(2), Instruction::I32Mul]);
    }
    code.push(set(width));
    code.extend([
        get(n),
        get(width),
        Instruction::I32Mul,
        Instruction::I32Const(8),
        Instruction::I32Mul,
        Instruction::Call(env.malloc),
        set(w),
    ]);

    // Copy the matrix (and the identity beside it) into the working copy.
    let mut copy_cell = w_addr(w, i, width, j);
    copy_cell.extend(elem_addr(rp, j, elem));
    copy_cell.push(elem.load());
    if elem == Elem::I32 {
        copy_cell.push(Instruction::F64ConvertI32S);
    }
    copy_cell.push(Instruction::F64Store(mem(0, 3)));
    let mut copy_row = row(0, i);
    copy_row.push(set(rp));
    copy_row.extend(zero_to(j, n, copy_cell));
    if inverse {
        // w[i][n + j] = (i == j) ? 1.0 : 0.0
        let identity_cell = vec![
            get(w),
            get(i),
            get(width),
            Instruction::I32Mul,
            get(n),
            Instruction::I32Add,
            get(j),
            Instruction::I32Add,
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            get(i),
            get(j),
            Instruction::I32Eq,
            Instruction::F64ConvertI32S,
            Instruction::F64Store(mem(0, 3)),
        ];
        copy_row.extend(zero_to(j, n, identity_cell));
    }
    code.extend(zero_to(i, n, copy_row));

    code.extend([
        Instruction::F64Const(1.0),
        Instruction::LocalSet(sign),
        Instruction::F64Const(1.0),
        Instruction::LocalSet(prev),
    ]);

    // Pivot search: the row at or below k with the largest |w[r][k]|.
    let mut search_body = w_load(w, i, width, k);
    search_body.extend([
        Instruction::F64Abs,
        Instruction::LocalSet(v),
        get(v),
        get(maxv),
        Instruction::F64Gt,
        Instruction::If(BlockType::Empty),
        get(v),
        set(maxv),
        get(i),
        set(p),
        Instruction::End,
    ]);
    let mut step = vec![get(k), set(p)];
    step.extend(w_load(w, k, width, k));
    step.extend([Instruction::F64Abs, set(maxv)]);
    step.extend(for_range(
        i,
        vec![get(k), Instruction::I32Const(1), Instruction::I32Add],
        n,
        search_body,
    ));
    step.extend([get(maxv), Instruction::F64Const(0.0), Instruction::F64Eq]);
    step.push(Instruction::If(BlockType::Empty));
    if inverse {
        step.extend(raise_return(env.singular, env.err_raise, zero()));
    } else {
        step.extend([Instruction::F64Const(0.0), Instruction::Return]);
    }
    step.push(Instruction::End);

    // Swap row p into place.
    let mut swap_cell = w_load(w, k, width, j);
    swap_cell.push(set(tmp));
    swap_cell.extend(w_addr(w, k, width, j));
    swap_cell.extend(w_load(w, p, width, j));
    swap_cell.push(Instruction::F64Store(mem(0, 3)));
    swap_cell.extend(w_addr(w, p, width, j));
    swap_cell.extend([get(tmp), Instruction::F64Store(mem(0, 3))]);
    step.extend([
        get(p),
        get(k),
        Instruction::I32Ne,
        Instruction::If(BlockType::Empty),
    ]);
    step.extend(zero_to(j, width, swap_cell));
    step.extend([
        Instruction::F64Const(0.0),
        get(sign),
        Instruction::F64Sub,
        set(sign),
        Instruction::End,
    ]);

    // Eliminate: w[i][j] = (pivot * w[i][j] - factor * w[k][j]) / prev.
    step.extend(w_load(w, k, width, k));
    step.push(set(pivot));
    let mut update = w_addr(w, i, width, j);
    update.push(get(pivot));
    update.extend(w_load(w, i, width, j));
    update.push(Instruction::F64Mul);
    update.push(get(factor));
    update.extend(w_load(w, k, width, j));
    update.extend([
        Instruction::F64Mul,
        Instruction::F64Sub,
        get(prev),
        Instruction::F64Div,
        Instruction::F64Store(mem(0, 3)),
    ]);
    let mut per_row = w_load(w, i, width, k);
    per_row.push(set(factor));
    if inverse {
        per_row.extend(zero_to(j, width, update));
        let mut guarded = vec![
            get(i),
            get(k),
            Instruction::I32Ne,
            Instruction::If(BlockType::Empty),
        ];
        guarded.extend(per_row);
        guarded.push(Instruction::End);
        step.extend(zero_to(i, n, guarded));
    } else {
        per_row.extend(for_range(
            j,
            vec![get(k), Instruction::I32Const(1), Instruction::I32Add],
            width,
            update,
        ));
        step.extend(for_range(
            i,
            vec![get(k), Instruction::I32Const(1), Instruction::I32Add],
            n,
            per_row,
        ));
    }
    step.extend([get(pivot), set(prev)]);
    code.extend(zero_to(k, n, step));

    if inverse {
        // The right half divided by the last pivot.
        let mut cell = elem_addr(newrow, j, Elem::F64);
        cell.extend([
            get(w),
            get(i),
            get(width),
            Instruction::I32Mul,
            get(n),
            Instruction::I32Add,
            get(j),
            Instruction::I32Add,
            Instruction::I32Const(8),
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::F64Load(mem(0, 3)),
            get(prev),
            Instruction::F64Div,
            Elem::F64.store(),
        ]);
        let mut per_row = emit_alloc_fresh(env.malloc, n, newrow, 8);
        per_row.extend(store_row(res, i, newrow));
        per_row.extend(zero_to(j, n, cell));
        code.extend(emit_alloc_fresh(env.malloc, n, res, 4));
        code.extend(zero_to(i, n, per_row));
        code.push(get(res));
    } else {
        // The last pivot is the determinant, up to the row swaps.
        let last = vec![
            get(n),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            set(k),
        ];
        code.extend(last);
        code.extend(w_load(w, k, width, k));
        code.extend([get(sign), Instruction::F64Mul]);
    }
    (f, code)
}

impl Env {
    fn new(codegen: &mut CodeGenerator) -> Result<Self, CompilerError> {
        Ok(Self {
            malloc: codegen
                .get_function_index("__malloc")
                .ok_or_else(|| missing("__malloc"))?,
            err_raise: codegen
                .get_function_index("__err_raise")
                .ok_or_else(|| missing("__err_raise"))?,
            cols_checked: 0,
            sizes: codegen.get_or_create_string_offset(MSG_SIZES)?,
            not_square: codegen.get_or_create_string_offset(MSG_NOT_SQUARE)?,
            singular: codegen.get_or_create_string_offset(MSG_SINGULAR)?,
            ragged: codegen.get_or_create_string_offset(MSG_RAGGED)?,
            overflow: codegen.get_or_create_string_offset(MSG_OVERFLOW)?,
        })
    }
}

/// Register every matrix function. Runs after the list operations
/// (`__malloc`) and the error runtime (`__err_raise`).
pub fn register_functions(codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
    let mut env = Env::new(codegen)?;
    let i32x = |n: usize| vec![WasmType::I32; n];

    let (frame, body) = gen_cols_checked();
    env.cols_checked = register_stdlib_function_with_locals(
        codegen,
        MATRIX_COLS_CHECKED,
        &i32x(1),
        Some(WasmType::I32),
        &frame.locals,
        body,
    )?;

    register_stdlib_function_with_locals(
        codegen,
        "matrix.rows",
        &i32x(1),
        Some(WasmType::I32),
        &[],
        length(0),
    )?;
    register_stdlib_function_with_locals(
        codegen,
        "matrix.cols",
        &i32x(1),
        Some(WasmType::I32),
        &[],
        gen_cols(),
    )?;

    let register = |codegen: &mut CodeGenerator,
                    name: &str,
                    params: usize,
                    ret: WasmType,
                    (frame, body): (Frame, Code)| {
        register_stdlib_function_with_locals(
            codegen,
            name,
            &i32x(params),
            Some(ret),
            &frame.locals,
            body,
        )
    };

    for (name, elem) in [
        ("matrix.transpose", Elem::I32),
        ("matrix.transpose.f64", Elem::F64),
        ("matrix.transpose.i64", Elem::I64),
    ] {
        register(codegen, name, 1, WasmType::I32, gen_transpose(env, elem))?;
    }
    for (name, elem, op) in [
        ("matrix.add", Elem::I32, Op::Add),
        ("matrix.add.f64", Elem::F64, Op::Add),
        ("matrix.sub", Elem::I32, Op::Sub),
        ("matrix.sub.f64", Elem::F64, Op::Sub),
    ] {
        register(codegen, name, 2, WasmType::I32, gen_add_sub(env, elem, op))?;
    }
    for (name, elem) in [("matrix.mul", Elem::I32), ("matrix.mul.f64", Elem::F64)] {
        register(codegen, name, 2, WasmType::I32, gen_mul(env, elem))?;
    }
    for (name, elem) in [
        ("matrix.determinant", Elem::I32),
        ("matrix.determinant.f64", Elem::F64),
    ] {
        register(codegen, name, 1, WasmType::F64, gen_gauss(env, elem, false))?;
    }
    for (name, elem) in [
        ("matrix.inverse", Elem::I32),
        ("matrix.inverse.f64", Elem::F64),
    ] {
        register(codegen, name, 1, WasmType::I32, gen_gauss(env, elem, true))?;
    }
    Ok(())
}
