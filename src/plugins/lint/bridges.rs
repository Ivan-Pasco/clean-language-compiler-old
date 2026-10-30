/// WASM host bridges for the plugin lint extension (Contract 5, Phase B).
///
/// Registers the 4 read-only AST accessor functions on a dedicated lint
/// linker. All accessors return an LP-string pointer into plugin memory
/// containing a JSON payload described in
/// `foundation/spec/framework/contracts/lint-extension.md` §4.
///
/// Bridges dispatch to the pure `LintArena` accessor methods; the WASM-facing
/// code here is only responsible for:
///   - Reading LP string parameters out of plugin memory
///   - Fetching the arena from `PluginState.lint_arena`
///   - Writing the returned JSON back into plugin memory as an LP string
///
/// The linker is built by `WasmPluginAdapter::build_lint_linker` and driven by
/// `call_lint_project`.
use anyhow::Result;
use wasmtime::{Caller, Linker};

use crate::plugins::wasm_adapter::{read_clean_string, write_clean_string, PluginState};

/// Fetch the lint arena from caller state, or a diagnostic message when
/// the caller invoked a lint bridge outside a `lint_project` call.
///
/// Returning the JSON "not in lint" payload rather than trapping keeps the
/// graceful-degradation guarantee — a plugin that somehow invokes a lint
/// bridge from `expand_block` sees a schema-checkable error, not a WASM trap.
fn arena_or_error(caller: &mut Caller<'_, PluginState>) -> Result<i32, i32> {
    if caller.data().lint_arena.is_some() {
        Ok(0) // caller reads arena directly from state
    } else {
        // Write the "not in lint" error payload directly and hand the pointer
        // back to the plugin. The Err branch signals the bridge to short-circuit.
        let msg = b"{\"error\":\"NOT-IN-LINT-CALL\"}";
        Err(write_clean_string(caller, msg))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bridge registration
// ─────────────────────────────────────────────────────────────────────────────

/// Register the 4 lint accessor bridges on the supplied linker.
///
/// Called by `WasmPluginAdapter::build_lint_linker`. The linker is separate
/// from the v1 / typed-emission linkers so a lint call gets exactly the
/// lint surface plus stdlib.
pub fn register_lint_bridges(linker: &mut Linker<PluginState>) -> Result<()> {
    // _ast_list_classes(handle) -> lp_ptr
    linker.func_wrap(
        "env",
        "_ast_list_classes",
        |mut caller: Caller<'_, PluginState>, handle: i32| -> i32 {
            if let Err(ptr) = arena_or_error(&mut caller) {
                return ptr;
            }
            let json = caller
                .data()
                .lint_arena
                .as_ref()
                .expect("lint_arena present — checked by arena_or_error")
                .list_classes_json(handle);
            write_clean_string(&mut caller, json.as_bytes())
        },
    )?;

    // _ast_class_fields(handle, name_lp) -> lp_ptr
    //
    // String param uses the plugin LP-pointer ABI: one i32 pointing at
    // `[4-byte length header || content]` in plugin memory. This matches
    // every other bridge across typed-emission, host-bridge, and server
    // extensions. Spec §4 (post-2026-07-23 alignment) declares 2 i32s.
    linker.func_wrap(
        "env",
        "_ast_class_fields",
        |mut caller: Caller<'_, PluginState>, handle: i32, name_lp: i32| -> i32 {
            if let Err(ptr) = arena_or_error(&mut caller) {
                return ptr;
            }
            let name = read_clean_string(&mut caller, name_lp).unwrap_or_default();
            let json = caller
                .data()
                .lint_arena
                .as_ref()
                .expect("lint_arena present — checked by arena_or_error")
                .class_fields_json(handle, &name);
            write_clean_string(&mut caller, json.as_bytes())
        },
    )?;

    // _ast_list_functions(handle) -> lp_ptr
    linker.func_wrap(
        "env",
        "_ast_list_functions",
        |mut caller: Caller<'_, PluginState>, handle: i32| -> i32 {
            if let Err(ptr) = arena_or_error(&mut caller) {
                return ptr;
            }
            let json = caller
                .data()
                .lint_arena
                .as_ref()
                .expect("lint_arena present — checked by arena_or_error")
                .list_functions_json(handle);
            write_clean_string(&mut caller, json.as_bytes())
        },
    )?;

    // _ast_list_blocks(handle, name_lp) -> lp_ptr (LP-pointer ABI, see above)
    linker.func_wrap(
        "env",
        "_ast_list_blocks",
        |mut caller: Caller<'_, PluginState>, handle: i32, name_lp: i32| -> i32 {
            if let Err(ptr) = arena_or_error(&mut caller) {
                return ptr;
            }
            let name = read_clean_string(&mut caller, name_lp).unwrap_or_default();
            let json = caller
                .data()
                .lint_arena
                .as_ref()
                .expect("lint_arena present — checked by arena_or_error")
                .list_blocks_json(handle, &name);
            write_clean_string(&mut caller, json.as_bytes())
        },
    )?;

    // _ast_block_subblocks(handle, name_lp) -> lp_ptr
    //
    // Nested-tree variant of `_ast_list_blocks`. See
    // `foundation/spec/framework/contracts/lint-extension.md` §4.5.
    linker.func_wrap(
        "env",
        "_ast_block_subblocks",
        |mut caller: Caller<'_, PluginState>, handle: i32, name_lp: i32| -> i32 {
            if let Err(ptr) = arena_or_error(&mut caller) {
                return ptr;
            }
            let name = read_clean_string(&mut caller, name_lp).unwrap_or_default();
            let json = caller
                .data()
                .lint_arena
                .as_ref()
                .expect("lint_arena present — checked by arena_or_error")
                .block_subblocks_json(handle, &name);
            write_clean_string(&mut caller, json.as_bytes())
        },
    )?;

    Ok(())
}
