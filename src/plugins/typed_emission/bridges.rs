/// Typed-emission bridge registration.
///
/// All ~30 host functions that the plugin WASM imports via the `"env"` module
/// are registered here onto a dedicated `Linker<PluginState>`. This linker is
/// separate from the v1 linker so that v3 plugins get exactly the typed-emission
/// surface and nothing the v1 surface exports that would be misleading.
///
/// Bridge naming mirrors the spec table in typed-emission.md §3.
use anyhow::Result;
use wasmtime::{Caller, Linker};

use super::arena::BATCH_TAG;
use super::batch_schema::{
    self, class_to_ast, function_to_ast, parse_batch_spec, parse_class_spec, stmt_to_ast,
    BatchSchemaError, ClassSpec, BATCH_FUNCTION_LIMIT,
};
use super::error::{EmitDiagnostic, EmitError};
use super::json::{
    decode_class_desc, decode_handle_array, decode_object_fields, decode_param_array,
    decode_state_fields,
};
use crate::ast::{
    AssignmentTarget, Class, Expression, ExternalFunction, Field, Function, Parameter,
    SourceLocation, StateBlock, StateDeclaration, StateScope, Statement, Type, Value, Visibility,
};

// ─── Re-use PluginState from wasm_adapter (pub(crate) visibility) ────────────
use crate::plugins::wasm_adapter::PluginState;

// ─────────────────────────────────────────────────────────────────────────────
// LP-string reading helpers (borrowed from wasm_adapter.rs conventions)
// ─────────────────────────────────────────────────────────────────────────────

/// Read an LP-string (4-byte LE length + bytes) from WASM memory at `ptr`.
/// Returns `None` on any invalid pointer/bounds condition.
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

/// Build a span JSON payload for use in `EmitDiagnostic.span_json` when the
/// diagnostic originates from `_emit_stmt_from_source`. The `"source"` field
/// is `"block_body"` per typed-emission.md §3.11 + §5.
fn build_span_json(start: usize, end: usize) -> String {
    format!(
        "{{\"start\":{},\"end\":{},\"source\":\"block_body\"}}",
        start, end
    )
}

/// Truncate a source string for inclusion in a diagnostic message. Long
/// fragments get an ellipsis appended.
fn truncate_for_diagnostic(source: &str, max_bytes: usize) -> String {
    if source.len() <= max_bytes {
        source.replace('\n', "\\n")
    } else {
        // Find a char boundary at or before `max_bytes`.
        let mut cut = max_bytes;
        while cut > 0 && !source.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", source[..cut].replace('\n', "\\n"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Arena access helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Borrow the arena mutably from caller state. Panics if the arena was not
/// installed before the linker call — this is a contract violation in
/// `call_expand_typed`, not something a plugin can trigger.
macro_rules! arena {
    ($caller:expr) => {
        $caller
            .data_mut()
            .emit_arena
            .as_mut()
            .expect("typed-emission: arena must be installed before bridge calls")
    };
}

/// Handle any EmitError from a take operation: PLUGIN008 emits diagnostic,
/// all others just return 0. The macro evaluates to the error-exit path.
macro_rules! take_or_return {
    ($arena:expr, $result:expr) => {
        match $result {
            Ok(v) => v,
            Err(EmitError::HandleConsumed { handle }) => {
                $arena.emit_plugin008(handle);
                return 0;
            }
            Err(e) => {
                $arena.emit_diagnostic(EmitDiagnostic {
                    severity: 2,
                    code: e.error_code().to_string(),
                    message: format!("typed-emission bridge error: {:?}", e),
                    span_json: String::new(),
                });
                return 0;
            }
        }
    };
}

/// Same as `take_or_return!` but the result is `Option<T>` (nullable handle).
macro_rules! take_opt_or_return {
    ($arena:expr, $result:expr) => {
        match $result {
            Ok(v) => v,
            Err(EmitError::HandleConsumed { handle }) => {
                $arena.emit_plugin008(handle);
                return 0;
            }
            Err(e) => {
                $arena.emit_diagnostic(EmitDiagnostic {
                    severity: 2,
                    code: e.error_code().to_string(),
                    message: format!("typed-emission bridge error: {:?}", e),
                    span_json: String::new(),
                });
                return 0;
            }
        }
    };
}

// ─────────────────────────────────────────────────────────────────────────────
// Public registration entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Register all typed-emission bridge functions onto `linker`.
///
/// The linker returned by `WasmPlugin::get_typed_emission_linker()` calls
/// this function. All ~57 functions are registered in the "env" module to
/// match the plugin WAT import declarations.
///
/// Registration order:
///  1. Type constructors (§3.10)
///  2. Expression constructors (§3.9)
///  3. Statement constructors (§3.8)
///  4. Declaration emitters (§3.1–§3.7, §3.11)
///  5. Batch emitters (§3.12–§3.13, widened for Amendment 4)
///  6. Batch-spec builder bridges (§3.14 Amendment 4)
///  7. Error bridge (§5)
pub fn register_typed_emission_bridges(linker: &mut Linker<PluginState>) -> Result<()> {
    register_type_constructors(linker)?;
    register_expr_constructors(linker)?;
    register_stmt_constructors(linker)?;
    register_declaration_emitters(linker)?;
    register_batch_emitters(linker)?;
    super::batch_builders::register_batch_builder_bridges(linker)?;
    register_error_bridge(linker)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// §4.4  Type constructors
// ─────────────────────────────────────────────────────────────────────────────

fn register_type_constructors(linker: &mut Linker<PluginState>) -> Result<()> {
    // _type_string(ctx) -> handle
    linker.func_wrap(
        "env",
        "_type_string",
        |mut caller: Caller<'_, PluginState>, ctx: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_type(Type::String)
        },
    )?;

    // _type_integer(ctx) -> handle
    linker.func_wrap(
        "env",
        "_type_integer",
        |mut caller: Caller<'_, PluginState>, ctx: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_type(Type::Integer)
        },
    )?;

    // _type_number(ctx) -> handle
    linker.func_wrap(
        "env",
        "_type_number",
        |mut caller: Caller<'_, PluginState>, ctx: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_type(Type::Number)
        },
    )?;

    // _type_boolean(ctx) -> handle
    linker.func_wrap(
        "env",
        "_type_boolean",
        |mut caller: Caller<'_, PluginState>, ctx: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_type(Type::Boolean)
        },
    )?;

    // _type_void(ctx) -> handle
    linker.func_wrap(
        "env",
        "_type_void",
        |mut caller: Caller<'_, PluginState>, ctx: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_type(Type::Void)
        },
    )?;

    // _type_array(ctx, elem_type_handle) -> handle
    linker.func_wrap(
        "env",
        "_type_array",
        |mut caller: Caller<'_, PluginState>, ctx: i32, elem_handle: i32| -> i32 {
            let a = arena!(caller);
            let elem = take_or_return!(a, a.take_type(ctx, elem_handle));
            a.alloc_type(Type::List(
                Box::new(elem),
                crate::ast::ListBehavior::Default,
            ))
        },
    )?;

    // _type_class_ref(ctx, name_lp) -> handle
    linker.func_wrap(
        "env",
        "_type_class_ref",
        |mut caller: Caller<'_, PluginState>, ctx: i32, name_lp: i32| -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_type(Type::Object(name))
        },
    )?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// §4.3  Expression constructors
// ─────────────────────────────────────────────────────────────────────────────

