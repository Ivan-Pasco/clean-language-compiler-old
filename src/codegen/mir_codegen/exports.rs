//! The module's exports, and the names user functions may not take.
//!
//! A compiled module exports what hosts look up by name
//! (`foundation/spec/platform/MEMORY_MODEL.md`, `BOXED_ANY_ABI.md` §6,
//! `SERVER_EXTENSIONS.md`):
//!
//! - `memory` and the `__heap_ptr` global;
//! - the allocator hosts call: `malloc`, `free`, `scope_push`, `scope_pop`;
//! - `_start` (and `_run_tests` when the file has tests);
//! - every function of the program, including plugin-generated ones, under
//!   its own name: hosts call route handlers, event handlers, callbacks
//!   (`_frame_callback`) and plugin entry points (`expand_block`) by the
//!   name the program hands them;
//! - `handle_event_N` for handlers registered through a `Handler` bridge
//!   parameter, and `__indirect_function_table` when functions are used as
//!   values;
//! - in a `--emit-heap-probes` build, the probed string-builder helpers.
//!
//! Imports and the compiler's own runtime helpers (`__string_concat`,
//! `list.add`, ...) are not exported.

use super::*;

/// Runtime helpers hosts call by their export name.
const HOST_RUNTIME_EXPORTS: [&str; 4] = ["malloc", "free", "scope_push", "scope_pop"];

/// The string-builder helpers `--emit-heap-probes` instruments. A probe
/// build exports them so tooling can tell which function indices the probes
/// follow (`maybe_emit_probe_after_call`).
const PROBED_HELPER_EXPORTS: [&str; 4] = [
    "string_builder_append",
    "string_builder_finalize",
    "string_builder_append_transient",
    "string_builder_finalize_to_main",
];

/// Export names the module defines itself, which no program function may
/// take.
fn is_reserved_export_name(name: &str) -> bool {
    matches!(
        name,
        "memory" | "__heap_ptr" | "__indirect_function_table" | "_start" | "_run_tests"
    ) || name.starts_with("handle_event_")
}

impl MirCodeGenerator<'_> {
    /// Reject program functions whose names belong to the runtime.
    ///
    /// Program functions, imports and runtime helpers share one function
    /// namespace (`function_map`), and runtime code calls its helpers by
    /// name: a function named `string_to_int` used to replace the helper
    /// `json.get` relies on, and one named `malloc` became the allocator
    /// hosts call.
    ///
    /// No function may take a name the module exports itself or a host
    /// runtime export. A function from the program's source files may not
    /// take any runtime name either: those registered before the program
    /// functions are in `function_map`, `registered_later` lists the ones
    /// registered after them. Plugin-generated functions may: a plugin can
    /// implement a language-level name the compiler also maps to a bridge
    /// (frame.server's `redirect`).
    pub(super) fn check_function_names(
        &self,
        functions: &[(SymbolId, MirFunction)],
        registered_later: &[&str],
    ) -> Result<(), CompilerError> {
        for (_, function) in functions {
            let name = function.name.as_str();
            let from_plugin = function.location.file == crate::ast::PLUGIN_OUTPUT_MARKER
                || function.location.file == crate::ast::PLUGIN_OUTPUT_V2_ROOT_MARKER;
            let runtime_name = self.wasm_generator.function_map.contains_key(name)
                || registered_later.contains(&name);
            let taken = (runtime_name && !from_plugin)
                || is_reserved_export_name(name)
                || HOST_RUNTIME_EXPORTS.contains(&name);
            if taken {
                return Err(CompilerError::semantic_error_with_code(
                    format!(
                        "function name '{name}' is reserved: the runtime defines a function or export with this name"
                    ),
                    Some("rename the function".to_string()),
                    Some(function.location.clone()),
                    "SEM003",
                ));
            }
        }
        Ok(())
    }

    /// Export every program function and the host runtime functions.
    ///
    /// Sorted by name: `function_map` is a `HashMap`, and iterating it
    /// directly made every recompilation produce a different binary.
    pub(super) fn export_functions(&mut self) {
        let total_functions = self.wasm_generator.function_count;
        let probed: &[&str] = if crate::emit_heap_probes_override() {
            &PROBED_HELPER_EXPORTS
        } else {
            &[]
        };
        let mut names: Vec<&str> = self
            .user_defined_function_names
            .iter()
            .map(String::as_str)
            .chain(HOST_RUNTIME_EXPORTS)
            .chain(probed.iter().copied())
            .collect();
        names.sort_unstable();
        names.dedup();
        for name in names {
            let Some(&index) = self.wasm_generator.function_map.get(name) else {
                continue;
            };
            if index >= total_functions {
                tracing::error!(
                    name = %name,
                    index = index,
                    total_functions = total_functions,
                    "Refusing to emit export with out-of-bounds function index \
                     (upstream registration bug)"
                );
                continue;
            }
            self.wasm_generator
                .export_section
                .export(name, wasm_encoder::ExportKind::Func, index);
        }
    }
}
