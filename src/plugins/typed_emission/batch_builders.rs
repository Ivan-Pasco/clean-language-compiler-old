/// Batch-spec builder bridge registration — Amendment 4 §3.14.
///
/// Provides ~27 builder bridges accessible only inside the typed-emission linker
/// sandbox. Plugins call these to compose `BatchSpec` / `BatchBuilderClass`
/// values using typed handles instead of JSON strings, eliminating the
/// JSON-in-Clean pattern (bug #4790d26673d1 / #52fea4ceb01e).
///
/// Dual-name registration (bug #e8ad1944445f fix):
/// Every builder is registered under BOTH its dotted name (e.g. `"batch.stringLit"`,
/// for internal registry consistency) AND its underscore alias (e.g. `"_batch_stringLit"`,
/// for use in Clean Language `external:` declarations, which reject dots in import names).
/// The underscore aliases are registered via `linker.alias` after all dotted functions
/// are set up, mirroring the Piece A pattern used by `register_plugin_stdlib_functions`.
///
/// `_batch_field` and `_batch_classField` are distinct aliases that avoid the
/// naming collision between `batch.field` (field access expression) and
/// `batch.classField` (class member declaration) noted in Amendment 4.
///
/// Handle namespace: batch builder handles have BATCH_TAG (bit 24) set.
/// The widened `_emit_helpers_batch` / `_emit_class_full` in bridges.rs
/// inspect this bit to distinguish a batch spec handle from an LP pointer.
use anyhow::Result;
use wasmtime::{Caller, Linker};

use super::arena::{BatchArray, BatchArrayKind, BatchArrayPushError, BatchBuilderClass, BatchNode};
use super::batch_schema::{
    BatchExpr, BatchField, BatchFunction, BatchMethod, BatchObjectField, BatchParam, BatchSpec,
    BatchStatement,
};
use super::error::{EmitDiagnostic, EmitError};
use crate::plugins::wasm_adapter::PluginState;

// ─── Arena access macro (mirrors bridges.rs) ─────────────────────────────────

macro_rules! arena {
    ($caller:expr) => {
        $caller
            .data_mut()
            .emit_arena
            .as_mut()
            .expect("typed-emission: arena must be installed before bridge calls")
    };
}

// ─────────────────────────────────────────────────────────────────────────────
// LP-string reading helper (duplicated minimally — bridges.rs is pub(crate) but
// read_lp_string is fn-private there; simpler to inline than expose).
// ─────────────────────────────────────────────────────────────────────────────