fn register_expr_constructors(linker: &mut Linker<PluginState>) -> Result<()> {
    // _expr_string_lit(ctx, value_lp) -> handle
    linker.func_wrap(
        "env",
        "_expr_string_lit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, value_lp: i32| -> i32 {
            let s = match read_lp_string(&mut caller, value_lp) {
                Some(s) => s,
                None => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_expr(Expression::Literal(Value::String(s)))
        },
    )?;

    // _expr_int_lit(ctx, value_lo_i32, value_hi_i32) -> handle
    // Reassemble i64 from two i32 halves: value = (hi << 32) | (lo as u32)
    linker.func_wrap(
        "env",
        "_expr_int_lit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, lo: i32, hi: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let value = ((hi as i64) << 32) | (lo as u32 as i64);
            a.alloc_expr(Expression::Literal(Value::Integer(value)))
        },
    )?;

    // _expr_number_lit(ctx, bits_lo, bits_hi) -> handle
    // f64 bits reassembled from two i32 halves.
    linker.func_wrap(
        "env",
        "_expr_number_lit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, bits_lo: i32, bits_hi: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let bits = ((bits_hi as u64) << 32) | (bits_lo as u32 as u64);
            let value = f64::from_bits(bits);
            a.alloc_expr(Expression::Literal(Value::Number(value)))
        },
    )?;

    // _expr_bool_lit(ctx, value_i32) -> handle
    linker.func_wrap(
        "env",
        "_expr_bool_lit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, value: i32| -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_expr(Expression::Literal(Value::Boolean(value != 0)))
        },
    )?;

    // _expr_ident(ctx, name_lp) -> handle
    linker.func_wrap(
        "env",
        "_expr_ident",
        |mut caller: Caller<'_, PluginState>, ctx: i32, name_lp: i32| -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            a.alloc_expr(Expression::Variable(name))
        },
    )?;

    // _expr_field(ctx, receiver_handle, name_lp) -> handle
    linker.func_wrap(
        "env",
        "_expr_field",
        |mut caller: Caller<'_, PluginState>, ctx: i32, recv_handle: i32, name_lp: i32| -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            let receiver = take_or_return!(a, a.take_expr(ctx, recv_handle));
            a.alloc_expr(Expression::PropertyAccess {
                object: Box::new(receiver),
                property: name,
                location: SourceLocation::new(0, 0, "plugin"),
            })
        },
    )?;

    // _expr_call(ctx, callee_lp, args_json_lp) -> handle
    linker.func_wrap(
        "env",
        "_expr_call",
        |mut caller: Caller<'_, PluginState>, ctx: i32, callee_lp: i32, args_json_lp: i32| -> i32 {
            let callee = match read_lp_string(&mut caller, callee_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let args_json = match read_lp_string(&mut caller, args_json_lp) {
                Some(s) => s,
                None => return 0,
            };
            let handles = match decode_handle_array(&args_json) {
                Ok(h) => h,
                Err(_) => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let mut args = Vec::with_capacity(handles.len());
            for h in handles {
                let e = take_or_return!(a, a.take_expr(ctx, h));
                args.push(e);
            }
            a.alloc_expr(Expression::Call(callee, args))
        },
    )?;

    // _expr_binop(ctx, op_lp, lhs_handle, rhs_handle) -> handle
    linker.func_wrap(
        "env",
        "_expr_binop",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         op_lp: i32,
         lhs_handle: i32,
         rhs_handle: i32|
         -> i32 {
            let op_str = match read_lp_string(&mut caller, op_lp) {
                Some(s) => s,
                None => return 0,
            };
            let op = match super::arena::map_binary_op(&op_str) {
                Some(o) => o,
                None => {
                    let a = arena!(caller);
                    if a.check_ctx(ctx).is_err() {
                        return 0;
                    }
                    a.emit_diagnostic(EmitDiagnostic {
                        severity: 2,
                        code: "PLUGIN_UNKNOWN_BINOP".to_string(),
                        message: format!("unknown binary operator `{}`", op_str),
                        span_json: String::new(),
                    });
                    return 0;
                }
            };
            let a = arena!(caller);
            let lhs = take_or_return!(a, a.take_expr(ctx, lhs_handle));
            let rhs = take_or_return!(a, a.take_expr(ctx, rhs_handle));
            a.alloc_expr(Expression::Binary(Box::new(lhs), op, Box::new(rhs)))
        },
    )?;

    // _expr_unop(ctx, op_lp, operand_handle) -> handle
    linker.func_wrap(
        "env",
        "_expr_unop",
        |mut caller: Caller<'_, PluginState>, ctx: i32, op_lp: i32, operand_handle: i32| -> i32 {
            let op_str = match read_lp_string(&mut caller, op_lp) {
                Some(s) => s,
                None => return 0,
            };
            let op = match super::arena::map_unary_op(&op_str) {
                Some(o) => o,
                None => {
                    let a = arena!(caller);
                    if a.check_ctx(ctx).is_err() {
                        return 0;
                    }
                    a.emit_diagnostic(EmitDiagnostic {
                        severity: 2,
                        code: "PLUGIN_UNKNOWN_UNOP".to_string(),
                        message: format!("unknown unary operator `{}`", op_str),
                        span_json: String::new(),
                    });
                    return 0;
                }
            };
            let a = arena!(caller);
            let operand = take_or_return!(a, a.take_expr(ctx, operand_handle));
            a.alloc_expr(Expression::Unary(op, Box::new(operand)))
        },
    )?;

    // _expr_binop_op(ctx, op_code, lhs_handle, rhs_handle) -> handle
    //
    // Numeric-code sibling to `_expr_binop` (LP-string variant above).
    // Uses the stable integer op-code table from typed-emission-ops.toml §3.10.
    // Invalid op_code → diagnostic PLUGIN_INVALID_BINOP_CODE + return 0.
    linker.func_wrap(
        "env",
        "_expr_binop_op",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         op_code: i32,
         lhs_handle: i32,
         rhs_handle: i32|
         -> i32 {
            let op = match super::ops::binop_from_code(op_code) {
                Some(o) => o,
                None => {
                    let a = arena!(caller);
                    if a.check_ctx(ctx).is_err() {
                        return 0;
                    }
                    a.emit_diagnostic(EmitDiagnostic {
                        severity: 2,
                        code: "PLUGIN_INVALID_BINOP_CODE".to_string(),
                        message: format!(
                            "invalid binary op code {} (valid range 0..={}; \
                             see foundation/spec/plugins/contracts/typed-emission-ops.toml [binop])",
                            op_code,
                            super::ops::BINOP_MAX_CODE,
                        ),
                        span_json: String::new(),
                    });
                    return 0;
                }
            };
            let a = arena!(caller);
            let lhs = take_or_return!(a, a.take_expr(ctx, lhs_handle));
            let rhs = take_or_return!(a, a.take_expr(ctx, rhs_handle));
            a.alloc_expr(Expression::Binary(Box::new(lhs), op, Box::new(rhs)))
        },
    )?;

    // _expr_unop_op(ctx, op_code, operand_handle) -> handle
    //
    // Numeric-code sibling to `_expr_unop` (LP-string variant above).
    // Uses the stable integer op-code table from typed-emission-ops.toml §3.10.
    // Invalid op_code → diagnostic PLUGIN_INVALID_UNOP_CODE + return 0.
    linker.func_wrap(
        "env",
        "_expr_unop_op",
        |mut caller: Caller<'_, PluginState>, ctx: i32, op_code: i32, operand_handle: i32| -> i32 {
            let op = match super::ops::unop_from_code(op_code) {
                Some(o) => o,
                None => {
                    let a = arena!(caller);
                    if a.check_ctx(ctx).is_err() {
                        return 0;
                    }
                    a.emit_diagnostic(EmitDiagnostic {
                        severity: 2,
                        code: "PLUGIN_INVALID_UNOP_CODE".to_string(),
                        message: format!(
                            "invalid unary op code {} (valid range 0..={}; \
                             see foundation/spec/plugins/contracts/typed-emission-ops.toml [unop])",
                            op_code,
                            super::ops::UNOP_MAX_CODE,
                        ),
                        span_json: String::new(),
                    });
                    return 0;
                }
            };
            let a = arena!(caller);
            let operand = take_or_return!(a, a.take_expr(ctx, operand_handle));
            a.alloc_expr(Expression::Unary(op, Box::new(operand)))
        },
    )?;

    // _expr_index(ctx, receiver_handle, index_handle) -> handle
    linker.func_wrap(
        "env",
        "_expr_index",
        |mut caller: Caller<'_, PluginState>, ctx: i32, recv_handle: i32, idx_handle: i32| -> i32 {
            let a = arena!(caller);
            let receiver = take_or_return!(a, a.take_expr(ctx, recv_handle));
            let index = take_or_return!(a, a.take_expr(ctx, idx_handle));
            a.alloc_expr(Expression::ListAccess(Box::new(receiver), Box::new(index)))
        },
    )?;

    // _expr_array_lit(ctx, elems_json_lp) -> handle
    linker.func_wrap(
        "env",
        "_expr_array_lit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, elems_json_lp: i32| -> i32 {
            let elems_json = match read_lp_string(&mut caller, elems_json_lp) {
                Some(s) => s,
                None => return 0,
            };
            let handles = match decode_handle_array(&elems_json) {
                Ok(h) => h,
                Err(_) => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let mut elems = Vec::with_capacity(handles.len());
            for h in handles {
                let e = take_or_return!(a, a.take_expr(ctx, h));
                elems.push(e);
            }
            // Build a list-literal expression: `[e1, e2, ...]`
            // Uses Expression::Literal(Value::List(...)) only for constant folding;
            // at compile time the values are Expressions, not Values, so we use
            // the nearest structural equivalent available in the AST.
            // The AST does not have a dedicated ArrayLiteral expression variant;
            // the canonical representation is a sequence of list.add calls.
            // For now emit it as a Call to the built-in `list` constructor with
            // all elements as arguments — this is what the parser produces for
            // `[a, b, c]` literal syntax.
            a.alloc_expr(Expression::Call("__array_literal__".to_string(), elems))
        },
    )?;

    // _expr_object_lit(ctx, fields_json_lp) -> handle
    // fields_json = [{"key":"k","value_handle":N}, ...]
    linker.func_wrap(
        "env",
        "_expr_object_lit",
        |mut caller: Caller<'_, PluginState>, ctx: i32, fields_json_lp: i32| -> i32 {
            let fields_json = match read_lp_string(&mut caller, fields_json_lp) {
                Some(s) => s,
                None => return 0,
            };
            let descs = match decode_object_fields(&fields_json) {
                Ok(d) => d,
                Err(_) => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            // Build a Call("__object_literal__", [k1_expr, v1_expr, ...]) pair sequence.
            // The object-literal AST node does not exist as a standalone variant;
            // we model it as alternating key (string literal) + value pairs.
            let mut args = Vec::with_capacity(descs.len() * 2);
            for desc in descs {
                args.push(Expression::Literal(Value::String(desc.key)));
                let v = take_or_return!(a, a.take_expr(ctx, desc.value_handle));
                args.push(v);
            }
            a.alloc_expr(Expression::Call("__object_literal__".to_string(), args))
        },
    )?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// §4.2  Statement constructors
