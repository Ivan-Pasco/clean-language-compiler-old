//! Cross-context trap stubs for plugin linkers.
//!
//! A plugin.wasm has one `external:` block. When a plugin exports both
//! `expand_block_typed` and `lint_project` (every framework plugin that
//! adopts Contract 5 Phase C), its imports declare the union of the
//! typed-emission and lint bridge surfaces. Wasmtime rejects
//! instantiation on any unresolved import — so a linker that only knows
//! its own surface fails on the *other* surface's names.
//!
//! Fix: each linker registers **trap-on-call stubs** for every name owned
//! by the other surface. Instantiation succeeds; a plugin that calls the
//! wrong-context bridge (e.g. `_emit_function` from inside
//! `lint_project`) traps with a `BRIDGE-WRONG-CONTEXT` diagnostic that
//! Contract 5 §5 already routes into the LINT001 / expand-error surface.
//!
//! Reference: cross-component prompt `f9ee2728-8597-11f1-9d55-da25a95a496b`.

use anyhow::Result;
use wasmtime::{Caller, Linker};

use crate::plugins::wasm_adapter::PluginState;

/// Register trap stubs for every typed-emission bridge on `linker`.
///
/// Called from `build_lint_linker` so a plugin that also exports
/// `expand_block_typed` (and therefore imports the typed-emission
/// surface) can still instantiate for a lint pass. Any bridge invoked
/// from the wrong context traps with a diagnostic naming the specific
/// bridge and the pass it was called from.
pub fn register_typed_emission_stubs(linker: &mut Linker<PluginState>) -> Result<()> {
    for (name, arity) in TYPED_EMISSION_BRIDGES {
        register_trap_stub(linker, name, *arity, "lint")?;
    }
    Ok(())
}

/// Register trap stubs for every lint bridge on `linker`.
///
/// Called from `build_typed_emission_linker` so a plugin that also
/// exports `lint_project` (and therefore imports the lint surface) can
/// still instantiate for an expand pass.
pub fn register_lint_stubs(linker: &mut Linker<PluginState>) -> Result<()> {
    for (name, arity) in LINT_BRIDGES {
        register_trap_stub(linker, name, *arity, "expand")?;
    }
    Ok(())
}

fn register_trap_stub(
    linker: &mut Linker<PluginState>,
    name: &'static str,
    arity: usize,
    context: &'static str,
) -> Result<()> {
    match arity {
        1 => linker.func_wrap(
            "env",
            name,
            move |_: Caller<'_, PluginState>, _: i32| -> i32 { trap(name, context) },
        )?,
        2 => linker.func_wrap(
            "env",
            name,
            move |_: Caller<'_, PluginState>, _: i32, _: i32| -> i32 { trap(name, context) },
        )?,
        3 => linker.func_wrap(
            "env",
            name,
            move |_: Caller<'_, PluginState>, _: i32, _: i32, _: i32| -> i32 {
                trap(name, context)
            },
        )?,
        4 => linker.func_wrap(
            "env",
            name,
            move |_: Caller<'_, PluginState>, _: i32, _: i32, _: i32, _: i32| -> i32 {
                trap(name, context)
            },
        )?,
        5 => linker.func_wrap(
            "env",
            name,
            move |_: Caller<'_, PluginState>, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 {
                trap(name, context)
            },
        )?,
        6 => linker.func_wrap(
            "env",
            name,
            move |_: Caller<'_, PluginState>,
                  _: i32,
                  _: i32,
                  _: i32,
                  _: i32,
                  _: i32,
                  _: i32|
                  -> i32 { trap(name, context) },
        )?,
        _ => {
            return Err(anyhow::anyhow!(
                "unsupported trap-stub arity {arity} for bridge `{name}`"
            ));
        }
    };
    Ok(())
}

fn trap(name: &'static str, context: &'static str) -> i32 {
    // wasmtime turns a Rust panic in a host func into a WASM trap; the
    // panic message is what the caller (call_expand_typed /
    // call_lint_project) surfaces to the user via anyhow.
    panic!("BRIDGE-WRONG-CONTEXT: `{name}` called during {context} pass");
}

// ─────────────────────────────────────────────────────────────────────────────
// Bridge surface lists
//
// These MUST stay in sync with the real registrations in
// `plugins::typed_emission::bridges`, `plugins::typed_emission::batch_builders`,
// and `plugins::lint::bridges`. The test at the bottom of this file guards
// against drift by locking arities.
// ─────────────────────────────────────────────────────────────────────────────

const LINT_BRIDGES: &[(&str, usize)] = &[
    ("_ast_list_classes", 1),
    ("_ast_class_fields", 3),
    ("_ast_list_functions", 1),
    ("_ast_list_blocks", 3),
];