fn read_lp_string(caller: &mut Caller<'_, PluginState>, ptr: i32) -> Option<String> {
    if ptr < 0 {
        return None;
    }
    let memory = caller.get_export("memory")?.into_memory()?;
    let data = memory.data(caller);
    let start = ptr as usize;
    if start + 4 > data.len() {
        return None;
    }
    let len_bytes: [u8; 4] = data[start..start + 4].try_into().ok()?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let data_end = start + 4 + len;
    if data_end > data.len() {
        return None;
    }
    std::str::from_utf8(&data[start + 4..data_end])
        .ok()
        .map(|s| s.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// PLUGIN013 diagnostic helper for builder errors
// ─────────────────────────────────────────────────────────────────────────────

fn emit_batch_plugin013(arena: &mut super::arena::EmitArena, message: impl Into<String>) {
    arena.emit_diagnostic(EmitDiagnostic {
        severity: 2,
        code: "PLUGIN013".to_string(),
        message: message.into(),
        // Batch builder errors use source="plugin" per Amendment 4 §3.14 error table.
        span_json: r#"{"start":0,"end":0,"source":"plugin"}"#.to_string(),
    });
}

fn emit_batch_plugin008(arena: &mut super::arena::EmitArena, handle: i32) {
    arena.emit_plugin008(handle);
}

/// Handle an `EmitError` from a batch arena operation — emits PLUGIN008 for
/// consumed handles, PLUGIN013 for all other errors. Returns `0` (error sentinel).
macro_rules! handle_batch_err {
    ($arena:expr, $result:expr) => {
        match $result {
            Ok(v) => v,
            Err(EmitError::HandleConsumed { handle }) => {
                emit_batch_plugin008($arena, handle);
                return 0;
            }
            Err(e) => {
                emit_batch_plugin013($arena, format!("batch builder arena error: {:?}", e));
                return 0;
            }
        }
    };
}

/// Handle `BatchArrayPushError`, emitting the right code and returning 0.
macro_rules! handle_push_err {
    ($arena:expr, $result:expr) => {
        match $result {
            Ok(v) => v,
            Err(BatchArrayPushError::ArrayConsumed { handle })
            | Err(BatchArrayPushError::ItemConsumed { handle }) => {
                emit_batch_plugin008($arena, handle);
                return 1; // arrayPush returns err on failure, not handle
            }
            Err(e) => {
                emit_batch_plugin013($arena, e.message());
                return 1;
            }
        }
    };
}

// ─────────────────────────────────────────────────────────────────────────────
// Public registration entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Register all batch-spec builder bridges onto `linker`.
/// Called from `register_typed_emission_bridges` in bridges.rs.
///
/// All 27 dotted names are registered first, then underscore aliases are
/// added via `linker.alias` (bug #e8ad1944445f — plugins need `_batch_*` names
/// because Clean Language's `external:` declarations reject dots in import names).
pub fn register_batch_builder_bridges(linker: &mut Linker<PluginState>) -> Result<()> {
    register_array_helpers(linker)?;
    register_expr_builders(linker)?;
    register_stmt_builders(linker)?;
    register_func_class_builders(linker)?;
    register_underscore_aliases(linker)?;
    Ok(())
}

/// Register `_batch_<name>` underscore aliases for all 27 builder bridges.
///
/// Each alias points to the already-registered `batch.<name>` function via
/// `linker.alias` — zero code duplication, identical runtime semantics.
/// The underscore form is what Clean Language plugins declare in `external:`:
///
/// ```clean
/// external: integer _batch_stringLit(integer ctx, string value)
/// ```
///
/// Note: `_batch_field` aliases `batch.field` (field access expression);
/// `_batch_classField` aliases `batch.classField` (class member declaration).
/// These are two distinct bridges per the Amendment 4 collision note.
fn register_underscore_aliases(linker: &mut Linker<PluginState>) -> Result<()> {
    // The complete list of 27 builders: dotted name → underscore alias.
    let aliases: &[(&str, &str)] = &[
        // Array helpers (2)
        ("batch.arrayNew", "_batch_arrayNew"),
        ("batch.arrayPush", "_batch_arrayPush"),
        // Expression builders (12)
        ("batch.stringLit", "_batch_stringLit"),
        ("batch.intLit", "_batch_intLit"),
        ("batch.numberLit", "_batch_numberLit"),
        ("batch.boolLit", "_batch_boolLit"),
        ("batch.ident", "_batch_ident"),
        ("batch.field", "_batch_field"),
        ("batch.call", "_batch_call"),
        ("batch.binop", "_batch_binop"),
        ("batch.unop", "_batch_unop"),
        ("batch.index", "_batch_index"),
        ("batch.arrayLit", "_batch_arrayLit"),
        ("batch.objectLit", "_batch_objectLit"),
        // Statement builders (7)
        ("batch.stmtCall", "_batch_stmtCall"),
        ("batch.stmtAssign", "_batch_stmtAssign"),
        ("batch.stmtIf", "_batch_stmtIf"),
        ("batch.stmtWhile", "_batch_stmtWhile"),
        ("batch.stmtFor", "_batch_stmtFor"),
        ("batch.stmtReturn", "_batch_stmtReturn"),
        ("batch.stmtBlock", "_batch_stmtBlock"),
        // Function/class/terminal builders (7)
        ("batch.func", "_batch_func"),
        ("batch.classField", "_batch_classField"),
        ("batch.method", "_batch_method"),
        ("batch.class", "_batch_class"),
        ("batch.param", "_batch_param"),
        ("batch.objectField", "_batch_objectField"),
        ("batch.spec", "_batch_spec"),
    ];

    for (dotted, underscore) in aliases {
        linker.alias("env", dotted, "env", underscore)?;
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Array helpers — batch.arrayNew, batch.arrayPush
// ─────────────────────────────────────────────────────────────────────────────

fn register_array_helpers(linker: &mut Linker<PluginState>) -> Result<()> {
    // batch.arrayNew(ctx) -> array_handle
    linker.func_wrap(
        "env",
        "batch.arrayNew",
        |mut caller: Caller<'_, PluginState>, ctx: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_batch(BatchNode::Array(BatchArray {
                kind: BatchArrayKind::Unset,
                items: Vec::new(),
            }))
        },
    )?;

    // batch.arrayPush(ctx, array_handle, item_handle) -> err (0 = success, 1 = error)
    linker.func_wrap(
        "env",
        "batch.arrayPush",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         array_handle: i32,
         item_handle: i32|
         -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 1;
            }
            handle_push_err!(a, a.batch_array_push(array_handle, item_handle));
            0
        },
    )?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Expression builders (12 bridges)
// ─────────────────────────────────────────────────────────────────────────────