// ─────────────────────────────────────────────────────────────────────────────

fn register_stmt_constructors(linker: &mut Linker<PluginState>) -> Result<()> {
    // _stmt_call(ctx, callee_lp, args_json_lp) -> handle
    linker.func_wrap(
        "env",
        "_stmt_call",
        |mut caller: Caller<'_, PluginState>, ctx: i32, callee_lp: i32, args_json_lp: i32| -> i32 {
            let callee = match read_lp_string(&mut caller, callee_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let args_json = match read_lp_string(&mut caller, args_json_lp) {
                Some(s) => s,
                None => return 0,
            };
            let handles = match decode_handle_array(&args_json) {
                Ok(h) => h,
                Err(_) => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let mut args = Vec::with_capacity(handles.len());
            for h in handles {
                let e = take_or_return!(a, a.take_expr(ctx, h));
                args.push(e);
            }
            let stmt = Statement::Expression {
                expr: Expression::Call(callee, args),
                location: None,
            };
            a.alloc_stmt(stmt)
        },
    )?;

    // _stmt_assign(ctx, target_lp, expr_handle) -> handle
    linker.func_wrap(
        "env",
        "_stmt_assign",
        |mut caller: Caller<'_, PluginState>, ctx: i32, target_lp: i32, expr_handle: i32| -> i32 {
            let target_name = match read_lp_string(&mut caller, target_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            let value = take_or_return!(a, a.take_expr(ctx, expr_handle));
            let stmt = Statement::Assignment {
                target: AssignmentTarget::Variable(target_name),
                value,
                location: None,
            };
            a.alloc_stmt(stmt)
        },
    )?;

    // _stmt_if(ctx, cond_handle, then_block_handle, else_block_handle) -> handle
    // else_block_handle == 0 means no else branch.
    linker.func_wrap(
        "env",
        "_stmt_if",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         cond_handle: i32,
         then_handle: i32,
         else_handle: i32|
         -> i32 {
            let a = arena!(caller);
            let cond = take_or_return!(a, a.take_expr(ctx, cond_handle));
            let then_stmt = take_or_return!(a, a.take_stmt(ctx, then_handle));
            let else_stmt = take_opt_or_return!(a, a.take_stmt_opt(ctx, else_handle));
            // Flatten block into vec<Statement> for the AST If node.
            let then_branch = flatten_block(then_stmt);
            let else_branch = else_stmt.map(flatten_block);
            let stmt = Statement::If {
                condition: cond,
                then_branch,
                else_branch,
                location: None,
            };
            a.alloc_stmt(stmt)
        },
    )?;

    // _stmt_while(ctx, cond_handle, body_block_handle) -> handle
    linker.func_wrap(
        "env",
        "_stmt_while",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         cond_handle: i32,
         body_handle: i32|
         -> i32 {
            let a = arena!(caller);
            let cond = take_or_return!(a, a.take_expr(ctx, cond_handle));
            let body_stmt = take_or_return!(a, a.take_stmt(ctx, body_handle));
            let body = flatten_block(body_stmt);
            let stmt = Statement::While {
                condition: cond,
                body,
                location: None,
            };
            a.alloc_stmt(stmt)
        },
    )?;

    // _stmt_for(ctx, iter_var_lp, iterable_expr_handle, body_block_handle) -> handle
    linker.func_wrap(
        "env",
        "_stmt_for",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         iter_lp: i32,
         iter_expr_handle: i32,
         body_handle: i32|
         -> i32 {
            let iter_var = match read_lp_string(&mut caller, iter_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let a = arena!(caller);
            let iterable = take_or_return!(a, a.take_expr(ctx, iter_expr_handle));
            let body_stmt = take_or_return!(a, a.take_stmt(ctx, body_handle));
            let body = flatten_block(body_stmt);
            let stmt = Statement::Iterate {
                iterator: iter_var,
                collection: iterable,
                body,
                location: None,
            };
            a.alloc_stmt(stmt)
        },
    )?;

    // _stmt_return(ctx, expr_handle) -> handle  (expr_handle=0 → return void)
    linker.func_wrap(
        "env",
        "_stmt_return",
        |mut caller: Caller<'_, PluginState>, ctx: i32, expr_handle: i32| -> i32 {
            let a = arena!(caller);
            let value = take_opt_or_return!(a, a.take_expr_opt(ctx, expr_handle));
            let stmt = Statement::Return {
                value,
                location: None,
            };
            a.alloc_stmt(stmt)
        },
    )?;

    // _emit_stmt_from_source(ctx, source_lp, origin_offset) -> stmt_handle
    //
    // v3 bridge added by typed-emission.md Amendment 2 (2026-07-01) resolving
    // §12 OQ3 in favor of the pass-through path. See §3.11.
    //
    // Parses a user-authored Clean-source fragment (extracted verbatim from the
    // DSL body the plugin received in `body_lp`) into a Statement AST node.
    // The `origin_offset` parameter is the byte offset of the fragment within
    // the plugin's `body_lp`, used to translate parser diagnostic spans back
    // into the DSL body's coordinate space (OQ7 option a).
    //
    // Failure paths per §3.11:
    //   - syntax error → 0 + diagnostic via _emit_error path with translated span
    //   - valid expression but not statement → 0 + PLUGIN012 diagnostic
    linker.func_wrap(
        "env",
        "_emit_stmt_from_source",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         source_lp: i32,
         origin_offset: i32|
         -> i32 {
            let source = match read_lp_string(&mut caller, source_lp) {
                Some(s) => s,
                None => return 0,
            };
            let origin = origin_offset.max(0) as usize;

            use crate::parser::grammar::{CleanParser, SingleStatementParse};
            let result = CleanParser::parse_single_statement(&source);

            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }

            match result {
                SingleStatementParse::Statement(stmt) => a.alloc_stmt(stmt),
                SingleStatementParse::ExpressionNotStatement => {
                    a.emit_diagnostic(EmitDiagnostic {
                        severity: 2,
                        code: "PLUGIN012".to_string(),
                        message: format!(
                            "ExpressionInStatementContext: the source fragment passed to \
                             _emit_stmt_from_source parses as an expression but is not a \
                             valid statement (typed-emission.md §3.11). Fragment: `{}`",
                            truncate_for_diagnostic(&source, 80)
                        ),
                        span_json: build_span_json(origin, origin + source.len()),
                    });
                    0
                }
                SingleStatementParse::ParseError {
                    message,
                    byte_offset,
                } => {
                    let start = origin + byte_offset;
                    // Clamp the end to the fragment boundary.
                    let end = origin + source.len();
                    a.emit_diagnostic(EmitDiagnostic {
                        severity: 2,
                        code: "SYN001".to_string(),
                        message: format!(
                            "syntax error in user-authored fragment passed to \
                             _emit_stmt_from_source: {} (fragment: `{}`)",
                            message,
                            truncate_for_diagnostic(&source, 80)
                        ),
                        span_json: build_span_json(start, end),
                    });
                    0
                }
            }
        },
    )?;

    // _stmt_block(ctx, stmts_json_lp) -> handle
    // stmts_json = [stmt_handle, ...] in order; each consumed.
    linker.func_wrap(
        "env",
        "_stmt_block",
        |mut caller: Caller<'_, PluginState>, ctx: i32, stmts_json_lp: i32| -> i32 {
            let stmts_json = match read_lp_string(&mut caller, stmts_json_lp) {
                Some(s) => s,
                None => return 0,
            };
            let handles = match decode_handle_array(&stmts_json) {
                Ok(h) => h,
                Err(_) => return 0,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }
            let mut stmts = Vec::with_capacity(handles.len());
            for h in handles {
                let s = take_or_return!(a, a.take_stmt(ctx, h));
                stmts.push(s);
            }
            // A "block" statement is represented as a While(true, body) would be
            // misleading; use the If statement with always-true condition would
            // also be misleading. The AST does not have a bare Block variant.
            // We use Expression { expr: Call("__block__", []) } as the outer
            // container and embed statements as nested structure through the
            // FunctionsBlock variant, which is the closest thing we have.
            //
            // Actually, looking at the AST, the cleanest representation for a
            // bare block of statements is to wrap them as the body of a synthetic
            // function stored as a PendingFunction in the arena — but that would
            // be confusing for callers that pass the handle to _stmt_if then_branch.
            //
            // The correct approach: store the block as a "block statement" by
            // using a synthetic Function whose body is the statement list, wrapped
            // as PendingFunction. When _stmt_if, _stmt_while, _stmt_for consume
            // a block handle, they call `flatten_block` which unwraps the synthetic
            // function body.
            //
            // This is done by allocating a PendingFunction with name="__block__",
            // void return, no params, and the statements as body.
            // The `flatten_block` helper then extracts just the body.
            let synthetic =
                Function::new("__block__".to_string(), Vec::new(), Type::Void, stmts, None);
            // We need to store this as a Stmt though, since _stmt_block returns
            // a stmt handle for consumption by other stmt constructors.
            // Solution: wrap in Statement::FunctionsBlock({synthetic}) — except
            // FunctionsBlock is a Vec<Function> and then we'd need to unwrap later.
            //
            // Simplest correct solution: use a dedicated "block" representation
            // via Statement::If with a literal `true` condition and no else branch.
            // This is semantically equivalent (always-executed sequence) and
            // `flatten_block` will detect and unwrap it.
            let block_stmt = Statement::If {
                condition: Expression::Literal(Value::Boolean(true)),
                then_branch: synthetic.body,
                else_branch: None,
                location: None,
            };
            a.alloc_stmt(block_stmt)
        },
    )?;

    Ok(())
}

