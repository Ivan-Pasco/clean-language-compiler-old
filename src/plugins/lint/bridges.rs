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
/// The 4 bridges are dormant in Cycle 1 — the linker is built by
/// `WasmPluginAdapter::build_lint_linker` but there is no `call_lint_project`
/// path yet, so no plugin can invoke them. Cycle 2 wires the invocation.
use anyhow::Result;
use wasmtime::{Caller, Linker};

use crate::plugins::wasm_adapter::PluginState;

// ─────────────────────────────────────────────────────────────────────────────
// LP-string I/O helpers (private to the lint bridges; mirrors the same
// helpers used in `typed_emission::bridges` — a few dozen lines of trivially-
// tested plumbing that isn't worth exposing as `pub(crate)` on wasm_adapter).
// ─────────────────────────────────────────────────────────────────────────────

/// Read an LP-string (4-byte LE length + bytes) from plugin memory at `ptr`.
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
    let end = start + 4 + len;
    if end > data.len() {
        return None;
    }
    std::str::from_utf8(&data[start + 4..end])
        .ok()
        .map(|s| s.to_string())
}

/// Allocate space in plugin memory via the bump allocator, write the LP
/// header + bytes, and return the pointer. Returns 0 on any failure — the
/// plugin treats 0 as an empty response (see spec §5 graceful degradation).
///
/// This mirrors `wasm_adapter::write_clean_string`, kept private to the
/// lint bridges so we don't need to widen wasm_adapter's visibility.
fn write_lp_string(caller: &mut Caller<'_, PluginState>, data: &[u8]) -> i32 {
    let data_len = data.len();
    let total_size = 4 + data_len;

    let ptr = caller.data_mut().allocate(total_size);

    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => m,
        None => return 0,
    };

    let current_size = memory.data_size(&mut *caller);
    let required_size = ptr + total_size;
    if required_size > current_size {
        let pages_needed = (required_size - current_size).div_ceil(65536);
        if memory.grow(&mut *caller, pages_needed as u64).is_err() {
            return 0;
        }
    }

    let len_bytes = (data_len as u32).to_le_bytes();
    if memory.write(&mut *caller, ptr, &len_bytes).is_err() {
        return 0;
    }
    if memory.write(&mut *caller, ptr + 4, data).is_err() {
        return 0;
    }

    ptr as i32
}

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
        Err(write_lp_string(caller, msg))
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
            write_lp_string(&mut caller, json.as_bytes())
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
            let name = read_lp_string(&mut caller, name_lp).unwrap_or_default();
            let json = caller
                .data()
                .lint_arena
                .as_ref()
                .expect("lint_arena present — checked by arena_or_error")
                .class_fields_json(handle, &name);
            write_lp_string(&mut caller, json.as_bytes())
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
            write_lp_string(&mut caller, json.as_bytes())
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
            let name = read_lp_string(&mut caller, name_lp).unwrap_or_default();
            let json = caller
                .data()
                .lint_arena
                .as_ref()
                .expect("lint_arena present — checked by arena_or_error")
                .list_blocks_json(handle, &name);
            write_lp_string(&mut caller, json.as_bytes())
        },
    )?;

    Ok(())
}