fn register_expr_builders(linker: &mut Linker<PluginState>) -> Result<()> {
    // batch.stringLit(ctx, value_lp) -> batch_expr_handle
    linker.func_wrap(
        "env",
        "batch.stringLit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, value_lp: i32| -> i32 {
            let value = match read_lp_string(&mut caller, value_lp) {
                Some(s) => s,
                None => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_batch(BatchNode::Expr(BatchExpr::StringLit { value }))
        },
    )?;

    // batch.intLit(ctx, value_lo, value_hi) -> batch_expr_handle
    // Reassemble i64 from two i32 halves: value = (hi << 32) | (lo as u32)
    linker.func_wrap(
        "env",
        "batch.intLit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, lo: i32, hi: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let value = ((hi as i64) << 32) | (lo as u32 as i64);
            a.alloc_batch(BatchNode::Expr(BatchExpr::IntLit { value }))
        },
    )?;

    // batch.numberLit(ctx, bits_lo, bits_hi) -> batch_expr_handle
    // f64 bits encoded as two i32s: (hi << 32) | (lo as u32)
    linker.func_wrap(
        "env",
        "batch.numberLit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, bits_lo: i32, bits_hi: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let bits = ((bits_hi as u64) << 32) | (bits_lo as u32 as u64);
            let value = f64::from_bits(bits);
            a.alloc_batch(BatchNode::Expr(BatchExpr::NumberLit { value }))
        },
    )?;

    // batch.boolLit(ctx, value) -> batch_expr_handle  (value: 0=false, non-zero=true)
    linker.func_wrap(
        "env",
        "batch.boolLit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, value: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_batch(BatchNode::Expr(BatchExpr::BoolLit { value: value != 0 }))
        },
    )?;

    // batch.ident(ctx, name_lp) -> batch_expr_handle
    linker.func_wrap(
        "env",
        "batch.ident",
        |mut caller: Caller<'_, PluginState>, ctx: i32, name_lp: i32| -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_batch(BatchNode::Expr(BatchExpr::Ident { name }))
        },
    )?;

    // batch.field(ctx, receiver_handle, name_lp) -> batch_expr_handle
    linker.func_wrap(
        "env",
        "batch.field",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         receiver_handle: i32,
         name_lp: i32|
         -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let receiver = handle_batch_err!(a, a.take_batch_expr(receiver_handle));
            a.alloc_batch(BatchNode::Expr(BatchExpr::Field {
                receiver: Box::new(receiver),
                name,
            }))
        },
    )?;

    // batch.call(ctx, callee_lp, args_array_handle) -> batch_expr_handle
    linker.func_wrap(
        "env",
        "batch.call",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         callee_lp: i32,
         args_array_handle: i32|
         -> i32 {
            let callee = match read_lp_string(&mut caller, callee_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let args = if args_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(args_array_handle));
                handle_batch_err!(a, a.drain_batch_exprs(items))
            };
            a.alloc_batch(BatchNode::Expr(BatchExpr::Call { callee, args }))
        },
    )?;

    // batch.binop(ctx, op_lp, lhs_handle, rhs_handle) -> batch_expr_handle
    linker.func_wrap(
        "env",
        "batch.binop",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         op_lp: i32,
         lhs_handle: i32,
         rhs_handle: i32|
         -> i32 {
            let op = match read_lp_string(&mut caller, op_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            // Validate op string against the same resolver as the JSON path.
            if super::arena::map_binary_op(&op).is_none() {
                emit_batch_plugin013(a, format!("batch.binop: unknown operator `{}`", op));
                return 0;
            }
            let lhs = handle_batch_err!(a, a.take_batch_expr(lhs_handle));
            let rhs = handle_batch_err!(a, a.take_batch_expr(rhs_handle));
            a.alloc_batch(BatchNode::Expr(BatchExpr::BinOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            }))
        },
    )?;

    // batch.unop(ctx, op_lp, operand_handle) -> batch_expr_handle
    linker.func_wrap(
        "env",
        "batch.unop",
        |mut caller: Caller<'_, PluginState>, ctx: i32, op_lp: i32, operand_handle: i32| -> i32 {
            let op = match read_lp_string(&mut caller, op_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            if super::arena::map_unary_op(&op).is_none() {
                emit_batch_plugin013(a, format!("batch.unop: unknown operator `{}`", op));
                return 0;
            }
            let operand = handle_batch_err!(a, a.take_batch_expr(operand_handle));
            a.alloc_batch(BatchNode::Expr(BatchExpr::UnOp {
                op,
                operand: Box::new(operand),
            }))
        },
    )?;

    // batch.index(ctx, receiver_handle, index_handle) -> batch_expr_handle
    linker.func_wrap(
        "env",
        "batch.index",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         receiver_handle: i32,
         index_handle: i32|
         -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let receiver = handle_batch_err!(a, a.take_batch_expr(receiver_handle));
            let index = handle_batch_err!(a, a.take_batch_expr(index_handle));
            a.alloc_batch(BatchNode::Expr(BatchExpr::Index {
                receiver: Box::new(receiver),
                index: Box::new(index),
            }))
        },
    )?;

    // batch.arrayLit(ctx, elems_array_handle) -> batch_expr_handle
    linker.func_wrap(
        "env",
        "batch.arrayLit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, elems_array_handle: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let elems = if elems_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(elems_array_handle));
                handle_batch_err!(a, a.drain_batch_exprs(items))
            };
            a.alloc_batch(BatchNode::Expr(BatchExpr::ArrayLit { elems }))
        },
    )?;

    // batch.objectLit(ctx, fields_array_handle) -> batch_expr_handle
    // fields_array_handle must be an array of batch.objectField handles.
    linker.func_wrap(
        "env",
        "batch.objectLit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, fields_array_handle: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let fields = if fields_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(fields_array_handle));
                handle_batch_err!(a, a.drain_batch_object_fields(items))
            };
            a.alloc_batch(BatchNode::Expr(BatchExpr::ObjectLit { fields }))
        },
    )?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Statement builders (7 bridges)