/// Flatten a "block statement" into its constituent statements.
///
/// `_stmt_block` encodes a bare sequence of statements as:
///   `Statement::If { condition: true, then_branch: [stmts...], else_branch: None }`
///
/// When another constructor (e.g. `_stmt_if`, `_stmt_while`) consumes a block
/// handle, it calls this to extract the `Vec<Statement>`.
///
/// A non-block statement is wrapped in a single-element vec (natural behaviour).
fn flatten_block(stmt: Statement) -> Vec<Statement> {
    match stmt {
        Statement::If {
            condition: Expression::Literal(Value::Boolean(true)),
            then_branch,
            else_branch: None,
            ..
        } => then_branch,
        other => vec![other],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// §4.1  Declaration emitters
// ─────────────────────────────────────────────────────────────────────────────

fn register_declaration_emitters(linker: &mut Linker<PluginState>) -> Result<()> {
    // _emit_function(ctx, name_lp, params_json_lp, return_type_handle, body_stmt_handle, flags)
    // flags: bit 0 = exported, bit 1 = BFS root, bit 2 = inline
    // return_type_handle = 0 → void
    // body_stmt_handle must be a block or single stmt
    linker.func_wrap(
        "env",
        "_emit_function",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         name_lp: i32,
         params_json_lp: i32,
         return_type_handle: i32,
         body_handle: i32,
         flags: i32|
         -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 1,
            };
            let params_json = match read_lp_string(&mut caller, params_json_lp) {
                Some(s) => s,
                None => return 1,
            };
            let param_descs = match decode_param_array(&params_json) {
                Ok(d) => d,
                Err(_) => return 1,
            };

            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 1;
            }

            // Decode return type (0 → Void)
            let return_type = if return_type_handle == 0 {
                Type::Void
            } else {
                match a.take_type(ctx, return_type_handle) {
                    Ok(t) => t,
                    Err(EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 1;
                    }
                    Err(_) => return 1,
                }
            };

            // Decode parameters
            let mut params = Vec::with_capacity(param_descs.len());
            for desc in param_descs {
                let param_type = match a.take_type(ctx, desc.type_handle) {
                    Ok(t) => t,
                    Err(EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 1;
                    }
                    Err(_) => return 1,
                };
                let default_value = if desc.default_expr_handle != 0 {
                    match a.take_expr(ctx, desc.default_expr_handle) {
                        Ok(e) => Some(e),
                        Err(EmitError::HandleConsumed { handle }) => {
                            a.emit_plugin008(handle);
                            return 1;
                        }
                        Err(_) => return 1,
                    }
                } else {
                    None
                };
                let p = if let Some(dv) = default_value {
                    Parameter::new_with_default(desc.name, param_type, dv)
                } else {
                    Parameter::new(desc.name, param_type)
                };
                params.push(p);
            }

            // Decode body
            let body_stmt = match a.take_stmt(ctx, body_handle) {
                Ok(s) => s,
                Err(EmitError::HandleConsumed { handle }) => {
                    a.emit_plugin008(handle);
                    return 1;
                }
                Err(_) => return 1,
            };
            let body = flatten_block(body_stmt);

            // Build function
            let mut func = Function::new(name, params, return_type, body, None);
            if flags & 1 != 0 {
                func.visibility = Visibility::Public;
            }
            // bit 1 = async/background root — not directly mapped; ignored for now.
            // bit 2 = inline — FunctionModifier::Inline if we ever add it.

            a.expansion.functions.push(func);
            0 // success
        },
    )?;

    // _define_function(ctx, name_lp, params_json_lp, return_type_handle, body_stmt_handle, flags)
    //   -> function_handle (non-zero on success, 0 on failure)
    //
    // Sub-cycle 3 addition (typed-emission.md §3 / sub-cycle 2 design risk row
    // on PendingFunction). Same parameter shape as `_emit_function`, but:
    //   - the resulting Function is stored in the arena as a PendingFunction
    //     and a handle is returned to the caller (instead of being pushed to
    //     `expansion.functions`),
    //   - the handle MUST be consumed by `_emit_class` (as a method),
    //     `_emit_route` (as the handler), or any future bridge that takes a
    //     `function_handle`. An un-consumed handle is silently dropped at
    //     arena.finish() time and the function is not emitted.
    //
    // Rationale: under sub-cycle 2 a method body could only be supplied by
    // calling `_emit_function` first (which pushed it as a top-level function)
    // and then `_emit_class` consuming it back out. That worked but required
    // every method to transit through `expansion.functions`. `_define_function`
    // provides a direct allocation path: the plugin builds the method's body
    // without polluting the file scope.
    linker.func_wrap(
        "env",
        "_define_function",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         name_lp: i32,
         params_json_lp: i32,
         return_type_handle: i32,
         body_handle: i32,
         flags: i32|
         -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 0,
            };
            let params_json = match read_lp_string(&mut caller, params_json_lp) {
                Some(s) => s,
                None => return 0,
            };
            let param_descs = match decode_param_array(&params_json) {
                Ok(d) => d,
                Err(_) => return 0,
            };

            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 0;
            }

            // Return type (0 → Void).
            let return_type = if return_type_handle == 0 {
                Type::Void
            } else {
                match a.take_type(ctx, return_type_handle) {
                    Ok(t) => t,
                    Err(EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 0;
                    }
                    Err(_) => return 0,
                }
            };

            // Parameters.
            let mut params = Vec::with_capacity(param_descs.len());
            for desc in param_descs {
                let param_type = match a.take_type(ctx, desc.type_handle) {
                    Ok(t) => t,
                    Err(EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 0;
                    }
                    Err(_) => return 0,
                };
                let default_value = if desc.default_expr_handle != 0 {
                    match a.take_expr(ctx, desc.default_expr_handle) {
                        Ok(e) => Some(e),
                        Err(EmitError::HandleConsumed { handle }) => {
                            a.emit_plugin008(handle);
                            return 0;
                        }
                        Err(_) => return 0,
                    }
                } else {
                    None
                };
                let p = if let Some(dv) = default_value {
                    Parameter::new_with_default(desc.name, param_type, dv)
                } else {
                    Parameter::new(desc.name, param_type)
                };
                params.push(p);
            }

            // Body.
            let body_stmt = match a.take_stmt(ctx, body_handle) {
                Ok(s) => s,
                Err(EmitError::HandleConsumed { handle }) => {
                    a.emit_plugin008(handle);
                    return 0;
                }
                Err(_) => return 0,
            };
            let body = flatten_block(body_stmt);

            let mut func = Function::new(name, params, return_type, body, None);
            if flags & 1 != 0 {
                func.visibility = Visibility::Public;
            }

            // Allocate as PendingFunction and return its handle.
            a.alloc_pending_function(func)
        },
    )?;

    // _emit_class(ctx, name_lp, class_json_lp, flags)
    linker.func_wrap(
        "env",
        "_emit_class",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         name_lp: i32,
         class_json_lp: i32,
         _flags: i32|
         -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 1,
            };
            let class_json = match read_lp_string(&mut caller, class_json_lp) {
                Some(s) => s,
                None => return 1,
            };
            let desc = match decode_class_desc(&class_json) {
                Ok(d) => d,
                Err(_) => return 1,
            };

            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 1;
            }

            let mut class = Class::new(name, None);
            class.base_class = desc.parent;

            for fd in desc.fields {
                let field_type = match a.take_type(ctx, fd.type_handle) {
                    Ok(t) => t,
                    Err(EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 1;
                    }
                    Err(_) => return 1,
                };
                let field = if fd.default_expr_handle != 0 {
                    let dv = match a.take_expr(ctx, fd.default_expr_handle) {
                        Ok(e) => e,
                        Err(EmitError::HandleConsumed { handle }) => {
                            a.emit_plugin008(handle);
                            return 1;
                        }
                        Err(_) => return 1,
                    };
                    Field::new_with_default(fd.name, field_type, dv)
                } else {
                    Field::new(fd.name, field_type)
                };
                class.fields.push(field);
            }

            for md in desc.methods {
                let func = match a.take_pending_function(ctx, md.function_handle) {
                    Ok(f) => f,
                    Err(EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 1;
                    }
                    Err(_) => return 1,
                };
                class.methods.push(func);
            }

            a.expansion.classes.push(class);
            0
        },
    )?;

    // _emit_external(ctx, name_lp, params_json_lp, return_type_handle, host_class_lp)
    //
    // Per typed-emission.md §3.3: when host_class_lp is non-empty, the compiler
    // checks it against the active build target's host class. A mismatch is
    // reported via PLUGIN007 (BridgeHostClassMismatchInEmission) and the
    // emission is refused.
    linker.func_wrap(
        "env",
        "_emit_external",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         name_lp: i32,
         params_json_lp: i32,
         return_type_handle: i32,
         host_class_lp: i32|
         -> i32 {
            let name = match read_lp_string(&mut caller, name_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 1,
            };
            let params_json = match read_lp_string(&mut caller, params_json_lp) {
                Some(s) => s,
                None => return 1,
            };
            // host_class_lp may legitimately be 0 / empty (plugin did not
            // restrict to a host class); only enforce when non-empty.
            let host_class_decl = read_lp_string(&mut caller, host_class_lp);
            let param_descs = match decode_param_array(&params_json) {
                Ok(d) => d,
                Err(_) => return 1,
            };

            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 1;
            }

            // PLUGIN007 — host class mismatch check.
            if let Some(declared) = host_class_decl.as_deref() {
                if !declared.is_empty() {
                    let active =
                        crate::target_host_class_override().unwrap_or_else(|| "server".to_string());
                    if declared != active {
                        a.emit_diagnostic(EmitDiagnostic {
                            severity: 2,
                            code: "PLUGIN007".to_string(),
                            message: format!(
                                "BridgeHostClassMismatchInEmission: _emit_external declared \
                                 host_class=`{}` but the active build target host class is \
                                 `{}` (typed-emission.md §3.3, bridge-host-classes.md §2). \
                                 external function `{}` was refused.",
                                declared, active, name
                            ),
                            span_json: String::new(),
                        });
                        return 1;
                    }
                }
            }

            let return_type = if return_type_handle == 0 {
                Type::Void
            } else {
                match a.take_type(ctx, return_type_handle) {
                    Ok(t) => t,
                    Err(EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 1;
                    }
                    Err(_) => return 1,
                }
            };

            let mut params = Vec::with_capacity(param_descs.len());
            for desc in param_descs {
                let param_type = match a.take_type(ctx, desc.type_handle) {
                    Ok(t) => t,
                    Err(EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 1;
                    }
                    Err(_) => return 1,
                };
                params.push(Parameter::new(desc.name, param_type));
            }

            let ext = ExternalFunction::new(name, params, return_type, None);
            a.expansion.externals.push(ext);
            0
        },
    )?;

    // _emit_state_block(ctx, fields_json_lp)
    linker.func_wrap(
        "env",
        "_emit_state_block",
        |mut caller: Caller<'_, PluginState>, ctx: i32, fields_json_lp: i32| -> i32 {
            let fields_json = match read_lp_string(&mut caller, fields_json_lp) {
                Some(s) => s,
                None => return 1,
            };
            let field_descs = match decode_state_fields(&fields_json) {
                Ok(d) => d,
                Err(_) => return 1,
            };

            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 1;
            }

            if a.expansion.state.is_some() {
                a.emit_diagnostic(EmitDiagnostic {
                    severity: 2,
                    code: "PLUGIN_DUPLICATE_STATE".to_string(),
                    message: "only one _emit_state_block call is allowed per expansion".to_string(),
                    span_json: String::new(),
                });
                return 1;
            }

            let mut state = StateBlock::new(StateScope::App, None);
            for desc in field_descs {
                let field_type = match a.take_type(ctx, desc.type_handle) {
                    Ok(t) => t,
                    Err(EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 1;
                    }
                    Err(_) => return 1,
                };
                // default_expr_handle = 0 → use a zero-value expression
                let initializer = if desc.default_expr_handle != 0 {
                    match a.take_expr(ctx, desc.default_expr_handle) {
                        Ok(e) => e,
                        Err(EmitError::HandleConsumed { handle }) => {
                            a.emit_plugin008(handle);
                            return 1;
                        }
                        Err(_) => return 1,
                    }
                } else {
                    Expression::Literal(Value::None)
                };
                let decl = StateDeclaration::new(desc.name, field_type, initializer, None);
                state.declarations.push(decl);
            }
            a.expansion.state = Some(state);
            0
        },
    )?;

    // _emit_statement_into_start(ctx, stmt_handle)
    linker.func_wrap(
        "env",
        "_emit_statement_into_start",
        |mut caller: Caller<'_, PluginState>, ctx: i32, stmt_handle: i32| -> i32 {
            let a = arena!(caller);
            let stmt = match a.take_stmt(ctx, stmt_handle) {
                Ok(s) => s,
                Err(EmitError::HandleConsumed { handle }) => {
                    a.emit_plugin008(handle);
                    return 1;
                }
                Err(_) => return 1,
            };
            a.push_start_stmt(stmt);
            0
        },
    )?;

    // _emit_route(ctx, method_lp, path_lp, handler_function_handle, attrs_json_lp)
    // Sugar: consume handler, mark it public (exported), emit route stmt to start.
    linker.func_wrap(
        "env",
        "_emit_route",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         method_lp: i32,
         path_lp: i32,
         handler_handle: i32,
         _attrs_json_lp: i32|
         -> i32 {
            let method = match read_lp_string(&mut caller, method_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 1,
            };
            let path = match read_lp_string(&mut caller, path_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 1,
            };

            let a = arena!(caller);
            let mut handler = match a.take_pending_function(ctx, handler_handle) {
                Ok(f) => f,
                Err(EmitError::HandleConsumed { handle }) => {
                    a.emit_plugin008(handle);
                    return 1;
                }
                Err(_) => return 1,
            };

            // Mark handler as exported.
            handler.visibility = Visibility::Public;
            let handler_name = handler.name.clone();
            a.expansion.functions.push(handler);

            // Emit `_http_route(method, path, handler_name)` into start.
            let route_stmt = Statement::Expression {
                expr: Expression::Call(
                    "_http_route".to_string(),
                    vec![
                        Expression::Literal(Value::String(method)),
                        Expression::Literal(Value::String(path)),
                        Expression::Variable(handler_name),
                    ],
                ),
                location: None,
            };
            a.push_start_stmt(route_stmt);
            0
        },
    )?;

    // _emit_artifact_directive(ctx, artifact_json_lp)
    // Appends to expansion.artifacts (Vec<serde_json::Value> raw payloads).
    // We store as Statement::Expression with a call to "__artifact__" for now;
    // when PluginExpansion gains an `artifacts` field this becomes a direct push.
    linker.func_wrap(
        "env",
        "_emit_artifact_directive",
        |mut caller: Caller<'_, PluginState>, ctx: i32, artifact_json_lp: i32| -> i32 {
            let artifact_json = match read_lp_string(&mut caller, artifact_json_lp) {
                Some(s) if !s.is_empty() => s,
                _ => return 1,
            };
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 1;
            }
            // Store raw JSON as a statement that the expander can inspect.
            let stmt = Statement::Expression {
                expr: Expression::Call(
                    "__artifact_directive__".to_string(),
                    vec![Expression::Literal(Value::String(artifact_json))],
                ),
                location: None,
            };
            a.expansion.statements.push(stmt);
            0
        },
    )?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// §4.4  Batch emitters (Amendment 3 §3.12 / §3.13 — Landing C2)