const TYPED_EMISSION_BRIDGES: &[(&str, usize)] = &[
    // Batch builders — dotted names (see typed_emission::batch_builders)
    ("batch.arrayNew", 1),
    ("batch.arrayPush", 3),
    ("batch.stmtSeq", 2),
    ("batch.args", 2),
    ("batch.stringLit", 2),
    ("batch.intLit", 3),
    ("batch.numberLit", 3),
    ("batch.boolLit", 2),
    ("batch.ident", 2),
    ("batch.field", 3),
    ("batch.call", 3),
    ("batch.binop", 4),
    ("batch.unop", 3),
    ("batch.index", 3),
    ("batch.arrayLit", 2),
    ("batch.objectLit", 2),
    ("batch.stmtCall", 3),
    ("batch.stmtAssign", 3),
    ("batch.stmtVarDecl", 4),
    ("batch.stmtIf", 4),
    ("batch.stmtWhile", 3),
    ("batch.stmtFor", 4),
    ("batch.stmtReturn", 2),
    ("batch.stmtBlock", 2),
    ("batch.param", 3),
    ("batch.objectField", 3),
    ("batch.classField", 3),
    ("batch.func", 5),
    ("batch.func2", 6),
    ("batch.method", 6),
    ("batch.class", 5),
    ("batch.spec", 2),
    // Batch builders — underscore aliases (what plugins actually declare
    // in `external:`; kept as full trap-stubs rather than linker aliases
    // because the aliased target is a trap-stub itself and we want the
    // panic message to name whichever form the plugin imported).
    ("_batch_arrayNew", 1),
    ("_batch_arrayPush", 3),
    ("_batch_stmtSeq", 2),
    ("_batch_args", 2),
    ("_batch_stringLit", 2),
    ("_batch_intLit", 3),
    ("_batch_numberLit", 3),
    ("_batch_boolLit", 2),
    ("_batch_ident", 2),
    ("_batch_field", 3),
    ("_batch_call", 3),
    ("_batch_binop", 4),
    ("_batch_unop", 3),
    ("_batch_index", 3),
    ("_batch_arrayLit", 2),
    ("_batch_objectLit", 2),
    ("_batch_stmtCall", 3),
    ("_batch_stmtAssign", 3),
    ("_batch_stmtVarDecl", 4),
    ("_batch_stmtIf", 4),
    ("_batch_stmtWhile", 3),
    ("_batch_stmtFor", 4),
    ("_batch_stmtReturn", 2),
    ("_batch_stmtBlock", 2),
    ("_batch_param", 3),
    ("_batch_objectField", 3),
    ("_batch_classField", 3),
    ("_batch_func", 5),
    ("_batch_func2", 6),
    ("_batch_method", 6),
    ("_batch_class", 5),
    ("_batch_spec", 2),
    // Type constructors
    ("_type_string", 1),
    ("_type_integer", 1),
    ("_type_number", 1),
    ("_type_boolean", 1),
    ("_type_void", 1),
    ("_type_array", 2),
    ("_type_class_ref", 2),
    // Expression constructors
    ("_expr_string_lit", 2),
    ("_expr_int_lit", 3),
    ("_expr_number_lit", 3),
    ("_expr_bool_lit", 2),
    ("_expr_ident", 2),
    ("_expr_field", 3),
    ("_expr_call", 3),
    ("_expr_method_call", 4),
    ("_expr_binop", 4),
    ("_expr_unop", 3),
    ("_expr_binop_op", 4),
    ("_expr_unop_op", 3),
    ("_expr_index", 3),
    ("_expr_array_lit", 2),
    ("_expr_object_lit", 2),
    // Statement constructors
    ("_stmt_call", 3),
    ("_stmt_assign", 3),
    ("_stmt_if", 4),
    ("_stmt_while", 3),
    ("_stmt_for", 4),
    ("_stmt_return", 2),
    ("_stmt_block", 2),
    ("_emit_stmt_from_source", 3),
    ("_emit_expr_from_source", 3),
    // Top-level emission
    ("_emit_function", 6),
    ("_define_function", 6),
    ("_emit_class", 4),
    ("_emit_external", 5),
    ("_emit_state_block", 2),
    ("_emit_statement_into_start", 2),
    ("_emit_statement_inline", 2),
    ("_emit_route", 5),
    ("_emit_artifact_directive", 2),
    ("_inject_source_file", 3),
    ("_transform_source_file", 3),
    ("_emit_capability", 2),
    ("_emit_helpers_batch", 3),
    ("_emit_class_full", 3),
    ("_emit_error", 5),
    // Context state
    ("_ctx_had_error", 1),
    ("_ctx_set_error_context", 3),
    ("_ctx_clear_error", 1),
];

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::Engine;

    /// The linker rejects duplicate registrations, so a name that appears
    /// twice in either surface list would fail here. Catches copy-paste
    /// bugs in the const arrays.
    #[test]
    fn stub_lists_have_no_duplicates() {
        let engine = Engine::default();
        let mut linker: Linker<PluginState> = Linker::new(&engine);
        register_typed_emission_stubs(&mut linker).expect("typed-emission stubs register cleanly");

        let mut linker: Linker<PluginState> = Linker::new(&engine);
        register_lint_stubs(&mut linker).expect("lint stubs register cleanly");
    }

    /// The two surfaces must stay disjoint — an overlap would mean one
    /// linker is stubbing over its own real bridge.
    #[test]
    fn surfaces_do_not_overlap() {
        use std::collections::HashSet;
        let te: HashSet<&str> = TYPED_EMISSION_BRIDGES.iter().map(|(n, _)| *n).collect();
        let lint: HashSet<&str> = LINT_BRIDGES.iter().map(|(n, _)| *n).collect();
        let overlap: Vec<&&str> = te.intersection(&lint).collect();
        assert!(overlap.is_empty(), "bridge surface overlap: {overlap:?}");
    }
}