// ─────────────────────────────────────────────────────────────────────────────

fn register_stmt_builders(linker: &mut Linker<PluginState>) -> Result<()> {
    // batch.stmtCall(ctx, callee_lp, args_array_handle) -> batch_stmt_handle
    linker.func_wrap(
        "env",
        "batch.stmtCall",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         callee_lp: i32,
         args_array_handle: i32|
         -> i32 {
            let callee = match read_lp_string(&mut caller, callee_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let args = if args_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(args_array_handle));
                handle_batch_err!(a, a.drain_batch_exprs(items))
            };
            a.alloc_batch(BatchNode::Stmt(BatchStatement::Call { callee, args }))
        },
    )?;

    // batch.stmtAssign(ctx, target_lp, expr_handle) -> batch_stmt_handle
    linker.func_wrap(
        "env",
        "batch.stmtAssign",
        |mut caller: Caller<'_, PluginState>, ctx: i32, target_lp: i32, expr_handle: i32| -> i32 {
            let target = match read_lp_string(&mut caller, target_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let expr = handle_batch_err!(a, a.take_batch_expr(expr_handle));
            a.alloc_batch(BatchNode::Stmt(BatchStatement::Assign { target, expr }))
        },
    )?;

    // batch.stmtIf(ctx, cond_handle, then_array_handle, else_array_handle) -> batch_stmt_handle
    // Pass else_array_handle = 0 to omit the else branch.
    linker.func_wrap(
        "env",
        "batch.stmtIf",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         cond_handle: i32,
         then_array_handle: i32,
         else_array_handle: i32|
         -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let cond = handle_batch_err!(a, a.take_batch_expr(cond_handle));

            let then = if then_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(then_array_handle));
                handle_batch_err!(a, a.drain_batch_stmts(items))
            };

            let else_ = if else_array_handle == 0 {
                None
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(else_array_handle));
                Some(handle_batch_err!(a, a.drain_batch_stmts(items)))
            };

            a.alloc_batch(BatchNode::Stmt(BatchStatement::If { cond, then, else_ }))
        },
    )?;

    // batch.stmtWhile(ctx, cond_handle, body_array_handle) -> batch_stmt_handle
    linker.func_wrap(
        "env",
        "batch.stmtWhile",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         cond_handle: i32,
         body_array_handle: i32|
         -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let cond = handle_batch_err!(a, a.take_batch_expr(cond_handle));
            let body = if body_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(body_array_handle));
                handle_batch_err!(a, a.drain_batch_stmts(items))
            };
            a.alloc_batch(BatchNode::Stmt(BatchStatement::While { cond, body }))
        },
    )?;

    // batch.stmtFor(ctx, iter_var_lp, iterable_handle, body_array_handle) -> batch_stmt_handle
    linker.func_wrap(
        "env",
        "batch.stmtFor",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         iter_var_lp: i32,
         iterable_handle: i32,
         body_array_handle: i32|
         -> i32 {
            let iter_var = match read_lp_string(&mut caller, iter_var_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let iterable = handle_batch_err!(a, a.take_batch_expr(iterable_handle));
            let body = if body_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(body_array_handle));
                handle_batch_err!(a, a.drain_batch_stmts(items))
            };
            a.alloc_batch(BatchNode::Stmt(BatchStatement::For {
                iter_var,
                iterable,
                body,
            }))
        },
    )?;

    // batch.stmtReturn(ctx, expr_handle) -> batch_stmt_handle
    // Pass expr_handle = 0 for a bare `return`.
    linker.func_wrap(
        "env",
        "batch.stmtReturn",
        |mut caller: Caller<'_, PluginState>, ctx: i32, expr_handle: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let expr = if expr_handle == 0 {
                None
            } else {
                Some(handle_batch_err!(a, a.take_batch_expr(expr_handle)))
            };
            a.alloc_batch(BatchNode::Stmt(BatchStatement::Return { expr }))
        },
    )?;

    // batch.stmtBlock(ctx, stmts_array_handle) -> batch_stmt_handle
    linker.func_wrap(
        "env",
        "batch.stmtBlock",
        |mut caller: Caller<'_, PluginState>, ctx: i32, stmts_array_handle: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let stmts = if stmts_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(stmts_array_handle));
                handle_batch_err!(a, a.drain_batch_stmts(items))
            };
            a.alloc_batch(BatchNode::Stmt(BatchStatement::Block { stmts }))
        },
    )?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Function, class, param, and terminal builders