// ─────────────────────────────────────────────────────────────────────────────

/// Build a span JSON payload with `source="batch_spec"` (Amendment 3 §5).
/// `end` may equal `start` when only a position, not a range, is known.
fn build_batch_span_json(start: usize, end: usize) -> String {
    format!(
        "{{\"start\":{},\"end\":{},\"source\":\"batch_spec\"}}",
        start, end
    )
}

/// Emit a PLUGIN013 diagnostic for a `BatchSchemaError`.
fn emit_plugin013(arena: &mut super::arena::EmitArena, err: &BatchSchemaError, spec_len: usize) {
    let (start, end) = match err.byte_offset() {
        Some(off) => (off, off),
        None => (0, spec_len),
    };
    arena.emit_diagnostic(EmitDiagnostic {
        severity: 2,
        code: "PLUGIN013".to_string(),
        message: format!("InvalidBatchSpec: {}", err.message()),
        span_json: build_batch_span_json(start, end),
    });
}

fn register_batch_emitters(linker: &mut Linker<PluginState>) -> Result<()> {
    // _emit_helpers_batch(ctx, spec_handle_or_lp, flags) -> err
    //   flags: bit 0 = BFS root per function, bit 1 = private
    //
    // Amendment 3 §3.12, widened by Amendment 4 §3.14 to accept either:
    //   - an LP-string pointer to JSON (Amendment 3 path — unchanged)
    //   - a batch_spec_handle built via batch.spec() (Amendment 4 path)
    //
    // Disambiguation: if `(spec_handle_or_lp & BATCH_TAG) != 0`, it is a batch
    // handle. Otherwise it is an LP pointer. See arena.rs doc for rationale.
    linker.func_wrap(
        "env",
        "_emit_helpers_batch",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         spec_handle_or_lp: i32,
         flags: i32|
         -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 1;
            }

            // ── Resolve the spec from either path ────────────────────────────
            let batch = if (spec_handle_or_lp & BATCH_TAG) != 0 {
                // Amendment 4 handle path.
                let spec = match a.take_batch_spec(spec_handle_or_lp) {
                    Ok(s) => s,
                    Err(super::error::EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 1;
                    }
                    Err(e) => {
                        a.emit_diagnostic(super::error::EmitDiagnostic {
                            severity: 2,
                            code: "PLUGIN013".to_string(),
                            message: format!(
                                "InvalidBatchSpec: _emit_helpers_batch handle error: {:?}",
                                e
                            ),
                            span_json: r#"{"start":0,"end":0,"source":"plugin"}"#.to_string(),
                        });
                        return 1;
                    }
                };
                spec
            } else {
                // Amendment 3 LP-string JSON path. The borrow of `a` ends at the
                // closing brace of the `if` arm above; Rust releases it here so
                // we can borrow `caller` to read WASM memory.
                let json = match read_lp_string(&mut caller, spec_handle_or_lp) {
                    Some(s) => s,
                    None => return 1,
                };
                let spec_len = json.len();
                let a = arena!(caller);
                match parse_batch_spec(&json) {
                    Ok(b) => b,
                    Err(e) => {
                        emit_plugin013(a, &e, spec_len);
                        return 1;
                    }
                }
            };

            // ── Batch limit (§3.12, both paths) ──────────────────────────────
            let a = arena!(caller);
            if batch.functions.len() > BATCH_FUNCTION_LIMIT {
                let e = BatchSchemaError::BatchLimit {
                    count: batch.functions.len(),
                    limit: BATCH_FUNCTION_LIMIT,
                };
                // spec_len is unknown for the handle path; use 0.
                emit_plugin013(a, &e, 0);
                return 1;
            }

            // ── Flag bits per Amendment 3 §3.12 ──────────────────────────────
            let private = (flags & 2) != 0;
            let exported = !private;

            // ── Convert each function ─────────────────────────────────────────
            for (idx, f) in batch.functions.into_iter().enumerate() {
                let func = match function_to_ast(f, exported) {
                    Ok(f) => f,
                    Err(e) => {
                        let wrapped = BatchSchemaError::Json {
                            message: format!("function[{}]: {}", idx, e.message()),
                            byte_offset: None,
                        };
                        emit_plugin013(a, &wrapped, 0);
                        return 1;
                    }
                };
                a.expansion.functions.push(func);
            }

            0 // success
        },
    )?;

    // _emit_class_full(ctx, class_handle_or_lp, flags) -> err
    //   flags: bit 0 = exported (Visibility::Public for methods)
    //
    // Amendment 3 §3.13, widened by Amendment 4 §3.14 to accept either:
    //   - an LP-string pointer to JSON (Amendment 3 path — unchanged)
    //   - a batch_spec_handle built via batch.class() (Amendment 4 path)
    //
    // Disambiguation: same BATCH_TAG bit-check as _emit_helpers_batch.
    linker.func_wrap(
        "env",
        "_emit_class_full",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         class_handle_or_lp: i32,
         flags: i32|
         -> i32 {
            let a = arena!(caller);
            if a.check_ctx(ctx).is_err() {
                return 1;
            }

            // ── Resolve class spec from either path ───────────────────────────
            let class_spec: ClassSpec = if (class_handle_or_lp & BATCH_TAG) != 0 {
                // Amendment 4 handle path.
                let builder_class = match a.take_batch_class_spec(class_handle_or_lp) {
                    Ok(c) => c,
                    Err(super::error::EmitError::HandleConsumed { handle }) => {
                        a.emit_plugin008(handle);
                        return 1;
                    }
                    Err(e) => {
                        a.emit_diagnostic(super::error::EmitDiagnostic {
                            severity: 2,
                            code: "PLUGIN013".to_string(),
                            message: format!(
                                "InvalidBatchSpec: _emit_class_full handle error: {:?}",
                                e
                            ),
                            span_json: r#"{"start":0,"end":0,"source":"plugin"}"#.to_string(),
                        });
                        return 1;
                    }
                };
                // Convert BatchBuilderClass to ClassSpec (same field layout).
                ClassSpec {
                    name: builder_class.name,
                    parent: builder_class.parent,
                    fields: builder_class.fields,
                    methods: builder_class.methods,
                }
            } else {
                // Amendment 3 LP-string JSON path. Borrow of `a` ends above.
                let json = match read_lp_string(&mut caller, class_handle_or_lp) {
                    Some(s) => s,
                    None => return 1,
                };
                let spec_len = json.len();
                let a = arena!(caller);
                match parse_class_spec(&json) {
                    Ok(c) => c,
                    Err(e) => {
                        emit_plugin013(a, &e, spec_len);
                        return 1;
                    }
                }
            };

            let a = arena!(caller);
            // spec_len is 0 for the handle path (no JSON to point into);
            // emit_plugin013 uses this for the span end byte in batch_spec spans.
            // For the JSON path, spec_len was set inside the else-branch above and
            // is no longer in scope — use 0 uniformly here since the class-spec
            // body-resolution errors below are not JSON-position errors.
            let spec_len: usize = 0;

            // 2. Resolve method bodies (inline OR body_handle) BEFORE the class
            //    conversion. We take the methods vec out by ownership so the
            //    BatchStatement values move into stmt_to_ast without needing
            //    Clone. The class-header data (name/parent/fields) stays.
            let mut class_spec = class_spec;
            let methods_taken: Vec<super::batch_schema::BatchMethod> =
                std::mem::take(&mut class_spec.methods);

            let mut method_bodies: Vec<Vec<crate::ast::Statement>> =
                Vec::with_capacity(methods_taken.len());
            // Keep method headers (name/params/return_type) parallel to bodies.
            let mut method_headers: Vec<super::batch_schema::BatchMethod> =
                Vec::with_capacity(methods_taken.len());

            for (mi, mut m) in methods_taken.into_iter().enumerate() {
                let body_source = m.body.take();
                let body_handle = m.body_handle;
                let name = m.name.clone();
                let body = match (body_source, body_handle) {
                    (Some(stmts), None) => {
                        let mut collected = Vec::with_capacity(stmts.len());
                        for s in stmts {
                            match stmt_to_ast(s) {
                                Ok(ast) => collected.push(ast),
                                Err(e) => {
                                    let wrapped = BatchSchemaError::Json {
                                        message: format!(
                                            "method[{}] `{}`: {}",
                                            mi,
                                            name,
                                            e.message()
                                        ),
                                        byte_offset: None,
                                    };
                                    emit_plugin013(a, &wrapped, spec_len);
                                    return 1;
                                }
                            }
                        }
                        collected
                    }
                    (None, Some(handle)) => {
                        // Consume the stmt handle from the arena. This is the
                        // §3.13 composition with §3.11.
                        match a.take_stmt(ctx, handle) {
                            Ok(stmt) => flatten_block(stmt),
                            Err(EmitError::HandleConsumed { handle }) => {
                                a.emit_plugin008(handle);
                                return 1;
                            }
                            Err(e) => {
                                let wrapped = BatchSchemaError::UnresolvedBodyHandle {
                                    handle,
                                    reason: format!("{:?}", e),
                                };
                                emit_plugin013(a, &wrapped, spec_len);
                                return 1;
                            }
                        }
                    }
                    (Some(_), Some(_)) => {
                        let wrapped = BatchSchemaError::Json {
                            message: format!(
                                "method[{}] `{}`: both `body` and `body_handle` are set; exactly one required",
                                mi, name
                            ),
                            byte_offset: None,
                        };
                        emit_plugin013(a, &wrapped, spec_len);
                        return 1;
                    }
                    (None, None) => {
                        let wrapped = BatchSchemaError::Json {
                            message: format!(
                                "method[{}] `{}`: neither `body` nor `body_handle` is set",
                                mi, name
                            ),
                            byte_offset: None,
                        };
                        emit_plugin013(a, &wrapped, spec_len);
                        return 1;
                    }
                };
                method_bodies.push(body);
                method_headers.push(m);
            }

            // Restore method headers so class_to_ast can consume them
            // alongside `method_bodies` (same order, same length).
            class_spec.methods = method_headers;

            let exported = (flags & 1) != 0;

            // 3. Build the AST class.
            let class = match class_to_ast(class_spec, method_bodies, exported) {
                Ok(c) => c,
                Err(e) => {
                    emit_plugin013(a, &e, spec_len);
                    return 1;
                }
            };

            a.expansion.classes.push(class);
            0
        },
    )?;

    // Silence unused-import warning for the crate-internal batch_schema
    // module when this bridge is compiled but tests are not.
    let _ = batch_schema::BATCH_FUNCTION_LIMIT;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// §4.5  Error reporting