// ─────────────────────────────────────────────────────────────────────────────

fn register_func_class_builders(linker: &mut Linker<PluginState>) -> Result<()> {
    // batch.param(ctx, name_lp, type_lp) -> param_handle
    linker.func_wrap(
        "env",
        "batch.param",
        |mut caller: Caller<'_, PluginState>, ctx: i32, name_lp: i32, type_lp: i32| -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let ty = match read_lp_string(&mut caller, type_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            // Validate type string (same resolver as the JSON path).
            if let Err(e) = super::batch_schema::resolve_type(&ty) {
                emit_batch_plugin013(a, format!("batch.param: {}", e.message()));
                return 0;
            }
            a.alloc_batch(BatchNode::Param(BatchParam { name, ty }))
        },
    )?;

    // batch.objectField(ctx, name_lp, expr_handle) -> object_field_handle
    linker.func_wrap(
        "env",
        "batch.objectField",
        |mut caller: Caller<'_, PluginState>, ctx: i32, name_lp: i32, expr_handle: i32| -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let expr = handle_batch_err!(a, a.take_batch_expr(expr_handle));
            a.alloc_batch(BatchNode::ObjectField(BatchObjectField {
                key: name,
                value: expr,
            }))
        },
    )?;

    // batch.classField(ctx, name_lp, type_lp) -> class_field_handle
    linker.func_wrap(
        "env",
        "batch.classField",
        |mut caller: Caller<'_, PluginState>, ctx: i32, name_lp: i32, type_lp: i32| -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let ty = match read_lp_string(&mut caller, type_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            if let Err(e) = super::batch_schema::resolve_type(&ty) {
                emit_batch_plugin013(a, format!("batch.classField: {}", e.message()));
                return 0;
            }
            a.alloc_batch(BatchNode::Field(BatchField { name, ty }))
        },
    )?;

    // batch.func(ctx, name_lp, params_array_handle, return_type_lp, body_array_handle) -> func_handle
    linker.func_wrap(
        "env",
        "batch.func",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         name_lp: i32,
         params_array_handle: i32,
         return_type_lp: i32,
         body_array_handle: i32|
         -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let return_type = match read_lp_string(&mut caller, return_type_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            if let Err(e) = super::batch_schema::resolve_type(&return_type) {
                emit_batch_plugin013(a, format!("batch.func: {}", e.message()));
                return 0;
            }
            let params = if params_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(params_array_handle));
                handle_batch_err!(a, a.drain_batch_params(items))
            };
            let body = if body_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(body_array_handle));
                handle_batch_err!(a, a.drain_batch_stmts(items))
            };
            a.alloc_batch(BatchNode::Function(BatchFunction {
                name,
                params,
                return_type,
                body,
            }))
        },
    )?;

    // batch.method(ctx, name_lp, params_array_handle, return_type_lp,
    //              body_handle_or_0, from_source_handle_or_0) -> method_handle
    //
    // Exactly one of body_handle_or_0 / from_source_handle_or_0 must be non-zero.
    // body_handle_or_0 is a batch_stmt_handle (BatchStatement::Block).
    // from_source_handle_or_0 is an AST stmt handle from _emit_stmt_from_source
    //   (Landing B) — stored in the arena as EmitNode::Stmt.
    linker.func_wrap(
        "env",
        "batch.method",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         name_lp: i32,
         params_array_handle: i32,
         return_type_lp: i32,
         body_handle_or_0: i32,
         from_source_handle_or_0: i32|
         -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let return_type = match read_lp_string(&mut caller, return_type_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }

            // Exclusive-or check: exactly one of the two body handles must be non-zero.
            match (body_handle_or_0 != 0, from_source_handle_or_0 != 0) {
                (false, false) => {
                    emit_batch_plugin013(
                        a,
                        "batch.method: both body_handle and from_source_handle are zero; \
                         exactly one must be non-zero (typed-emission.md §3.14)",
                    );
                    return 0;
                }
                (true, true) => {
                    emit_batch_plugin013(
                        a,
                        "batch.method: both body_handle and from_source_handle are non-zero; \
                         exactly one must be non-zero (typed-emission.md §3.14)",
                    );
                    return 0;
                }
                _ => {} // Exactly one is non-zero — proceed.
            }

            if let Err(e) = super::batch_schema::resolve_type(&return_type) {
                emit_batch_plugin013(a, format!("batch.method: {}", e.message()));
                return 0;
            }

            let params = if params_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(params_array_handle));
                handle_batch_err!(a, a.drain_batch_params(items))
            };

            // body_handle path: consume a batch stmt handle, must be a Block.
            let (body, body_handle_opt) = if body_handle_or_0 != 0 {
                // body_handle_or_0 must be a tagged batch handle pointing at a BatchStmt.
                if !super::arena::EmitArena::is_batch_handle(body_handle_or_0) {
                    emit_batch_plugin013(
                        a,
                        format!(
                            "batch.method: body_handle {} is not a batch handle \
                             (did you pass an AST handle? use from_source_handle instead)",
                            body_handle_or_0
                        ),
                    );
                    return 0;
                }
                let batch_stmt = handle_batch_err!(a, a.take_batch_stmt(body_handle_or_0));
                // Wrap into Vec<BatchStatement> — the body field expects a list.
                let stmts = match batch_stmt {
                    BatchStatement::Block { stmts } => stmts,
                    other => vec![other], // non-block stmt: wrap as single-element block
                };
                (Some(stmts), None)
            } else {
                // from_source_handle path: consume an AST stmt handle (EmitNode::Stmt).
                // This is a handle returned by _emit_stmt_from_source (Landing B).
                // It lives in the main (AST) arena, not the batch arena.
                let handle = from_source_handle_or_0;
                if super::arena::EmitArena::is_batch_handle(handle) {
                    emit_batch_plugin013(
                        a,
                        format!(
                            "batch.method: from_source_handle {} is a batch handle, not an AST \
                             stmt handle from _emit_stmt_from_source",
                            handle
                        ),
                    );
                    return 0;
                }
                // from_source_handle lives in the AST arena — it is a raw int
                // handle (no BATCH_TAG). We store it in BatchMethod.body_handle
                // so the downstream bridge (_emit_class_full) can consume it
                // via arena.take_stmt() exactly as the JSON "body_handle" path does.
                (None, Some(handle))
            };

            let method = BatchMethod {
                name: name.clone(),
                params,
                return_type,
                body,
                body_handle: body_handle_opt,
            };

            a.alloc_batch(BatchNode::Method(method))
        },
    )?;

    // batch.class(ctx, name_lp, parent_name_lp_or_0, fields_array_handle,
    //             methods_array_handle) -> batch_spec_handle
    //
    // Terminal builder for _emit_class_full. parent_name_lp_or_0 = 0 → no parent.
    linker.func_wrap(
        "env",
        "batch.class",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         name_lp: i32,
         parent_name_lp_or_0: i32,
         fields_array_handle: i32,
         methods_array_handle: i32|
         -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let parent = if parent_name_lp_or_0 == 0 {
                None
            } else {
                match read_lp_string(&mut caller, parent_name_lp_or_0) {
                    Some(s) if !s.is_empty() => Some(s),
                    _ => None,
                }
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let fields = if fields_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(fields_array_handle));
                handle_batch_err!(a, a.drain_batch_fields(items))
            };
            let methods = if methods_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) = handle_batch_err!(a, a.take_batch_array(methods_array_handle));
                handle_batch_err!(a, a.drain_batch_methods(items))
            };
            a.alloc_batch(BatchNode::ClassSpec(BatchBuilderClass {
                name,
                parent,
                fields,
                methods,
            }))
        },
    )?;

    // batch.spec(ctx, functions_array_handle) -> batch_spec_handle
    //
    // Terminal builder for _emit_helpers_batch. Consumes a function array handle.
    linker.func_wrap(
        "env",
        "batch.spec",
        |mut caller: Caller<'_, PluginState>, ctx: i32, functions_array_handle: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let functions = if functions_array_handle == 0 {
                Vec::new()
            } else {
                let (items, _kind) =
                    handle_batch_err!(a, a.take_batch_array(functions_array_handle));
                handle_batch_err!(a, a.drain_batch_functions(items))
            };
            a.alloc_batch(BatchNode::Spec(BatchSpec { functions }))
        },
    )?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests — pure arena-level (no WASM round-trip)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::arena::{
        BatchArray, BatchArrayKind, BatchArrayPushError, BatchBuilderClass, BatchNode, EmitArena,
        BATCH_TAG,
    };
    use super::super::batch_schema::{
        BatchExpr, BatchField, BatchFunction, BatchMethod, BatchParam, BatchSpec, BatchStatement,
    };

    fn fresh_arena() -> EmitArena {
        EmitArena::new(42)
    }

    // ── 1. Handle allocation: tagged with BATCH_TAG ───────────────────────────

    #[test]
    fn batch_handle_has_tag_bit() {
        let mut a = fresh_arena();
        let h = a.alloc_batch(BatchNode::Expr(BatchExpr::BoolLit { value: true }));
        assert_ne!(h, 0);
        assert!(EmitArena::is_batch_handle(h));
        // Verify tag bit is set.
        assert!((h & BATCH_TAG) != 0);
        // Internal index starts at 1.
        assert_eq!(h & !BATCH_TAG, 1);
    }

    #[test]
    fn batch_handles_are_monotonic() {
        let mut a = fresh_arena();
        let h1 = a.alloc_batch(BatchNode::Expr(BatchExpr::BoolLit { value: false }));
        let h2 = a.alloc_batch(BatchNode::Expr(BatchExpr::IntLit { value: 7 }));
        let idx1 = h1 & !BATCH_TAG;
        let idx2 = h2 & !BATCH_TAG;
        assert_eq!(idx2, idx1 + 1);
    }

    // ── 2. Consumption + PLUGIN008 on re-use ─────────────────────────────────

    #[test]
    fn batch_handle_consumed_on_take() {
        let mut a = fresh_arena();
        let h = a.alloc_batch(BatchNode::Expr(BatchExpr::BoolLit { value: true }));
        let _ = a.take_batch_expr(h).expect("first take should succeed");
        let err = a.take_batch_expr(h).unwrap_err();
        assert_eq!(
            err,
            crate::plugins::typed_emission::error::EmitError::HandleConsumed { handle: h }
        );
    }

    // ── 3. Array mutation-in-place ────────────────────────────────────────────

    #[test]
    fn array_push_sets_kind_on_first_push() {
        let mut a = fresh_arena();
        let arr_h = a.alloc_batch(BatchNode::Array(BatchArray {
            kind: BatchArrayKind::Unset,
            items: Vec::new(),
        }));
        let item_h = a.alloc_batch(BatchNode::Expr(BatchExpr::IntLit { value: 1 }));
        a.batch_array_push(arr_h, item_h)
            .expect("push should succeed");

        // Array kind is now Expr.
        let arr_kind = a.peek_batch_array_kind(arr_h).unwrap();
        assert_eq!(arr_kind, BatchArrayKind::Expr);
    }

    #[test]
    fn array_push_kind_mismatch_returns_error() {
        let mut a = fresh_arena();
        let arr_h = a.alloc_batch(BatchNode::Array(BatchArray {
            kind: BatchArrayKind::Unset,
            items: Vec::new(),
        }));
        let expr_h = a.alloc_batch(BatchNode::Expr(BatchExpr::BoolLit { value: true }));
        a.batch_array_push(arr_h, expr_h).unwrap();

        // Now push a Stmt — kind mismatch.
        let stmt_h = a.alloc_batch(BatchNode::Stmt(BatchStatement::Return { expr: None }));
        let err = a.batch_array_push(arr_h, stmt_h).unwrap_err();
        assert!(matches!(err, BatchArrayPushError::KindMismatch { .. }));
    }

    #[test]
    fn array_consumed_after_drain() {
        let mut a = fresh_arena();
        let arr_h = a.alloc_batch(BatchNode::Array(BatchArray {
            kind: BatchArrayKind::Unset,
            items: Vec::new(),
        }));
        let item_h = a.alloc_batch(BatchNode::Stmt(BatchStatement::Return { expr: None }));
        a.batch_array_push(arr_h, item_h).unwrap();

        let (_items, _kind) = a
            .take_batch_array(arr_h)
            .expect("take_batch_array should succeed");
        // Array is now consumed — second take must fail.
        let err = a.take_batch_array(arr_h).unwrap_err();
        assert!(matches!(
            err,
            crate::plugins::typed_emission::error::EmitError::HandleConsumed { .. }
        ));
    }

    // ── 4. Terminal builders ──────────────────────────────────────────────────

    #[test]
    fn batch_spec_wraps_functions() {
        let mut a = fresh_arena();
        let fn_h = a.alloc_batch(BatchNode::Function(BatchFunction {
            name: "helper".to_string(),
            params: Vec::new(),
            return_type: "void".to_string(),
            body: Vec::new(),
        }));
        let arr_h = a.alloc_batch(BatchNode::Array(BatchArray {
            kind: BatchArrayKind::Unset,
            items: Vec::new(),
        }));
        a.batch_array_push(arr_h, fn_h).unwrap();
        let (items, _kind) = a.take_batch_array(arr_h).unwrap();
        let funcs = a.drain_batch_functions(items).unwrap();
        let spec_h = a.alloc_batch(BatchNode::Spec(BatchSpec { functions: funcs }));
        let spec = a.take_batch_spec(spec_h).unwrap();
        assert_eq!(spec.functions.len(), 1);
        assert_eq!(spec.functions[0].name, "helper");
    }

    #[test]
    fn batch_class_spec_wraps_fields_and_methods() {
        let mut a = fresh_arena();
        let spec_h = a.alloc_batch(BatchNode::ClassSpec(BatchBuilderClass {
            name: "Foo".to_string(),
            parent: None,
            fields: vec![BatchField {
                name: "x".to_string(),
                ty: "integer".to_string(),
            }],
            methods: Vec::new(),
        }));
        let spec = a.take_batch_class_spec(spec_h).unwrap();
        assert_eq!(spec.name, "Foo");
        assert_eq!(spec.fields.len(), 1);
    }

    // ── 5. Handle-path vs JSON-path produces same BatchFunction shape ─────────

    #[test]
    fn builder_func_matches_json_parsed_func() {
        use super::super::batch_schema::{function_to_ast, parse_batch_spec};

        // JSON path.
        let json = r#"{"functions":[{
            "name":"greet","params":[{"name":"name","type":"string"}],
            "return_type":"void",
            "body":[{"kind":"return"}]
        }]}"#;
        let spec = parse_batch_spec(json).unwrap();
        let json_fn = function_to_ast(spec.functions.into_iter().next().unwrap(), false).unwrap();

        // Builder path.
        let mut a = fresh_arena();
        let fn_h = a.alloc_batch(BatchNode::Function(BatchFunction {
            name: "greet".to_string(),
            params: vec![BatchParam {
                name: "name".to_string(),
                ty: "string".to_string(),
            }],
            return_type: "void".to_string(),
            body: vec![BatchStatement::Return { expr: None }],
        }));
        let builder_fn_batch = a.take_batch_function(fn_h).unwrap();
        let builder_fn = function_to_ast(builder_fn_batch, false).unwrap();

        // Both paths produce the same AST shape.
        assert_eq!(json_fn.name, builder_fn.name);
        assert_eq!(json_fn.parameters.len(), builder_fn.parameters.len());
        assert_eq!(json_fn.return_type, builder_fn.return_type);
        assert_eq!(json_fn.body.len(), builder_fn.body.len());
    }

    // ── 6. batch.method body_handle path stores inline body ──────────────────

    #[test]
    fn method_with_inline_body_stores_stmts() {
        let mut a = fresh_arena();
        let method = BatchMethod {
            name: "doThing".to_string(),
            params: Vec::new(),
            return_type: "void".to_string(),
            body: Some(vec![BatchStatement::Return { expr: None }]),
            body_handle: None,
        };
        let h = a.alloc_batch(BatchNode::Method(method));
        let m = a.take_batch_method(h).unwrap();
        assert!(m.body.is_some());
        assert!(m.body_handle.is_none());
        assert_eq!(m.body.unwrap().len(), 1);
    }

    // ── 7. batch.method from_source path stores handle ref ───────────────────

    #[test]
    fn method_with_from_source_handle_stores_handle() {
        let mut a = fresh_arena();
        let method = BatchMethod {
            name: "passThrough".to_string(),
            params: Vec::new(),
            return_type: "void".to_string(),
            body: None,
            body_handle: Some(99), // AST stmt handle (no BATCH_TAG)
        };
        let h = a.alloc_batch(BatchNode::Method(method));
        let m = a.take_batch_method(h).unwrap();
        assert!(m.body.is_none());
        assert_eq!(m.body_handle, Some(99));
    }

    // ── 8. Handle type mismatch returns WrongNodeKind ─────────────────────────

    #[test]
    fn take_batch_expr_on_stmt_returns_wrong_kind() {
        let mut a = fresh_arena();
        let h = a.alloc_batch(BatchNode::Stmt(BatchStatement::Return { expr: None }));
        let err = a.take_batch_expr(h).unwrap_err();
        assert!(matches!(
            err,
            crate::plugins::typed_emission::error::EmitError::WrongNodeKind { .. }
        ));
    }

    // ── 9. Non-batch handle rejected by take_batch_node ──────────────────────

    #[test]
    fn non_batch_handle_rejected_by_take_batch_node() {
        let mut a = fresh_arena();
        // A plain AST handle (no BATCH_TAG) — should be rejected.
        let ast_handle = a.alloc_stmt(crate::ast::Statement::Return {
            value: None,
            location: None,
        });
        let err = a.take_batch_node(ast_handle).unwrap_err();
        assert!(matches!(
            err,
            crate::plugins::typed_emission::error::EmitError::OutOfRange { .. }
        ));
    }

    // ── 10. Item consumed by arrayPush cannot be re-used ─────────────────────

    #[test]
    fn item_consumed_by_array_push_cannot_be_reused() {
        let mut a = fresh_arena();
        let arr_h = a.alloc_batch(BatchNode::Array(BatchArray {
            kind: BatchArrayKind::Unset,
            items: Vec::new(),
        }));
        let item_h = a.alloc_batch(BatchNode::Expr(BatchExpr::BoolLit { value: false }));
        a.batch_array_push(arr_h, item_h).unwrap();
        // item_h is now consumed by the array.
        let err = a.take_batch_expr(item_h).unwrap_err();
        assert!(matches!(
            err,
            crate::plugins::typed_emission::error::EmitError::HandleConsumed { .. }
        ));
    }
}