// ─────────────────────────────────────────────────────────────────────────────

fn register_error_bridge(linker: &mut Linker<PluginState>) -> Result<()> {
    // _emit_error(ctx, severity_i32, code_lp, message_lp, span_lp) -> 0
    linker.func_wrap(
        "env",
        "_emit_error",
        |mut caller: Caller<'_, PluginState>,
         ctx: i32,
         severity: i32,
         code_lp: i32,
         message_lp: i32,
         span_lp: i32|
         -> i32 {
            let code = read_lp_string(&mut caller, code_lp).unwrap_or_default();
            let message = read_lp_string(&mut caller, message_lp).unwrap_or_default();
            let span_json = read_lp_string(&mut caller, span_lp).unwrap_or_default();
            let a = arena!(caller);
            // ctx validation: if ctx is wrong we still emit the diagnostic (the
            // call came from the plugin) but we don't set saw_error_severity via
            // the normal path — instead we emit a secondary PLUGIN_WRONG_CTX error.
            if a.check_ctx(ctx).is_err() {
                a.emit_diagnostic(EmitDiagnostic {
                    severity: 2,
                    code: "PLUGIN_WRONG_CTX".to_string(),
                    message: format!(
                        "_emit_error called with wrong ctx {} (expected {})",
                        ctx,
                        a.ctx_handle()
                    ),
                    span_json: String::new(),
                });
                return 0;
            }
            let sev = severity.clamp(1, 2) as u8;
            a.emit_diagnostic(EmitDiagnostic {
                severity: sev,
                code,
                message,
                span_json,
            });
            0
        },
    )?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests for bridges (invoke helpers directly, no WASM round-trip)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::arena::EmitArena;
    use super::super::json::decode_handle_array;
    use super::*;
    use crate::ast::{BinaryOperator, Expression, Statement, Type, Value};

    fn make_arena(ctx: i32) -> EmitArena {
        EmitArena::new(ctx)
    }

    // ── Type constructors build correct AST nodes ─────────────────────────────

    #[test]
    fn type_alloc_and_take() {
        let mut arena = make_arena(1);
        let h = arena.alloc_type(Type::Integer);
        let t = arena.take_type(1, h).unwrap();
        assert_eq!(t, Type::Integer);
    }

    #[test]
    fn type_array_wraps_elem() {
        let mut arena = make_arena(1);
        let elem_h = arena.alloc_type(Type::String);
        let elem = arena.take_type(1, elem_h).unwrap();
        let array_type = Type::List(Box::new(elem), crate::ast::ListBehavior::Default);
        let h = arena.alloc_type(array_type.clone());
        let got = arena.take_type(1, h).unwrap();
        assert_eq!(got, array_type);
    }

    // ── Expression constructors ────────────────────────────────────────────────

    #[test]
    fn expr_int_lit_assembles_i64() {
        let lo: i32 = 2;
        let hi: i32 = 0;
        let value = ((hi as i64) << 32) | (lo as u32 as i64);
        assert_eq!(value, 2_i64);

        let mut arena = make_arena(1);
        let h = arena.alloc_expr(Expression::Literal(Value::Integer(value)));
        let e = arena.take_expr(1, h).unwrap();
        assert_eq!(e, Expression::Literal(Value::Integer(2)));
    }

    #[test]
    fn expr_bool_lit_false() {
        let mut arena = make_arena(1);
        let h = arena.alloc_expr(Expression::Literal(Value::Boolean(false)));
        let e = arena.take_expr(1, h).unwrap();
        assert_eq!(e, Expression::Literal(Value::Boolean(false)));
    }

    // ── Statement constructors ─────────────────────────────────────────────────

    #[test]
    fn stmt_return_with_expr() {
        let mut arena = make_arena(1);
        let expr_h = arena.alloc_expr(Expression::Literal(Value::Integer(42)));
        let expr = arena.take_expr(1, expr_h).unwrap();
        let stmt_h = arena.alloc_stmt(Statement::Return {
            value: Some(expr),
            location: None,
        });
        let stmt = arena.take_stmt(1, stmt_h).unwrap();
        match stmt {
            Statement::Return {
                value: Some(Expression::Literal(Value::Integer(42))),
                ..
            } => {}
            other => panic!("unexpected stmt: {:?}", other),
        }
    }

    #[test]
    fn stmt_block_via_handle_array_json() {
        // Ensure our JSON decode helper works for a simple array.
        let json = "[1, 2, 3]";
        let handles = decode_handle_array(json).unwrap();
        assert_eq!(handles, vec![1, 2, 3]);
    }

    // ── Binary op mapping ──────────────────────────────────────────────────────

    #[test]
    fn binop_mapping_coverage() {
        use super::super::arena::map_binary_op;
        assert_eq!(map_binary_op("+"), Some(BinaryOperator::Add));
        assert_eq!(map_binary_op("=="), Some(BinaryOperator::Equal));
        assert_eq!(map_binary_op("and"), Some(BinaryOperator::And));
        assert_eq!(map_binary_op("???"), None);
    }

    // ── Declaration emitters ──────────────────────────────────────────────────

    // ── Sub-cycle 3: _define_function happy path ──────────────────────────────

    #[test]
    fn define_function_allocates_pending_function_handle() {
        let mut arena = make_arena(11);
        // Build a body block containing return 5.
        let ret_t_h = arena.alloc_type(Type::Integer);
        let ret_t = arena.take_type(11, ret_t_h).unwrap();

        let five_h = arena.alloc_expr(Expression::Literal(Value::Integer(5)));
        let five = arena.take_expr(11, five_h).unwrap();
        let ret_stmt = Statement::Return {
            value: Some(five),
            location: None,
        };
        let body_block = Statement::If {
            condition: Expression::Literal(Value::Boolean(true)),
            then_branch: vec![ret_stmt],
            else_branch: None,
            location: None,
        };
        let body_h = arena.alloc_stmt(body_block);
        let body = flatten_block(arena.take_stmt(11, body_h).unwrap());

        let func = Function::new("helper".to_string(), Vec::new(), ret_t, body, None);
        let pf_handle = arena.alloc_pending_function(func);
        assert!(pf_handle > 0);

        // PendingFunction handle is NOT in expansion.functions — must be
        // consumed by _emit_class or _emit_route to be emitted.
        assert!(arena.expansion.functions.is_empty());

        // Taking it back works.
        let retrieved = arena.take_pending_function(11, pf_handle).unwrap();
        assert_eq!(retrieved.name, "helper");
    }

    // ── Sub-cycle 3: PLUGIN008 on method handle reuse ─────────────────────────

    #[test]
    fn define_function_pending_handle_reuse_emits_plugin008() {
        let mut arena = make_arena(12);
        let ret_t = Type::Void;
        let func = Function::new("m".to_string(), Vec::new(), ret_t, Vec::new(), None);
        let h = arena.alloc_pending_function(func);

        // First take consumes the handle.
        let _ = arena.take_pending_function(12, h).unwrap();

        // Second take must report HandleConsumed (PLUGIN008).
        let err = arena.take_pending_function(12, h).unwrap_err();
        assert_eq!(err.error_code(), "PLUGIN008");
    }

    // ── Sub-cycle 3: _emit_class consumes a PendingFunction method handle ─────

    #[test]
    fn emit_class_method_handle_consumed_correctly() {
        let mut arena = make_arena(13);
        // Allocate a method via _define_function semantics.
        let m = Function::new("ping".to_string(), Vec::new(), Type::Void, Vec::new(), None);
        let m_handle = arena.alloc_pending_function(m);

        // Simulate _emit_class consuming it.
        let pf = arena.take_pending_function(13, m_handle).unwrap();
        assert_eq!(pf.name, "ping");

        // Re-using the same method handle in a second _emit_class call must
        // be PLUGIN008.
        let err = arena.take_pending_function(13, m_handle).unwrap_err();
        assert_eq!(err.error_code(), "PLUGIN008");
    }

    // ── Sub-cycle 3: _emit_external host class — PLUGIN007 reasoning ──────────

    #[test]
    fn plugin007_error_code_matches_spec() {
        // The PLUGIN007 diagnostic is emitted inside the bridge closure; this
        // test asserts the diagnostic code we use matches the spec id.
        let mut arena = make_arena(1);
        arena.emit_diagnostic(super::super::error::EmitDiagnostic {
            severity: 2,
            code: "PLUGIN007".to_string(),
            message: "host class mismatch".to_string(),
            span_json: String::new(),
        });
        let (_exp, diags, saw_err) = arena.finish();
        assert!(saw_err);
        assert_eq!(diags[0].code, "PLUGIN007");
    }

    // ── §3.11: _emit_stmt_from_source ─────────────────────────────────────────

    use crate::parser::grammar::{CleanParser, SingleStatementParse};

    #[test]
    fn parse_single_statement_happy_path_call() {
        // A call fragment must parse as a Statement (not ExpressionNotStatement,
        // not ParseError). We don't over-constrain the inner shape here because
        // `print(...)` may be parsed via `print_parenthesized_stmt` or via the
        // generic function-call expression path depending on grammar routing.
        let src = "print(\"hi\")";
        match CleanParser::parse_single_statement(src) {
            SingleStatementParse::Statement(_) => {}
            other => panic!("expected Statement, got {:?}", type_name(&other)),
        }
    }

    #[test]
    fn parse_single_statement_return_stmt() {
        let src = "return 42";
        match CleanParser::parse_single_statement(src) {
            SingleStatementParse::Statement(Statement::Return { .. }) => {}
            other => panic!("expected return statement, got {:?}", type_name(&other)),
        }
    }

    #[test]
    fn parse_single_statement_expression_not_statement() {
        // `1 + 1` is a valid expression but not a valid bare statement.
        let src = "1 + 1";
        match CleanParser::parse_single_statement(src) {
            SingleStatementParse::ExpressionNotStatement => {}
            other => panic!(
                "expected ExpressionNotStatement, got {:?}",
                type_name(&other)
            ),
        }
    }

    #[test]
    fn parse_single_statement_syntax_error() {
        let src = "return + +";
        match CleanParser::parse_single_statement(src) {
            SingleStatementParse::ParseError { .. } => {}
            other => panic!("expected ParseError, got {:?}", type_name(&other)),
        }
    }

    #[test]
    fn parse_single_statement_multiple_stmts_wraps_in_block() {
        let src = "x = 1\nprint(x)";
        match CleanParser::parse_single_statement(src) {
            SingleStatementParse::Statement(Statement::If {
                then_branch,
                condition: Expression::Literal(Value::Boolean(true)),
                ..
            }) => assert_eq!(then_branch.len(), 2),
            other => panic!(
                "expected wrapped block statement, got {:?}",
                type_name(&other)
            ),
        }
    }

    #[test]
    fn emit_stmt_from_source_arena_alloc_and_single_consumption() {
        // Simulate the bridge body directly: parse then alloc_stmt into arena.
        let mut arena = make_arena(42);
        let result = CleanParser::parse_single_statement("print(\"hello\")");
        let stmt = match result {
            SingleStatementParse::Statement(s) => s,
            _ => panic!("expected happy path"),
        };
        let h = arena.alloc_stmt(stmt);
        assert!(h > 0);
        // First consumption succeeds.
        let _ = arena.take_stmt(42, h).expect("first take");
        // Second consumption triggers PLUGIN008.
        let err = arena.take_stmt(42, h).unwrap_err();
        assert_eq!(err.error_code(), "PLUGIN008");
    }

    #[test]
    fn plugin012_error_code_matches_spec() {
        // The PLUGIN012 diagnostic is emitted inside the bridge closure; this
        // asserts the diagnostic code we use matches the reserved spec id.
        let mut arena = make_arena(1);
        arena.emit_diagnostic(super::super::error::EmitDiagnostic {
            severity: 2,
            code: "PLUGIN012".to_string(),
            message: "expression in statement context".to_string(),
            span_json: super::build_span_json(200, 210),
        });
        let (_exp, diags, saw_err) = arena.finish();
        assert!(saw_err);
        assert_eq!(diags[0].code, "PLUGIN012");
        assert!(diags[0].span_json.contains("\"start\":200"));
        assert!(diags[0].span_json.contains("\"source\":\"block_body\""));
    }

    #[test]
    fn origin_offset_translation_shifts_span() {
        // Emulate the bridge's span-building logic: origin=200 with a fragment
        // of length 30 → span 200..230.
        let json = super::build_span_json(200, 230);
        assert!(json.contains("\"start\":200"));
        assert!(json.contains("\"end\":230"));
    }

    fn type_name(v: &SingleStatementParse) -> &'static str {
        match v {
            SingleStatementParse::Statement(_) => "Statement",
            SingleStatementParse::ExpressionNotStatement => "ExpressionNotStatement",
            SingleStatementParse::ParseError { .. } => "ParseError",
        }
    }

    #[test]
    fn emit_function_builds_correctly() {
        let mut arena = make_arena(7);
        // Allocate return type = integer
        let ret_h = arena.alloc_type(Type::Integer);
        // Allocate body: return 2
        let val_h = arena.alloc_expr(Expression::Literal(Value::Integer(2)));
        let val_expr = arena.take_expr(7, val_h).unwrap();
        let ret_stmt_h = arena.alloc_stmt(Statement::Return {
            value: Some(val_expr),
            location: None,
        });
        // Build block (single stmt wrapping using flatten_block semantics)
        let block_stmt = Statement::If {
            condition: Expression::Literal(Value::Boolean(true)),
            then_branch: vec![arena.take_stmt(7, ret_stmt_h).unwrap()],
            else_branch: None,
            location: None,
        };
        let block_h = arena.alloc_stmt(block_stmt);

        // Consume return type and body to build function
        let ret_type = arena.take_type(7, ret_h).unwrap();
        let body_stmt = arena.take_stmt(7, block_h).unwrap();
        let body = flatten_block(body_stmt);
        let func = Function::new("two".to_string(), Vec::new(), ret_type, body, None);
        arena.expansion.functions.push(func);

        assert_eq!(arena.expansion.functions.len(), 1);
        assert_eq!(arena.expansion.functions[0].name, "two");
        match &arena.expansion.functions[0].body[0] {
            Statement::Return {
                value: Some(Expression::Literal(Value::Integer(2))),
                ..
            } => {}
            other => panic!("unexpected body: {:?}", other),
        }
    }
}
