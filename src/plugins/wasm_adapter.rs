//! WASM Plugin Adapter
//!
//! Wraps a WASM module to implement the FrameworkPlugin trait
//! Provides full Clean Language runtime environment for plugin execution

use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;
use std::sync::atomic::{AtomicI32, Ordering};
use wasmtime::{Caller, Engine, Linker, Memory, Module, Store, TypedFunc};

use crate::ast::{FrameworkBlock, Statement};
use crate::plugins::{
    FrameworkPlugin, PluginCompletionItem, PluginDiagnostic, PluginError, PluginExpansion,
    PluginHoverInfo, PluginLspContext, PluginResult,
};

use super::plugin_abi::PluginManifest;

/// Adapter that wraps a WASM plugin module
pub struct WasmPluginAdapter {
    /// Plugin name
    name: String,
    /// Plugin manifest providing export names and version information
    manifest: PluginManifest,
    /// Compiled WASM module
    module: Module,
    /// Wasmtime engine
    engine: Engine,
    /// Cached block types this plugin handles
    handles_cache: Vec<&'static str>,
    /// Cached ORM expression patterns from `[handles] expressions`
    expression_patterns_cache: Vec<String>,
    /// Cached name string
    name_cache: &'static str,
    /// Cached version string
    version_cache: &'static str,
    /// Cached description string
    description_cache: &'static str,
    /// Cached Linker — set up once, reused across all expand calls.
    /// The Linker is bound to the Engine (not the Store), so it can be
    /// shared across multiple Store/Instance pairs.
    cached_linker: Option<Linker<PluginState>>,
    /// Plugin Contracts v2 — shared per-build state. Captured by the
    /// `_build_state_set` / `_build_state_get` bridge stubs in the linker so
    /// every plugin loaded with the same `BuildState` shares the same store.
    /// See `foundation/spec/plugins/contracts/lifecycle.md` §2.5.
    build_state: crate::plugins::BuildState,
    /// Monotonically-increasing counter for typed-emission ctx handles.
    /// Each `call_expand_typed` call allocates a fresh ctx by fetching-and-
    /// incrementing this counter. Because ctx values are never recycled, a
    /// plugin cannot accidentally re-use a handle from a prior call even if
    /// it stored the old ctx in its `_build_state_set` keystore.
    next_ctx_counter: AtomicI32,
}

impl WasmPluginAdapter {
    /// Create a new WASM plugin adapter
    pub fn new(
        name: String,
        manifest: PluginManifest,
        module: Module,
        engine: Engine,
    ) -> Result<Self> {
        // Pre-cache static strings to avoid repeated allocations
        let name_cache: &'static str = Box::leak(name.clone().into_boxed_str());
        let version_cache: &'static str =
            Box::leak(manifest.plugin.version.clone().into_boxed_str());
        let description_cache: &'static str =
            Box::leak(manifest.plugin.description.clone().into_boxed_str());

        let handles_cache: Vec<&'static str> = manifest
            .handles
            .blocks
            .iter()
            .map(|s| Box::leak(s.clone().into_boxed_str()) as &'static str)
            .collect();

        // Build expression patterns from the manifest, then augment by scanning the WASM
        // module exports for `expand_{verb}` functions.  This ensures that ORM verbs
        // with a direct dispatch export (e.g. `expand_exists`) are registered as handled
        // expression patterns even when the plugin's plugin.toml `[handles] expressions`
        // list is missing the corresponding `*.{verb}:` entry.
        //
        // The authoritative set of ORM-verb export names that the compiler's direct-dispatch
        // path in `call_expand` / `call_expand_full` knows how to call:
        let orm_dispatch_verbs = [
            "find", "first", "count", "exists", "insert", "update", "delete", "paginate", "cursor",
        ];
        let mut expression_patterns_cache: Vec<String> = manifest.handles.expressions.clone();
        {
            // Collect export names from the WASM module.
            let export_names: Vec<String> = module
                .exports()
                .filter(|e| matches!(e.ty(), wasmtime::ExternType::Func(_)))
                .map(|e| e.name().to_string())
                .collect();
            for verb in &orm_dispatch_verbs {
                let export_name = format!("expand_{}", verb);
                if export_names.iter().any(|n| n == &export_name) {
                    // Build the glob pattern this verb should be registered under.
                    let pattern = format!("*.{}:", verb);
                    // Add it only if not already present (exact or equivalent match).
                    let already_present = expression_patterns_cache
                        .iter()
                        .any(|p| p.trim_end_matches(':') == format!("*.{}", verb));
                    if !already_present {
                        tracing::debug!(
                            plugin = %name,
                            verb = verb,
                            pattern = %pattern,
                            "Auto-registering ORM verb pattern from plugin export"
                        );
                        expression_patterns_cache.push(pattern);
                    }
                }
            }
        }

        let mut adapter = Self {
            name,
            manifest,
            module,
            engine,
            handles_cache,
            expression_patterns_cache,
            name_cache,
            version_cache,
            description_cache,
            cached_linker: None,
            build_state: crate::plugins::new_build_state(),
            next_ctx_counter: AtomicI32::new(1),
        };

        // Pre-build the linker once — this sets up ~50+ host function stubs
        // and is expensive to do per-block call.
        let linker = adapter.setup_linker()?;
        adapter.cached_linker = Some(linker);

        Ok(adapter)
    }

    /// Replace the adapter's per-build state container so multiple plugins
    /// loaded into the same registry share a single keystore. Called by
    /// `WasmPluginLoader::load_plugins` immediately after `new()` so the
    /// linker's bridge stubs (set up once, cached) capture the shared store.
    ///
    /// Note: because the bridge stubs were registered before this point,
    /// the linker is rebuilt to ensure the captured `Arc` points to the
    /// shared store. Plugins loaded after `set_build_state` see the new
    /// state immediately on their next slot invocation.
    pub fn set_build_state(&mut self, state: crate::plugins::BuildState) -> Result<()> {
        self.build_state = state;
        // Rebuild the linker so the bridge stubs capture the new Arc.
        let linker = self.setup_linker()?;
        self.cached_linker = Some(linker);
        Ok(())
    }

    /// Create a new store with host functions.
    ///
    /// The store inherits the engine's epoch-based interruption (see
    /// `wasm_loader::build_engine`) and sets a per-call deadline derived
    /// from `CLN_PLUGIN_TIMEOUT_SECS` (default 30 s). A plugin call that
    /// runs past the deadline traps with `wasm trap: interrupt`, surfacing
    /// a WASM backtrace instead of hanging the compiler indefinitely —
    /// the diagnostic the framework team asked for under
    /// COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS.
    /// Setting the env var to `0` disables the deadline.
    fn create_store(&self) -> Store<PluginState> {
        let state = PluginState::new();
        let mut store = Store::new(&self.engine, state);
        let timeout = super::wasm_loader::plugin_timeout_secs();
        if timeout > 0 {
            let ticks = (timeout * 1000) / super::wasm_loader::EPOCH_TICK_MS;
            store.set_epoch_deadline(ticks.max(1));
            // `set_epoch_deadline` only schedules when the deadline fires;
            // the default action on expiry is `epoch_deadline_yield_and_continue`,
            // which under a sync `Func::call` simply resumes execution —
            // the ticker keeps incrementing but the plugin call never
            // traps and the compiler still hangs. `epoch_deadline_trap()`
            // changes the action to a `wasm trap: interrupt` that bubbles
            // through `Func::call` as an `anyhow::Error`, and the existing
            // bridge call sites already surface that as a structured
            // plugin error. This is the line that actually closes the
            // hang reported as COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS.
            store.epoch_deadline_trap();
        }
        store
    }

    /// Get a reference to the cached linker, or create one if not yet cached.
    fn get_linker(&self) -> Result<&Linker<PluginState>> {
        self.cached_linker
            .as_ref()
            .ok_or_else(|| anyhow!("Linker not initialized"))
    }

    /// Build a fresh typed-emission linker for a single v3 expansion call.
    ///
    /// Unlike the v1 linker (which is cached because it has no per-call state),
    /// the typed-emission linker is built once per `call_expand_typed` call
    /// because the bridge closures access `store.data_mut().emit_arena` — a
    /// per-call arena installed just before instantiation. If we tried to cache
    /// the linker across calls the closures would capture a stale arena reference.
    ///
    /// The overhead is acceptable: the typed-emission bridge surface is ~30
    /// functions compared to the v1 linker's ~120, and the arena fill rate
    /// per block expansion is at most a few hundred nodes.
    fn build_typed_emission_linker(&self) -> Result<Linker<PluginState>> {
        let mut linker = Linker::new(&self.engine);
        // Register the full stdlib first so any Clean Language code inside a v3
        // plugin body (string.concat, env::print, float_to_string, math ops, etc.)
        // resolves at instantiation time. Without this, wasmtime rejects the module
        // with "unknown import" for every stdlib function the plugin references.
        //
        // Bug: f4b7d6977f05 (TYPED-EMISSION-LINKER-INCOMPLETE) — this call was
        // missing, causing frame.locale (and any other v3 plugin that calls stdlib
        // from its expand_block_typed body) to fail instantiation.
        self.register_plugin_stdlib_functions(&mut linker)?;
        // Then overlay the 30 typed-emission bridge functions on top of the stdlib.
        // These are the v3-specific handles: _emit_function, _stmt_block, _stmt_return, etc.
        crate::plugins::typed_emission::register_typed_emission_bridges(&mut linker)?;
        Ok(linker)
    }

    /// Allocate the next monotonic ctx_handle for typed-emission.
    fn next_ctx_handle(&self) -> i32 {
        self.next_ctx_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Create a store with a typed-emission arena pre-installed.
    fn create_store_with_arena(
        &self,
        arena: crate::plugins::typed_emission::EmitArena,
    ) -> Store<PluginState> {
        let mut state = PluginState::new();
        state.emit_arena = Some(arena);
        let mut store = Store::new(&self.engine, state);
        let timeout = super::wasm_loader::plugin_timeout_secs();
        if timeout > 0 {
            let ticks = (timeout * 1000) / super::wasm_loader::EPOCH_TICK_MS;
            store.set_epoch_deadline(ticks.max(1));
            store.epoch_deadline_trap();
        }
        store
    }

    /// Plugin Contracts v3 typed expansion. Called when
    /// `dispatch.version >= 3` in `call_expand` or `call_expand_full`.
    ///
    /// Allocates an `EmitArena`, installs it into the store, instantiates the
    /// plugin with the typed-emission linker, calls `expand_block_typed`, drains
    /// the arena, routes diagnostics, and returns the accumulated `PluginExpansion`.
    fn call_expand_typed(
        &self,
        block: &FrameworkBlock,
        export_name: &str,
        block_name_str: &str,
        attributes_str: &str,
        body_str: &str,
    ) -> Result<PluginExpansion> {
        let ctx = self.next_ctx_handle();
        let arena = crate::plugins::typed_emission::EmitArena::new(ctx);
        let mut store = self.create_store_with_arena(arena);

        let linker = self.build_typed_emission_linker()?;
        let instance = linker.instantiate(&mut store, &self.module).map_err(|e| {
            anyhow!(
                "typed-emission: failed to instantiate plugin `{}`: {}",
                self.name,
                e
            )
        })?;

        // Obtain and fix the heap pointer (same as v1 path).
        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
            anyhow!(
                "typed-emission: plugin `{}` does not export memory",
                self.name
            )
        })?;

        let globals: Vec<_> = instance
            .exports(&mut store)
            .filter_map(|e| e.into_global())
            .collect();
        for global in globals {
            if let wasmtime::Val::I32(val) = global.get(&mut store) {
                if val == 1024 {
                    let _ = global.set(&mut store, wasmtime::Val::I32(8192));
                    break;
                }
            }
        }

        // Write LP-strings into plugin WASM memory.
        let block_name_ptr = self.find_or_write_string(&mut store, &memory, block_name_str)?;
        let attributes_ptr = self.find_or_write_string(&mut store, &memory, attributes_str)?;
        let body_ptr = self.find_or_write_string(&mut store, &memory, body_str)?;

        // Call the plugin's `expand_block_typed(ctx, block_name_lp, attrs_lp, body_lp) -> i32`.
        let expand: TypedFunc<(i32, i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, export_name)
            .map_err(|e| {
                anyhow!(
                    "typed-emission: plugin `{}` does not export `{}`: {}",
                    self.name,
                    export_name,
                    e
                )
            })?;

        let plugin_return = expand
            .call(&mut store, (ctx, block_name_ptr, attributes_ptr, body_ptr))
            .map_err(|e| {
                let oom = store.data().oom_during_call.clone();
                anyhow!(
                    "PLUGIN009: plugin `{}` trapped during typed emission of block `{}`: {}{}",
                    self.name,
                    block.name.trim_end_matches(':'),
                    e,
                    oom.as_deref()
                        .map(|s| format!(" [OOM: {}]", s))
                        .unwrap_or_default(),
                )
            })?;

        // Drain the arena.
        let arena = store.data_mut().emit_arena.take().expect(
            "typed-emission: arena was taken during expansion — cross-call arena reuse detected",
        );

        let (expansion, diagnostics, saw_error) = arena.finish();

        // Route diagnostics to the compiler diagnostic channel.
        for diag in &diagnostics {
            if diag.severity >= 2 {
                tracing::error!(
                    target: "compiler::plugins::typed_emission",
                    plugin = %self.name,
                    block = block.name.trim_end_matches(':'),
                    code = %diag.code,
                    message = %diag.message,
                    "plugin typed-emission error"
                );
            } else {
                tracing::warn!(
                    target: "compiler::plugins::typed_emission",
                    plugin = %self.name,
                    block = block.name.trim_end_matches(':'),
                    code = %diag.code,
                    message = %diag.message,
                    "plugin typed-emission warning"
                );
            }
        }

        if plugin_return != 0 || saw_error {
            let error_summary = diagnostics
                .iter()
                .filter(|d| d.severity >= 2)
                .map(|d| format!("[{}] {}", d.code, d.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(anyhow!(
                "plugin `{}` failed typed emission of block `{}` (return={}, errors: {})",
                self.name,
                block.name.trim_end_matches(':'),
                plugin_return,
                if error_summary.is_empty() {
                    "(none)"
                } else {
                    &error_summary
                }
            ));
        }

        tracing::debug!(
            target: "compiler::plugins::arena",
            plugin = %self.name,
            block = block.name.trim_end_matches(':'),
            functions = expansion.functions.len(),
            classes = expansion.classes.len(),
            externals = expansion.externals.len(),
            "typed-emission: expansion complete"
        );

        Ok(expansion)
    }

    /// Plugin Contracts v3 sub-cycle 3 — typed-emission variant of a lifecycle
    /// slot call. Used by `invoke_lifecycle_slot` when the plugin opts into
    /// typed emission at the manifest level (`expansion_version = "3.0.0"`).
    ///
    /// Signature mirrors `call_expand_typed`: a per-call `EmitArena` is
    /// installed, a fresh `ctx_handle` is allocated, the plugin's typed slot
    /// export is invoked as `(ctx, build_context_lp) -> i32`, diagnostics are
    /// routed, and the accumulated `PluginExpansion` is returned.
    ///
    /// Per typed-emission.md §7: every v1 lifecycle slot has a typed-emission
    /// counterpart with the same call site but a different ABI shape.
    fn call_lifecycle_slot_typed(
        &self,
        slot_name: &str,
        export_name: &str,
        context: &crate::plugins::BuildContext,
    ) -> Result<PluginExpansion> {
        let ctx = self.next_ctx_handle();
        let arena = crate::plugins::typed_emission::EmitArena::new(ctx);
        let mut store = self.create_store_with_arena(arena);

        let linker = self.build_typed_emission_linker()?;
        let instance = linker.instantiate(&mut store, &self.module).map_err(|e| {
            anyhow!(
                "typed-emission: failed to instantiate plugin `{}` for slot `{}`: {}",
                self.name,
                slot_name,
                e
            )
        })?;

        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
            anyhow!(
                "typed-emission: plugin `{}` does not export memory",
                self.name
            )
        })?;

        // Heap pointer fix — same convention as call_expand_typed.
        let globals: Vec<_> = instance
            .exports(&mut store)
            .filter_map(|e| e.into_global())
            .collect();
        for global in globals {
            if let wasmtime::Val::I32(val) = global.get(&mut store) {
                if val == 1024 {
                    let _ = global.set(&mut store, wasmtime::Val::I32(8192));
                    break;
                }
            }
        }

        let context_json = serde_json::to_string(context).map_err(|e| {
            anyhow!(
                "typed-emission: failed to serialize build context for slot `{}`: {}",
                slot_name,
                e
            )
        })?;
        let context_ptr = self.find_or_write_string(&mut store, &memory, &context_json)?;

        // The typed lifecycle slot signature is `(ctx, build_context_lp) -> i32`.
        let slot: TypedFunc<(i32, i32), i32> = instance
            .get_typed_func(&mut store, export_name)
            .map_err(|e| {
                anyhow!(
                    "typed-emission: plugin `{}` does not export typed lifecycle slot \
                     `{}` (export `{}`): {}",
                    self.name,
                    slot_name,
                    export_name,
                    e
                )
            })?;

        let plugin_return = slot.call(&mut store, (ctx, context_ptr)).map_err(|e| {
            let oom = store.data().oom_during_call.clone();
            anyhow!(
                "PLUGIN009: plugin `{}` trapped during typed lifecycle slot `{}`: {}{}",
                self.name,
                slot_name,
                e,
                oom.as_deref()
                    .map(|s| format!(" [OOM: {}]", s))
                    .unwrap_or_default(),
            )
        })?;

        let arena = store
            .data_mut()
            .emit_arena
            .take()
            .expect("typed-emission: arena must be present after slot call");

        let (expansion, diagnostics, saw_error) = arena.finish();

        for diag in &diagnostics {
            if diag.severity >= 2 {
                tracing::error!(
                    target: "compiler::plugins::typed_emission",
                    plugin = %self.name,
                    slot = slot_name,
                    code = %diag.code,
                    message = %diag.message,
                    "plugin typed-emission error (lifecycle slot)"
                );
            } else {
                tracing::warn!(
                    target: "compiler::plugins::typed_emission",
                    plugin = %self.name,
                    slot = slot_name,
                    code = %diag.code,
                    message = %diag.message,
                    "plugin typed-emission warning (lifecycle slot)"
                );
            }
        }

        if plugin_return != 0 || saw_error {
            let error_summary = diagnostics
                .iter()
                .filter(|d| d.severity >= 2)
                .map(|d| format!("[{}] {}", d.code, d.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(anyhow!(
                "plugin `{}` failed typed lifecycle slot `{}` (return={}, errors: {})",
                self.name,
                slot_name,
                plugin_return,
                if error_summary.is_empty() {
                    "(none)"
                } else {
                    &error_summary
                }
            ));
        }

        Ok(expansion)
    }

    /// Register all stdlib host functions that any plugin (v1 or v3) may import.
    ///
    /// Classification of every register_* call:
    ///
    /// - `register_env_core_functions`            SHARED — print, string.concat, float_to_string,
    ///                                            string.split, type conversions; any non-trivial
    ///                                            plugin calls these from Clean source.
    /// - `register_string_dot_functions`          SHARED — string.slice, string.substring,
    ///                                            string.indexOf, string.replace, etc.
    /// - `register_string_underscore_functions`   SHARED — string_compare, string_replaceAll,
    ///                                            string_length, _str_eq; used by generated code.
    /// - `register_list_functions`                SHARED — list.push_f64 and list operations;
    ///                                            plugins that build or traverse lists need these.
    /// - `register_memory_runtime_functions`      SHARED — allocator / memory growth primitives;
    ///                                            required by the WASM module's own alloc helpers.
    /// - `register_http_client_functions`         SHARED — http_get/post/put/patch/delete; a
    ///                                            plugin that issues outbound HTTP calls needs these.
    /// - `register_http_server_functions`         SHARED (stubs) — _http_route/_http_listen;
    ///                                            stubs that return 0, safe to include, prevent
    ///                                            "unknown import" panics if a plugin links them.
    /// - `register_request_context_functions`     SHARED (stubs) — _req_param/_req_query/etc.;
    ///                                            same rationale as http_server: stub-only, safe.
    /// - `register_file_functions`                SHARED — file I/O stubs; included so plugins
    ///                                            that optionally call file ops still instantiate.
    /// - `register_math_functions`                SHARED — Math.sin/cos/sqrt/pow/random/etc.;
    ///                                            used by numeric plugins from Clean source.
    /// - `register_http_auth_stubs`               SHARED (stubs) — _req_cookie/_http_redirect/
    ///                                            session stubs; returns 0, never v1-specific logic.
    /// - `register_build_state_bridges`           SHARED — _build_state_set/_build_state_get;
    ///                                            v2 contract bridges valid in v3 as well (§2.5).
    ///
    /// Nothing in this list is v1-expansion-protocol-specific (no parse_plugin_code, no v1
    /// scope push/pop injected code). Every entry is either a real I/O function or a zero-stub
    /// that prevents "missing import" wasmtime rejections.
    fn register_plugin_stdlib_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // Core I/O and conversions (print, float_to_string, string.concat, etc.)
        self.register_env_core_functions(linker)?;
        // string.* dot-namespace functions (slice, substring, indexOf, replace, …)
        self.register_string_dot_functions(linker)?;
        // string_* underscore-namespace functions (string_compare, _str_eq, …)
        self.register_string_underscore_functions(linker)?;
        // list.* functions (push_f64, …)
        self.register_list_functions(linker)?;
        // Memory / allocator runtime helpers
        self.register_memory_runtime_functions(linker)?;
        // Outbound HTTP client (http_get/post/…)
        self.register_http_client_functions(linker)?;
        // HTTP server stubs (_http_route, _http_listen — zero returns)
        self.register_http_server_functions(linker)?;
        // Request context stubs (_req_param, _req_query, … — zero/empty returns)
        self.register_request_context_functions(linker)?;
        // File I/O stubs
        self.register_file_functions(linker)?;
        // Math namespace (Math.sin, Math.cos, Math.sqrt, Math.random, …)
        self.register_math_functions(linker)?;
        // Auth / session / redirect stubs (_req_cookie, _http_redirect, _session_*, …)
        self.register_http_auth_stubs(linker)?;
        // Build-state bridges (_build_state_set, _build_state_get — Plugin Contracts v2+)
        self.register_build_state_bridges(linker)?;
        Ok(())
    }

    /// Set up the v1 linker with host functions.
    /// Provides the full Clean Language runtime environment for v1 plugins.
    /// Delegates to `register_plugin_stdlib_functions` so the stdlib surface is
    /// always in sync with the v3 linker.
    fn setup_linker(&self) -> Result<Linker<PluginState>> {
        let mut linker = Linker::new(&self.engine);
        self.register_plugin_stdlib_functions(&mut linker)?;
        Ok(linker)
    }

    /// Plugin Contracts v2 — register the `_build_state_set` /
    /// `_build_state_get` bridges that plugins use to communicate state
    /// across calls within one build. See `lifecycle.md` §2.5.
    fn register_build_state_bridges(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        register_build_state_bridges(linker, &self.build_state)
    }

    // =========================================
    // ENV NAMESPACE - Core I/O and conversions
    // =========================================
    fn register_env_core_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // env.print - Print without newline
        linker.func_wrap(
            "env",
            "print",
            |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| {
                if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let data = memory.data(&caller);
                    if let Some(slice) = data.get(ptr as usize..(ptr as usize + len as usize)) {
                        if let Ok(s) = std::str::from_utf8(slice) {
                            eprint!("[Plugin] {}", s);
                        }
                    }
                }
            },
        )?;

        // env.printl - Print with newline
        linker.func_wrap(
            "env",
            "printl",
            |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| {
                if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let data = memory.data(&caller);
                    if let Some(slice) = data.get(ptr as usize..(ptr as usize + len as usize)) {
                        if let Ok(s) = std::str::from_utf8(slice) {
                            eprintln!("[Plugin] {}", s);
                        }
                    }
                }
            },
        )?;

        // env.print_integer - Print integer value (i64) without newline
        linker.func_wrap(
            "env",
            "print_integer",
            |_: Caller<'_, PluginState>, _value: i64| {},
        )?;

        // env.print_float - Print float value without newline
        linker.func_wrap(
            "env",
            "print_float",
            |_: Caller<'_, PluginState>, _value: f64| {},
        )?;

        // env.print_boolean - Print boolean value without newline
        linker.func_wrap(
            "env",
            "print_boolean",
            |_: Caller<'_, PluginState>, _value: i32| {},
        )?;

        // env.input - Read user input (stub - returns empty string)
        linker.func_wrap(
            "env",
            "input",
            |mut caller: Caller<'_, PluginState>, _prompt_ptr: i32| -> i32 {
                // Return empty string - allocate string in memory
                let state = caller.data_mut();
                state.allocate(8) as i32 // Return pointer to empty string area
            },
        )?;

        // env.input_integer - Read integer input (stub - returns 0)
        linker.func_wrap(
            "env",
            "input_integer",
            |_: Caller<'_, PluginState>, _prompt_ptr: i32| -> i32 { 0 },
        )?;

        // env.input_float - Read float input (stub - returns 0.0)
        linker.func_wrap(
            "env",
            "input_float",
            |_: Caller<'_, PluginState>, _prompt_ptr: i32| -> f64 { 0.0 },
        )?;

        // env.input_yesno - Read yes/no input (stub - returns 0/false)
        linker.func_wrap(
            "env",
            "input_yesno",
            |_: Caller<'_, PluginState>, _prompt_ptr: i32| -> i32 { 0 },
        )?;

        // env.input_range - Read range input (stub - returns min value)
        linker.func_wrap(
            "env",
            "input_range",
            |_: Caller<'_, PluginState>,
             _prompt_ptr: i32,
             min: i32,
             _max: i32,
             _default: i32|
             -> i32 { min },
        )?;

        // env.int_to_string - Convert integer to string
        linker.func_wrap(
            "env",
            "int_to_string",
            |mut caller: Caller<'_, PluginState>, value: i32| -> i32 {
                let s = value.to_string();
                let bytes = s.as_bytes();
                let len = bytes.len();

                let state = caller.data_mut();
                let ptr = state.allocate(4 + len);

                if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    // Write length prefix
                    let len_bytes = (len as u32).to_le_bytes();
                    let _ = memory.write(&mut caller, ptr, &len_bytes);
                    // Write string data
                    let _ = memory.write(&mut caller, ptr + 4, bytes);
                }

                ptr as i32
            },
        )?;

        // env.float_to_string - Convert float to string
        linker.func_wrap(
            "env",
            "float_to_string",
            |mut caller: Caller<'_, PluginState>, value: f64| -> i32 {
                let s = value.to_string();
                let bytes = s.as_bytes();
                let len = bytes.len();

                let state = caller.data_mut();
                let ptr = state.allocate(4 + len);

                if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let len_bytes = (len as u32).to_le_bytes();
                    let _ = memory.write(&mut caller, ptr, &len_bytes);
                    let _ = memory.write(&mut caller, ptr + 4, bytes);
                }

                ptr as i32
            },
        )?;

        // env.bool_to_string - Convert boolean to string
        linker.func_wrap(
            "env",
            "bool_to_string",
            |mut caller: Caller<'_, PluginState>, value: i32| -> i32 {
                let s = if value != 0 { "true" } else { "false" };
                let bytes = s.as_bytes();
                let len = bytes.len();

                let state = caller.data_mut();
                let ptr = state.allocate(4 + len);

                if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let len_bytes = (len as u32).to_le_bytes();
                    let _ = memory.write(&mut caller, ptr, &len_bytes);
                    let _ = memory.write(&mut caller, ptr + 4, bytes);
                }

                ptr as i32
            },
        )?;

        // env.string_to_int - Convert string to integer
        linker.func_wrap(
            "env",
            "string_to_int",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                if let Some(s) = read_clean_string(&mut caller, str_ptr) {
                    s.parse::<i32>().unwrap_or(0)
                } else {
                    0
                }
            },
        )?;

        // env.string_to_float - Convert string to float
        linker.func_wrap(
            "env",
            "string_to_float",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> f64 {
                if let Some(s) = read_clean_string(&mut caller, str_ptr) {
                    s.parse::<f64>().unwrap_or(0.0)
                } else {
                    0.0
                }
            },
        )?;

        // FIXED: env.string.concat - Concatenate two length-prefixed strings
        // Each pointer points to: [4-byte little-endian length][content bytes]
        // Returns pointer to new length-prefixed concatenated string
        linker.func_wrap(
            "env",
            "string.concat",
            |mut caller: Caller<'_, PluginState>, str1_ptr: i32, str2_ptr: i32| -> i32 {
                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return 0,
                };

                let data = memory.data(&caller);

                // Read first string (length-prefixed)
                let ptr1 = str1_ptr as usize;
                if ptr1 + 4 > data.len() {
                    return 0;
                }
                let len1 = u32::from_le_bytes([
                    data[ptr1],
                    data[ptr1 + 1],
                    data[ptr1 + 2],
                    data[ptr1 + 3],
                ]) as usize;
                let content1_start = ptr1 + 4;
                let content1_end = content1_start + len1;
                let s1 = if content1_end <= data.len() {
                    data[content1_start..content1_end].to_vec()
                } else {
                    Vec::new()
                };

                // Read second string (length-prefixed)
                let ptr2 = str2_ptr as usize;
                if ptr2 + 4 > data.len() {
                    return 0;
                }
                let len2 = u32::from_le_bytes([
                    data[ptr2],
                    data[ptr2 + 1],
                    data[ptr2 + 2],
                    data[ptr2 + 3],
                ]) as usize;
                let content2_start = ptr2 + 4;
                let content2_end = content2_start + len2;
                let s2 = if content2_end <= data.len() {
                    data[content2_start..content2_end].to_vec()
                } else {
                    Vec::new()
                };

                // Concatenate
                let mut result = s1;
                result.extend(s2);
                let result_len = result.len();
                let total_size = 4 + result_len;

                // Allocate and write result (length-prefixed)
                let state = caller.data_mut();
                let ptr = state.allocate(total_size);
                if ptr == 0 && state.oom_during_call.is_none() {
                    state.oom_during_call = Some(format!(
                        "string.concat: bridge bump allocator exhausted (>= {} MiB \
                         consumed) requesting {} bytes",
                        i32::MAX as usize / (1024 * 1024),
                        total_size,
                    ));
                }

                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                // Check if we need to grow memory
                let current_size = memory.data_size(&caller);
                let required_size = ptr + total_size;
                if required_size > current_size {
                    let pages_needed = (required_size - current_size).div_ceil(65536);
                    if memory.grow(&mut caller, pages_needed as u64).is_err() {
                        let current_mb = current_size / (1024 * 1024);
                        let state = caller.data_mut();
                        if state.oom_during_call.is_none() {
                            state.oom_during_call = Some(format!(
                                "string.concat: memory.grow failed at {} MiB current \
                                 (plugin host-cap 1 GiB), requested {} byte result \
                                 ({} additional pages)",
                                current_mb, result_len, pages_needed,
                            ));
                        }
                        return 0; // Allocation failed
                    }
                }

                // Write length and data
                let len_bytes = (result_len as u32).to_le_bytes();
                if memory.write(&mut caller, ptr, &len_bytes).is_err() {
                    return 0;
                }
                if memory.write(&mut caller, ptr + 4, &result).is_err() {
                    return 0;
                }

                ptr as i32
            },
        )?;

        // env.string.split - Split a string by a delimiter.
        //
        // Parameters:
        //   str_ptr   - pointer to length-prefixed source string
        //   delim_ptr - pointer to length-prefixed delimiter string
        //
        // Returns a pointer to a list structure:
        //   Header (16 bytes): [length: i32, capacity: i32, type_tag: i32, flags: i32]
        //   Data (4 bytes * length): i32 pointers, each pointing to a
        //                            length-prefixed substring in linear memory.
        //
        // Returns 0 on any memory error.
        linker.func_wrap(
            "env",
            "string.split",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32, delim_ptr: i32| -> i32 {
                // Read both strings safely.
                let source = match read_clean_string(&mut caller, str_ptr) {
                    Some(s) => s,
                    None => return 0,
                };
                let delimiter = match read_clean_string(&mut caller, delim_ptr) {
                    Some(s) => s,
                    None => return 0,
                };

                // Perform the split.
                let parts: Vec<&str> = if delimiter.is_empty() {
                    // Empty delimiter: split into individual characters.
                    // Collect into a Vec<&str> by splitting at char boundaries.
                    // We'll use a temporary approach: split on "" returns the
                    // same string, so instead split on every char.
                    source.split("").filter(|s| !s.is_empty()).collect()
                } else {
                    source.split(delimiter.as_str()).collect()
                };

                let count = parts.len();

                // Write each substring and collect their pointers.
                let mut part_ptrs: Vec<i32> = Vec::with_capacity(count);
                for part in &parts {
                    let p = write_clean_string(&mut caller, part.as_bytes());
                    if p == 0 && !part.is_empty() {
                        return 0; // memory allocation failure
                    }
                    part_ptrs.push(p);
                }

                // Build the list header + pointer array.
                // Header: [length(i32), capacity(i32), type_tag(i32), flags(i32)] = 16 bytes
                // Each element is a 4-byte i32 pointer.
                let header_size = 16usize;
                let data_size = count * 4;
                let total = header_size + data_size;

                let list_ptr = caller.data_mut().allocate(total);

                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return 0,
                };

                // Grow if needed.
                let current_size = memory.data_size(&caller);
                let required_size = list_ptr + total;
                if required_size > current_size {
                    let pages_needed = (required_size - current_size).div_ceil(65536);
                    if memory.grow(&mut caller, pages_needed as u64).is_err() {
                        return 0;
                    }
                }

                // Write header: length = count, capacity = count, type_tag = 0, flags = 0
                let count_u32 = count as u32;
                let header_bytes = [
                    count_u32.to_le_bytes(),
                    count_u32.to_le_bytes(),
                    0u32.to_le_bytes(),
                    0u32.to_le_bytes(),
                ]
                .concat();
                if memory.write(&mut caller, list_ptr, &header_bytes).is_err() {
                    return 0;
                }

                // Write each element pointer.
                for (i, &p) in part_ptrs.iter().enumerate() {
                    let offset = list_ptr + header_size + i * 4;
                    if memory
                        .write(&mut caller, offset, &(p as u32).to_le_bytes())
                        .is_err()
                    {
                        return 0;
                    }
                }

                list_ptr as i32
            },
        )?;

        // env.string_trim - Remove leading and trailing whitespace
        // String format: [4-byte length][data]
        linker.func_wrap(
            "env",
            "string_trim",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                // Use the safe read_clean_string helper — returns None on out-of-bounds.
                let original = match read_clean_string(&mut caller, str_ptr) {
                    Some(s) => s,
                    None => return 0,
                };
                let trimmed = original.trim().to_owned();
                write_clean_string(&mut caller, trimmed.as_bytes())
            },
        )?;

        // env.string_trim_start - Remove leading whitespace
        linker.func_wrap(
            "env",
            "string_trim_start",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = match read_clean_string(&mut caller, str_ptr) {
                    Some(s) => s,
                    None => return 0,
                };
                let trimmed = original.trim_start().to_owned();
                write_clean_string(&mut caller, trimmed.as_bytes())
            },
        )?;

        // env.string_trim_end - Remove trailing whitespace
        linker.func_wrap(
            "env",
            "string_trim_end",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = match read_clean_string(&mut caller, str_ptr) {
                    Some(s) => s,
                    None => return 0,
                };
                let trimmed = original.trim_end().to_owned();
                write_clean_string(&mut caller, trimmed.as_bytes())
            },
        )?;

        Ok(())
    }

    // =========================================
    // DOT-NOTATION TRIM ALIASES + string dot-methods
    // For compatibility with WASM modules using dot notation
    // =========================================
    fn register_string_dot_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // =========================================
        // DOT-NOTATION TRIM ALIASES
        // For compatibility with WASM modules using dot notation
        // =========================================

        // env.string.trim - Alias for string_trim (dot notation)
        linker.func_wrap(
            "env",
            "string.trim",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = match read_clean_string(&mut caller, str_ptr) {
                    Some(s) => s,
                    None => return 0,
                };
                let trimmed = original.trim().to_owned();
                write_clean_string(&mut caller, trimmed.as_bytes())
            },
        )?;

        // env.string.trimStart - Alias for string_trim_start (dot notation)
        linker.func_wrap(
            "env",
            "string.trimStart",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = match read_clean_string(&mut caller, str_ptr) {
                    Some(s) => s,
                    None => return 0,
                };
                let trimmed = original.trim_start().to_owned();
                write_clean_string(&mut caller, trimmed.as_bytes())
            },
        )?;

        // env.string.trimEnd - Alias for string_trim_end (dot notation)
        linker.func_wrap(
            "env",
            "string.trimEnd",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = match read_clean_string(&mut caller, str_ptr) {
                    Some(s) => s,
                    None => return 0,
                };
                let trimmed = original.trim_end().to_owned();
                write_clean_string(&mut caller, trimmed.as_bytes())
            },
        )?;

        // env.string_compare - Compare two strings
        // Returns 0 if equal, 1 if not equal (C/strcmp convention; codegen uses i32.eqz to test equality)
        //
        // Bounds-checked via `read_lp_from_data`: when either pointer is
        // unreadable (negative, OOB header, length larger than memory),
        // the bridge reads it as an empty string. This is the natural
        // semantics — an unreadable pointer compares not-equal to any
        // non-empty literal and equal to the empty literal — and crucially
        // **does not panic** the way the previous raw-slicing version did
        // (the COMPILER-PLUGIN-STRING-COMPARE-PANIC-OUT-OF-BOUNDS symptom).
        linker.func_wrap(
            "env",
            "string_compare",
            |mut caller: Caller<'_, PluginState>, ptr1: i32, ptr2: i32| -> i32 {
                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return 1, // not equal — no memory means no comparison possible
                };
                let data = memory.data(&caller);
                let s1 = read_lp_from_data(data, ptr1).unwrap_or(&[]);
                let s2 = read_lp_from_data(data, ptr2).unwrap_or(&[]);
                if s1 == s2 {
                    0
                } else {
                    1
                }
            },
        )?;

        // env.string_replace - Replace all occurrences of a substring
        // string_replace(string_ptr: i32, search_ptr: i32, replace_ptr: i32) -> i32
        linker.func_wrap(
            "env",
            "string_replace",
            |mut caller: Caller<'_, PluginState>,
             string_ptr: i32,
             search_ptr: i32,
             replace_ptr: i32|
             -> i32 {
                // Read all three strings first
                let (string_val, search, replace) = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();
                    let s3 = String::from_utf8_lossy(
                        read_lp_from_data(data, replace_ptr).unwrap_or(&[]),
                    )
                    .to_string();

                    (s1, s2, s3)
                };

                // Perform the replacement
                let result = string_val.replace(&search, &replace);

                // Allocate and write result
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let total_size = result_len + 4;
                let state = caller.data_mut();
                let ptr = state.allocate(total_size);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                // Check if we need to grow memory
                let current_size = memory.data_size(&caller);
                let required_size = ptr + total_size;
                if required_size > current_size {
                    let pages_needed = (required_size - current_size).div_ceil(65536);
                    if memory.grow(&mut caller, pages_needed as u64).is_err() {
                        return 0;
                    }
                }

                let len_bytes = (result_len as u32).to_le_bytes();
                if memory.write(&mut caller, ptr, &len_bytes).is_err() {
                    return 0;
                }
                if memory.write(&mut caller, ptr + 4, result_bytes).is_err() {
                    return 0;
                }
                ptr as i32
            },
        )?;

        // env.string_repeat - Repeat a string N times
        // string_repeat(str_ptr: i32, str_len: i32, count: i32) -> i32
        linker.func_wrap(
            "env",
            "string_repeat",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32, _str_len: i32, count: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let len_start = str_ptr as usize;
                    if len_start + 4 > data.len() {
                        return 0;
                    }
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    if data_start + len > data.len() {
                        return 0;
                    }
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
                };

                let repeat_count = count.max(0) as usize;
                let result = string_val.repeat(repeat_count);
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let total_size = result_len + 4;
                let state = caller.data_mut();
                let ptr = state.allocate(total_size);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                let current_size = memory.data_size(&caller);
                let required_size = ptr + total_size;
                if required_size > current_size {
                    let pages_needed = (required_size - current_size).div_ceil(65536);
                    if memory.grow(&mut caller, pages_needed as u64).is_err() {
                        return 0;
                    }
                }

                let len_bytes = (result_len as u32).to_le_bytes();
                if memory.write(&mut caller, ptr, &len_bytes).is_err() {
                    return 0;
                }
                if memory.write(&mut caller, ptr + 4, result_bytes).is_err() {
                    return 0;
                }
                ptr as i32
            },
        )?;

        // env.string_matches - Test whether a string matches a pattern (regex)
        // string_matches(str_ptr: i32, str_len: i32, pattern_id: i32) -> i32
        // pattern_id is a compile-time constant: email=0 url=1 uuid=2 phone=3 date=4
        //   integer=5 number=6 alphanumeric=7
        linker.func_wrap(
            "env",
            "string_matches",
            |mut caller: Caller<'_, PluginState>,
             str_ptr: i32,
             _str_len: i32,
             pattern_id: i32|
             -> i32 {
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let data = memory.data(&caller).to_vec();
                let ptr = str_ptr as usize;
                if ptr + 4 > data.len() {
                    return 0;
                }
                let len = u32::from_le_bytes(data[ptr..ptr + 4].try_into().unwrap()) as usize;
                let s = if ptr + 4 + len <= data.len() {
                    String::from_utf8_lossy(&data[ptr + 4..ptr + 4 + len]).to_string()
                } else {
                    return 0;
                };
                let matched = match pattern_id {
                    0 => {
                        let p: Vec<&str> = s.splitn(2, '@').collect();
                        p.len() == 2 && !p[0].is_empty() && p[1].contains('.')
                    }
                    1 => s.starts_with("http://") || s.starts_with("https://"),
                    2 => {
                        let b = s.as_bytes();
                        b.len() == 36
                            && b[8] == b'-'
                            && b[13] == b'-'
                            && b[18] == b'-'
                            && b[23] == b'-'
                            && b.iter().enumerate().all(|(i, &c)| {
                                if i == 8 || i == 13 || i == 18 || i == 23 {
                                    c == b'-'
                                } else {
                                    c.is_ascii_hexdigit()
                                }
                            })
                    }
                    3 => {
                        let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
                        d.len() >= 7 && d.len() <= 15
                    }
                    4 => {
                        let p: Vec<&str> = s.splitn(3, '-').collect();
                        p.len() == 3
                            && p[0].len() == 4
                            && p[1].len() == 2
                            && p[2].len() == 2
                            && p.iter().all(|x| x.chars().all(|c| c.is_ascii_digit()))
                    }
                    5 => !s.is_empty() && s.parse::<i64>().is_ok(),
                    6 => !s.is_empty() && s.parse::<f64>().is_ok(),
                    7 => !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()),
                    _ => false,
                };
                if matched {
                    1
                } else {
                    0
                }
            },
        )?;

        // Endpoint test bridge stubs (plugin sandbox has no live server)
        linker.func_wrap(
            "env",
            "_test_http_request",
            |_: i32,
             _: i32,
             _: i32,
             _: i32,
             _: i32,
             _: i32,
             _: i32,
             _: i32,
             _: i32,
             _: i32|
             -> i32 { 0 },
        )?;
        linker.func_wrap(
            "env",
            "_test_response_status",
            |_caller: Caller<'_, PluginState>, _: i32| -> i32 { 0 },
        )?;
        linker.func_wrap(
            "env",
            "_test_response_body",
            |_caller: Caller<'_, PluginState>, _: i32| -> i32 { 0 },
        )?;

        // env.string_from_char_code - Create string from character code
        // string_from_char_code(char_code: i32) -> i32
        linker.func_wrap(
            "env",
            "string_from_char_code",
            |mut caller: Caller<'_, PluginState>, char_code: i32| -> i32 {
                // Create a single-character string from the char code
                let ch = if (0..=127).contains(&char_code) {
                    char::from_u32(char_code as u32).unwrap_or('\0')
                } else {
                    '\0'
                };
                let result = ch.to_string();

                // Allocate and write result
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, ptr, &len_bytes);
                let _ = memory.write(&mut caller, ptr + 4, result_bytes);
                ptr as i32
            },
        )?;

        // env.string.substring - Extract a substring from a string
        // CRITICAL: This was the missing function causing string corruption
        // string.substring(ptr: i32, start: i32, end: i32) -> i32
        linker.func_wrap(
            "env",
            "string.substring",
            |mut caller: Caller<'_, PluginState>, ptr: i32, start: i32, end: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let len_start = ptr as usize;
                    if len_start + 4 > data.len() {
                        return 0;
                    }
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    if data_start + len > data.len() {
                        return 0;
                    }
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
                };

                let str_len = string_val.len() as i32;
                let actual_start = start.max(0).min(str_len) as usize;
                let actual_end = end.max(0).min(str_len) as usize;
                let actual_end = actual_end.max(actual_start);

                let substring = &string_val[actual_start..actual_end];

                // Allocate and write result
                let result_bytes = substring.as_bytes();
                let result_len = result_bytes.len();
                let total_size = result_len + 4;
                let state = caller.data_mut();
                let result_ptr = state.allocate(total_size);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                // Check if we need to grow memory
                let current_size = memory.data_size(&caller);
                let required_size = result_ptr + total_size;
                if required_size > current_size {
                    let pages_needed = (required_size - current_size).div_ceil(65536);
                    if memory.grow(&mut caller, pages_needed as u64).is_err() {
                        return 0;
                    }
                }

                let len_bytes = (result_len as u32).to_le_bytes();
                if memory.write(&mut caller, result_ptr, &len_bytes).is_err() {
                    return 0;
                }
                if memory
                    .write(&mut caller, result_ptr + 4, result_bytes)
                    .is_err()
                {
                    return 0;
                }
                result_ptr as i32
            },
        )?;

        // env.string.length - Get string length
        linker.func_wrap(
            "env",
            "string.length",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> i32 {
                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return 0,
                };
                let data = memory.data(&caller);
                let len_start = ptr as usize;
                if len_start + 4 > data.len() {
                    return 0;
                }
                let len_bytes: [u8; 4] = match data[len_start..len_start + 4].try_into() {
                    Ok(b) => b,
                    Err(_) => return 0,
                };
                u32::from_le_bytes(len_bytes) as i32
            },
        )?;

        // env.string.indexOf - Find the first occurrence of a substring
        linker.func_wrap(
            "env",
            "string.indexOf",
            |mut caller: Caller<'_, PluginState>, string_ptr: i32, search_ptr: i32| -> i32 {
                let (string_val, search) = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };

                match string_val.find(&search) {
                    Some(idx) => idx as i32,
                    None => -1,
                }
            },
        )?;

        // env.string.lastIndexOf - Find the last occurrence of a substring
        linker.func_wrap(
            "env",
            "string.lastIndexOf",
            |mut caller: Caller<'_, PluginState>, string_ptr: i32, search_ptr: i32| -> i32 {
                let (string_val, search) = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };

                match string_val.rfind(&search) {
                    Some(idx) => idx as i32,
                    None => -1,
                }
            },
        )?;

        // env.string.charAt - Get character at index
        linker.func_wrap(
            "env",
            "string.charAt",
            |mut caller: Caller<'_, PluginState>, ptr: i32, index: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = if index >= 0 && (index as usize) < string_val.len() {
                    string_val
                        .chars()
                        .nth(index as usize)
                        .unwrap_or('\0')
                        .to_string()
                } else {
                    String::new()
                };

                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        // env.string.charCodeAt - Get character code at index
        linker.func_wrap(
            "env",
            "string.charCodeAt",
            |mut caller: Caller<'_, PluginState>, ptr: i32, index: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                if index >= 0 && (index as usize) < string_val.len() {
                    string_val.chars().nth(index as usize).unwrap_or('\0') as i32
                } else {
                    -1
                }
            },
        )?;

        // env.string.startsWith - Check if string starts with prefix
        linker.func_wrap(
            "env",
            "string.startsWith",
            |mut caller: Caller<'_, PluginState>, string_ptr: i32, prefix_ptr: i32| -> i32 {
                let (string_val, prefix) = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, prefix_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };

                if string_val.starts_with(&prefix) {
                    1
                } else {
                    0
                }
            },
        )?;

        // env.string.endsWith - Check if string ends with suffix
        linker.func_wrap(
            "env",
            "string.endsWith",
            |mut caller: Caller<'_, PluginState>, string_ptr: i32, suffix_ptr: i32| -> i32 {
                let (string_val, suffix) = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, suffix_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };

                if string_val.ends_with(&suffix) {
                    1
                } else {
                    0
                }
            },
        )?;

        // env.string.includes - Check if string contains substring
        linker.func_wrap(
            "env",
            "string.includes",
            |mut caller: Caller<'_, PluginState>, string_ptr: i32, search_ptr: i32| -> i32 {
                let (string_val, search) = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };

                if string_val.contains(&search) {
                    1
                } else {
                    0
                }
            },
        )?;

        // env.string.toUpperCase - Convert to uppercase
        linker.func_wrap(
            "env",
            "string.toUpperCase",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = string_val.to_uppercase();
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        // env.string.toLowerCase - Convert to lowercase
        linker.func_wrap(
            "env",
            "string.toLowerCase",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = string_val.to_lowercase();
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        // env.string.replaceFirst - Replace first occurrence of substring
        linker.func_wrap(
            "env",
            "string.replaceFirst",
            |mut caller: Caller<'_, PluginState>,
             string_ptr: i32,
             search_ptr: i32,
             replace_ptr: i32|
             -> i32 {
                let (string_val, search, replace) = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();
                    let s3 = String::from_utf8_lossy(
                        read_lp_from_data(data, replace_ptr).unwrap_or(&[]),
                    )
                    .to_string();

                    (s1, s2, s3)
                };

                let result = string_val.replacen(&search, &replace, 1);
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        // env.string.repeat - Repeat string n times
        //
        // Registry signature: `string_repeat(str_ptr: i32, str_len: i32, count: i32)`.
        // `str_len` is documented as "raw length (ignored)" — the host reads
        // the length from the 4-byte prefix at `str_ptr`. Kept in the
        // signature to match the registry so host conformance checks pass.
        linker.func_wrap(
            "env",
            "string.repeat",
            |mut caller: Caller<'_, PluginState>, ptr: i32, _str_len: i32, count: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let count = count.max(0) as usize;
                let result = string_val.repeat(count);
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        // env.string.reverse - Reverse the string
        linker.func_wrap(
            "env",
            "string.reverse",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result: String = string_val.chars().rev().collect();
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        Ok(())
    }

    // =========================================
    // UNDERSCORE-STYLE STRING ALIASES
    // For compatibility with different WASM naming conventions
    // =========================================
    fn register_string_underscore_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // env.string_substring - Alias for string.substring
        linker.func_wrap(
            "env",
            "string_substring",
            |mut caller: Caller<'_, PluginState>, ptr: i32, start: i32, end: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let len_start = ptr as usize;
                    if len_start + 4 > data.len() {
                        return 0;
                    }
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    if data_start + len > data.len() {
                        return 0;
                    }
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
                };

                let str_len = string_val.len() as i32;
                let actual_start = start.max(0).min(str_len) as usize;
                let actual_end = end.max(0).min(str_len) as usize;
                let actual_end = actual_end.max(actual_start);

                let substring = &string_val[actual_start..actual_end];

                let result_bytes = substring.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        // env.string_length - Alias for string.length
        linker.func_wrap(
            "env",
            "string_length",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> i32 {
                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return 0,
                };
                let data = memory.data(&caller);
                let len_start = ptr as usize;
                if len_start + 4 > data.len() {
                    return 0;
                }
                let len_bytes: [u8; 4] = match data[len_start..len_start + 4].try_into() {
                    Ok(b) => b,
                    Err(_) => return 0,
                };
                u32::from_le_bytes(len_bytes) as i32
            },
        )?;

        // env.string_indexOf - Alias for string.indexOf
        linker.func_wrap(
            "env",
            "string_indexOf",
            |mut caller: Caller<'_, PluginState>, string_ptr: i32, search_ptr: i32| -> i32 {
                let (string_val, search) = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };

                match string_val.find(&search) {
                    Some(idx) => idx as i32,
                    None => -1,
                }
            },
        )?;

        // env.string_charAt - Alias for string.charAt
        linker.func_wrap(
            "env",
            "string_charAt",
            |mut caller: Caller<'_, PluginState>, ptr: i32, index: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = if index >= 0 && (index as usize) < string_val.len() {
                    string_val
                        .chars()
                        .nth(index as usize)
                        .unwrap_or('\0')
                        .to_string()
                } else {
                    String::new()
                };

                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        // env.string_charCodeAt - Alias for string.charCodeAt
        linker.func_wrap(
            "env",
            "string_charCodeAt",
            |mut caller: Caller<'_, PluginState>, ptr: i32, index: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                if index >= 0 && (index as usize) < string_val.len() {
                    string_val.chars().nth(index as usize).unwrap_or('\0') as i32
                } else {
                    -1
                }
            },
        )?;

        // env.string_toUpperCase - Alias for string.toUpperCase
        linker.func_wrap(
            "env",
            "string_toUpperCase",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = string_val.to_uppercase();
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        // env.string_toLowerCase - Alias for string.toLowerCase
        linker.func_wrap(
            "env",
            "string_toLowerCase",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = string_val.to_lowercase();
                let result_bytes = result.as_bytes();
                let result_len = result_bytes.len();
                let state = caller.data_mut();
                let result_ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, result_ptr, &len_bytes);
                let _ = memory.write(&mut caller, result_ptr + 4, result_bytes);
                result_ptr as i32
            },
        )?;

        Ok(())
    }

    // =========================================
    // LIST NAMESPACE - List/array operations
    // =========================================
    fn register_list_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // env.list.push_f64 - Push an f64 element to a list
        // List header: [length: i32, capacity: i32, type_tag: i32, flags: i32] (16 bytes)
        // Data starts at offset 16, each f64 element is 8 bytes
        linker.func_wrap(
            "env",
            "list.push_f64",
            |mut caller: Caller<'_, PluginState>, array_ptr: i32, value: f64| -> i32 {
                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => {
                        eprintln!("[list.push_f64: no memory export]");
                        return array_ptr;
                    }
                };

                let data = memory.data(&caller);
                let ptr = array_ptr as usize;

                if ptr + 16 > data.len() {
                    eprintln!("[list.push_f64: invalid pointer {}]", array_ptr);
                    return array_ptr;
                }

                // Read length from offset 0
                let length =
                    u32::from_le_bytes([data[ptr], data[ptr + 1], data[ptr + 2], data[ptr + 3]])
                        as usize;

                // Data starts at offset 16, each f64 element is 8 bytes
                let element_offset = ptr + 16 + length * 8;

                if element_offset + 8 > data.len() {
                    eprintln!("[list.push_f64: out of memory bounds]");
                    return array_ptr;
                }

                // Write the f64 value
                let data_mut = memory.data_mut(&mut caller);
                let bytes = value.to_le_bytes();
                data_mut[element_offset..element_offset + 8].copy_from_slice(&bytes);

                // Increment length
                let new_length = (length + 1) as u32;
                data_mut[ptr..ptr + 4].copy_from_slice(&new_length.to_le_bytes());

                array_ptr
            },
        )?;

        Ok(())
    }

    // =========================================
    // MEMORY_RUNTIME NAMESPACE - Memory management
    // =========================================
    fn register_memory_runtime_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // memory_runtime.mem_alloc(type_id: i32, size: i32) -> i32
        //
        // Signature per foundation/spec bridge contract and the compiler
        // emission in src/codegen/mir_codegen/instructions.rs (Alloca lowering):
        // the first WASM arg is the type tag (for telemetry, hosts may ignore)
        // and the second is the byte count to allocate.
        //
        // The host must:
        //   1. bump the shared allocator by `size`,
        //   2. grow linear memory if the returned range exceeds current
        //      memory size — otherwise writes by the plugin to the returned
        //      pointer trap once cumulative allocations cross the initial
        //      memory boundary (32 pages / 2 MB).
        // Returns 0 on any failure.
        linker.func_wrap(
            "memory_runtime",
            "mem_alloc",
            |mut caller: Caller<'_, PluginState>, _type_id: i32, size: i32| -> i32 {
                if size <= 0 {
                    return 0;
                }
                let size = size as usize;

                let state = caller.data_mut();
                let ptr = state.allocate(size);
                if ptr == 0 && state.oom_during_call.is_none() {
                    state.oom_during_call = Some(format!(
                        "mem_alloc: bridge bump allocator exhausted (>= {} MiB \
                         consumed) requesting {} bytes",
                        i32::MAX as usize / (1024 * 1024),
                        size,
                    ));
                }

                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return 0,
                };

                let current_size = memory.data_size(&caller);
                let required_size = ptr + size;
                if required_size > current_size {
                    let pages_needed = (required_size - current_size).div_ceil(65536);
                    if memory.grow(&mut caller, pages_needed as u64).is_err() {
                        let current_mb = current_size / (1024 * 1024);
                        let state = caller.data_mut();
                        if state.oom_during_call.is_none() {
                            state.oom_during_call = Some(format!(
                                "mem_alloc: memory.grow failed at {} MiB current \
                                 (plugin host-cap 1 GiB), requested {} more bytes \
                                 ({} additional pages)",
                                current_mb, size, pages_needed,
                            ));
                        }
                        return 0;
                    }
                }

                ptr as i32
            },
        )?;

        // memory_runtime.mem_retain - Increment reference count (no-op in this simple impl)
        linker.func_wrap(
            "memory_runtime",
            "mem_retain",
            |_: Caller<'_, PluginState>, _ptr: i32| {},
        )?;

        // memory_runtime.mem_release - Decrement reference count (no-op in this simple impl)
        linker.func_wrap(
            "memory_runtime",
            "mem_release",
            |_: Caller<'_, PluginState>, _ptr: i32| {},
        )?;

        // memory_runtime.mem_scope_push - Push scope mark for arena allocation (no-op in this impl)
        linker.func_wrap(
            "memory_runtime",
            "mem_scope_push",
            |_: Caller<'_, PluginState>| {},
        )?;

        // memory_runtime.mem_scope_pop - Pop scope mark for arena allocation (no-op in this impl)
        linker.func_wrap(
            "memory_runtime",
            "mem_scope_pop",
            |_: Caller<'_, PluginState>| {},
        )?;

        // env._arena_scope_push — push a save mark onto the host arena stack.
        //
        // Returns the new depth (always >= 1) as an opaque handle. The plugin
        // must pass this handle to the paired `_arena_scope_pop` call.
        //
        // Called by HIR-rewritten loops to bracket per-iteration scratch
        // allocations. Every allocation made between a push and the
        // corresponding pop is reclaimed in O(1) by pop.
        linker.func_wrap(
            "env",
            "_arena_scope_push",
            |mut caller: Caller<'_, PluginState>| -> i32 {
                let state = caller.data_mut();
                // Push current alloc_offset onto the mark stack.
                let _mark = state.arena_mark();
                let depth = state.arena_marks.len() as i32;
                // Trace-level: high-volume in hot loops, gated by trace filter.
                // The framework session needs this to confirm the bridges
                // actually fire inside the loop (vs. just being emitted as
                // WASM imports that never get called).
                tracing::trace!(
                    target: "compiler::plugins::arena",
                    event = "scope_push",
                    handle = depth,
                    alloc_offset_at_push = state.alloc_offset,
                    "arena scope push",
                );
                depth
            },
        )?;

        // env._arena_scope_pop — pop the host arena stack to `handle`, reclaiming
        // all allocations made after the corresponding push.
        //
        // `handle` is the value returned by the matching `_arena_scope_push`.
        // Passing `handle == 0` is a no-op (defensive: allows generated code
        // to tolerate early-return paths that never executed the push).
        linker.func_wrap(
            "env",
            "_arena_scope_pop",
            |mut caller: Caller<'_, PluginState>, handle: i32| {
                if handle <= 0 {
                    return;
                }
                let target_depth = (handle as usize).saturating_sub(1);
                let state = caller.data_mut();
                let alloc_offset_before = state.alloc_offset;
                let depth_before = state.arena_marks.len();
                // Pop marks until we reach the depth at the time of the push.
                // In the happy path exactly one mark is popped. The loop
                // handles mis-matched push/pop gracefully (e.g. plugin trap
                // mid-scope) by truncating.
                while state.arena_marks.len() > target_depth {
                    if let Some(saved_offset) = state.arena_marks.pop() {
                        // Use arena_reset only when the saved offset is a valid
                        // reset point (above stable zone, not above current top).
                        if saved_offset >= state.stable_zone_end
                            && saved_offset <= state.alloc_offset
                        {
                            state.arena_reset(ArenaMark(saved_offset));
                        }
                    }
                }
                // Trace-level: emit reclaim trajectory so Bug A's diagnosis
                // can see whether mark/reset actually shrinks the arena.
                tracing::trace!(
                    target: "compiler::plugins::arena",
                    event = "scope_pop",
                    handle = handle,
                    alloc_offset_before = alloc_offset_before,
                    alloc_offset_after = state.alloc_offset,
                    bytes_reclaimed = alloc_offset_before.saturating_sub(state.alloc_offset),
                    depth_before = depth_before,
                    depth_after = state.arena_marks.len(),
                    "arena scope pop",
                );
            },
        )?;

        Ok(())
    }

    // =========================================
    // HTTP NAMESPACE - Network operations (stubs)
    // =========================================
    fn register_http_client_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // env.http_get
        linker.func_wrap(
            "env",
            "http_get",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                // Return empty response
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_post
        linker.func_wrap(
            "env",
            "http_post",
            |mut caller: Caller<'_, PluginState>,
             _url_ptr: i32,
             _url_len: i32,
             _body_ptr: i32,
             _body_len: i32|
             -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_put
        linker.func_wrap(
            "env",
            "http_put",
            |mut caller: Caller<'_, PluginState>,
             _url_ptr: i32,
             _url_len: i32,
             _body_ptr: i32,
             _body_len: i32|
             -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_patch
        linker.func_wrap(
            "env",
            "http_patch",
            |mut caller: Caller<'_, PluginState>,
             _url_ptr: i32,
             _url_len: i32,
             _body_ptr: i32,
             _body_len: i32|
             -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_delete
        linker.func_wrap(
            "env",
            "http_delete",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_head
        linker.func_wrap(
            "env",
            "http_head",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_options
        linker.func_wrap(
            "env",
            "http_options",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_get_with_headers
        linker.func_wrap(
            "env",
            "http_get_with_headers",
            |mut caller: Caller<'_, PluginState>,
             _url_ptr: i32,
             _url_len: i32,
             _headers_ptr: i32,
             _headers_len: i32|
             -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_post_with_headers
        linker.func_wrap(
            "env",
            "http_post_with_headers",
            |mut caller: Caller<'_, PluginState>,
             _url_ptr: i32,
             _url_len: i32,
             _body_ptr: i32,
             _body_len: i32,
             _headers_ptr: i32,
             _headers_len: i32|
             -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_post_json
        linker.func_wrap(
            "env",
            "http_post_json",
            |mut caller: Caller<'_, PluginState>,
             _url_ptr: i32,
             _url_len: i32,
             _json_ptr: i32,
             _json_len: i32|
             -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_put_json
        linker.func_wrap(
            "env",
            "http_put_json",
            |mut caller: Caller<'_, PluginState>,
             _url_ptr: i32,
             _url_len: i32,
             _json_ptr: i32,
             _json_len: i32|
             -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_patch_json
        linker.func_wrap(
            "env",
            "http_patch_json",
            |mut caller: Caller<'_, PluginState>,
             _url_ptr: i32,
             _url_len: i32,
             _json_ptr: i32,
             _json_len: i32|
             -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_post_form
        linker.func_wrap(
            "env",
            "http_post_form",
            |mut caller: Caller<'_, PluginState>,
             _url_ptr: i32,
             _url_len: i32,
             _form_ptr: i32,
             _form_len: i32|
             -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.http_set_user_agent
        linker.func_wrap(
            "env",
            "http_set_user_agent",
            |_: Caller<'_, PluginState>, _ua_ptr: i32, _ua_len: i32| {},
        )?;

        // env.http_set_timeout
        linker.func_wrap(
            "env",
            "http_set_timeout",
            |_: Caller<'_, PluginState>, _timeout_ms: i32| {},
        )?;

        // env.http_set_max_redirects
        linker.func_wrap(
            "env",
            "http_set_max_redirects",
            |_: Caller<'_, PluginState>, _max: i32| {},
        )?;

        // env.http_enable_cookies
        linker.func_wrap(
            "env",
            "http_enable_cookies",
            |_: Caller<'_, PluginState>, _enable: i32| {},
        )?;

        // env.http_get_response_code
        linker.func_wrap(
            "env",
            "http_get_response_code",
            |_: Caller<'_, PluginState>| -> i32 { 0 },
        )?;

        // env.http_get_response_headers
        linker.func_wrap(
            "env",
            "http_get_response_headers",
            |_: Caller<'_, PluginState>| -> i32 { 0 },
        )?;

        // env.http_encode_url
        linker.func_wrap(
            "env",
            "http_encode_url",
            |_caller: Caller<'_, PluginState>, url_ptr: i32, _url_len: i32| -> i32 {
                // Return input unchanged for now
                url_ptr
            },
        )?;

        // env.http_decode_url
        linker.func_wrap(
            "env",
            "http_decode_url",
            |_: Caller<'_, PluginState>, url_ptr: i32, _url_len: i32| -> i32 { url_ptr },
        )?;

        // env.http_build_query
        linker.func_wrap(
            "env",
            "http_build_query",
            |mut caller: Caller<'_, PluginState>, _params_ptr: i32, _params_len: i32| -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        Ok(())
    }

    // =========================================
    // HTTP SERVER NAMESPACE - Server functions (stubs)
    // These are for Frame runtime, stubbed for plugin execution
    // =========================================
    fn register_http_server_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // env._http_route - Register route handler (stub)
        linker.func_wrap(
            "env",
            "_http_route",
            |_: Caller<'_, PluginState>,
             _method_ptr: i32,
             _method_len: i32,
             _path_ptr: i32,
             _path_len: i32,
             _handler_idx: i32|
             -> i32 {
                0 // Success (stub - plugins don't actually register routes)
            },
        )?;

        // env._http_listen - Start HTTP server (stub)
        linker.func_wrap(
            "env",
            "_http_listen",
            |_: Caller<'_, PluginState>, _port: i32| -> i32 {
                0 // Success (stub - plugins don't actually start servers)
            },
        )?;

        Ok(())
    }

    // =========================================
    // REQUEST CONTEXT ACCESS - Stubs for plugins
    // =========================================
    fn register_request_context_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // env._req_param - Get path parameter (stub)
        linker.func_wrap(
            "env",
            "_req_param",
            |mut caller: Caller<'_, PluginState>, _name_ptr: i32, _name_len: i32| -> i32 {
                // Return empty string (stub)
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env._req_query - Get query parameter (stub)
        linker.func_wrap(
            "env",
            "_req_query",
            |mut caller: Caller<'_, PluginState>, _name_ptr: i32, _name_len: i32| -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env._req_body - Get request body (stub)
        linker.func_wrap(
            "env",
            "_req_body",
            |mut caller: Caller<'_, PluginState>| -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env._req_header - Get request header (stub)
        linker.func_wrap(
            "env",
            "_req_header",
            |mut caller: Caller<'_, PluginState>, _name_ptr: i32, _name_len: i32| -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env._req_method - Get request method (stub)
        linker.func_wrap(
            "env",
            "_req_method",
            |mut caller: Caller<'_, PluginState>| -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env._req_path - Get request path (stub)
        linker.func_wrap(
            "env",
            "_req_path",
            |mut caller: Caller<'_, PluginState>| -> i32 {
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        Ok(())
    }

    // =========================================
    // FILE NAMESPACE - File operations (stubs)
    // =========================================
    fn register_file_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // env.file_write
        linker.func_wrap(
            "env",
            "file_write",
            |_: Caller<'_, PluginState>,
             _path_ptr: i32,
             _path_len: i32,
             _content_ptr: i32,
             _content_len: i32|
             -> i32 {
                0 // Success
            },
        )?;

        // env.file_read
        linker.func_wrap(
            "env",
            "file_read",
            |mut caller: Caller<'_, PluginState>,
             _path_ptr: i32,
             _path_len: i32,
             _buf_ptr: i32|
             -> i32 {
                // Return empty content
                let state = caller.data_mut();
                state.allocate(4) as i32
            },
        )?;

        // env.file_exists
        linker.func_wrap(
            "env",
            "file_exists",
            |_: Caller<'_, PluginState>, _path_ptr: i32, _path_len: i32| -> i32 {
                0 // File does not exist
            },
        )?;

        // env.file_delete
        linker.func_wrap(
            "env",
            "file_delete",
            |_: Caller<'_, PluginState>, _path_ptr: i32, _path_len: i32| -> i32 {
                0 // Success
            },
        )?;

        // env.file_append
        linker.func_wrap(
            "env",
            "file_append",
            |_: Caller<'_, PluginState>,
             _path_ptr: i32,
             _path_len: i32,
             _content_ptr: i32,
             _content_len: i32|
             -> i32 {
                0 // Success
            },
        )?;

        Ok(())
    }

    // =========================================
    // MATH NAMESPACE - Math operations
    // Required by compiled WASM modules for stdlib functions
    // =========================================
    fn register_math_functions(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // math_pow - Power function (base^exp)
        linker.func_wrap(
            "env",
            "math_pow",
            |_: Caller<'_, PluginState>, base: f64, exp: f64| -> f64 { base.powf(exp) },
        )?;

        // math_sin - Sine
        linker.func_wrap(
            "env",
            "math_sin",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.sin() },
        )?;

        // math_cos - Cosine
        linker.func_wrap(
            "env",
            "math_cos",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.cos() },
        )?;

        // math_tan - Tangent
        linker.func_wrap(
            "env",
            "math_tan",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.tan() },
        )?;

        // math_asin - Arc sine
        linker.func_wrap(
            "env",
            "math_asin",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.asin() },
        )?;

        // math_acos - Arc cosine
        linker.func_wrap(
            "env",
            "math_acos",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.acos() },
        )?;

        // math_atan - Arc tangent
        linker.func_wrap(
            "env",
            "math_atan",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.atan() },
        )?;

        // math_atan2 - Arc tangent of y/x
        linker.func_wrap(
            "env",
            "math_atan2",
            |_: Caller<'_, PluginState>, y: f64, x: f64| -> f64 { y.atan2(x) },
        )?;

        // math_sinh - Hyperbolic sine
        linker.func_wrap(
            "env",
            "math_sinh",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.sinh() },
        )?;

        // math_cosh - Hyperbolic cosine
        linker.func_wrap(
            "env",
            "math_cosh",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.cosh() },
        )?;

        // math_tanh - Hyperbolic tangent
        linker.func_wrap(
            "env",
            "math_tanh",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.tanh() },
        )?;

        // math_ln - Natural logarithm
        linker.func_wrap(
            "env",
            "math_ln",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.ln() },
        )?;

        // math_log10 - Base-10 logarithm
        linker.func_wrap(
            "env",
            "math_log10",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.log10() },
        )?;

        // math_log2 - Base-2 logarithm
        linker.func_wrap(
            "env",
            "math_log2",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.log2() },
        )?;

        // math_exp - Exponential (e^x)
        linker.func_wrap(
            "env",
            "math_exp",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.exp() },
        )?;

        // math_exp2 - 2^x
        linker.func_wrap(
            "env",
            "math_exp2",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.exp2() },
        )?;

        // math_floor - Floor
        linker.func_wrap(
            "env",
            "math_floor",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.floor() },
        )?;

        // math_ceil - Ceiling
        linker.func_wrap(
            "env",
            "math_ceil",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.ceil() },
        )?;

        // math_round - Round
        linker.func_wrap(
            "env",
            "math_round",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.round() },
        )?;

        // math_abs - Absolute value
        linker.func_wrap(
            "env",
            "math_abs",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.abs() },
        )?;

        // math_sqrt - Square root
        linker.func_wrap(
            "env",
            "math_sqrt",
            |_: Caller<'_, PluginState>, x: f64| -> f64 { x.sqrt() },
        )?;

        // math_min - Minimum of two values
        linker.func_wrap(
            "env",
            "math_min",
            |_: Caller<'_, PluginState>, a: f64, b: f64| -> f64 { a.min(b) },
        )?;

        // math_max - Maximum of two values
        linker.func_wrap(
            "env",
            "math_max",
            |_: Caller<'_, PluginState>, a: f64, b: f64| -> f64 { a.max(b) },
        )?;

        Ok(())
    }

    // =========================================
    // ADDITIONAL HTTP/AUTH STUBS - These are functions that plugins generate
    // as OUTPUT but don't actually call. Only add ones not already defined above.
    // =========================================
    fn register_http_auth_stubs(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // Additional HTTP request stubs (not defined above)
        linker.func_wrap(
            "env",
            "_req_cookie",
            |mut caller: Caller<'_, PluginState>, _: i32, _: i32| -> i32 {
                caller.data_mut().allocate(4) as i32
            },
        )?;
        linker.func_wrap("env", "_req_form", |_: Caller<'_, PluginState>| -> i32 {
            0
        })?;
        linker.func_wrap("env", "_req_ip", |_: Caller<'_, PluginState>| -> i32 { 0 })?;
        linker.func_wrap("env", "_req_headers", |_: Caller<'_, PluginState>| -> i32 {
            0
        })?;

        // Additional HTTP routing stubs (not defined above)
        linker.func_wrap(
            "env",
            "_http_route_protected",
            |_: Caller<'_, PluginState>,
             _: i32,
             _: i32,
             _: i32,
             _: i32,
             _: i32,
             _: i32,
             _: i32|
             -> i32 { 0 },
        )?;
        linker.func_wrap(
            "env",
            "_http_respond",
            |_: Caller<'_, PluginState>, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
        )?;
        linker.func_wrap(
            "env",
            "_http_redirect",
            |_: Caller<'_, PluginState>, _: i32, _: i32, _: i32| -> i32 { 0 },
        )?;
        linker.func_wrap(
            "env",
            "_http_set_header",
            |_: Caller<'_, PluginState>, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
        )?;

        // Session stubs - signatures from codegen/mod.rs
        // _session_store: (user_id: i32, role_ptr: i32, role_len: i32, claims_ptr: i32, claims_len: i32) -> i32
        linker.func_wrap(
            "env",
            "_session_store",
            |mut caller: Caller<'_, PluginState>, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 {
                caller.data_mut().allocate(4) as i32
            },
        )?;
        // _session_create: same signature as _session_store (old API name used by frame.data <=2.0.0)
        linker.func_wrap(
            "env",
            "_session_create",
            |mut caller: Caller<'_, PluginState>, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 {
                caller.data_mut().allocate(4) as i32
            },
        )?;
        // _session_get: () -> i32 (no params - returns session JSON pointer)
        linker.func_wrap("env", "_session_get", |_: Caller<'_, PluginState>| -> i32 {
            0
        })?;
        // _session_delete: () -> i32 (no params - returns 1 if deleted)
        linker.func_wrap(
            "env",
            "_session_delete",
            |_: Caller<'_, PluginState>| -> i32 { 0 },
        )?;
        // _session_destroy: old API name for _session_delete (used by frame.data <=2.0.0)
        linker.func_wrap(
            "env",
            "_session_destroy",
            |_: Caller<'_, PluginState>| -> i32 { 0 },
        )?;
        // _session_set_cookie: old API name for _http_set_cookie (used by frame.data <=2.0.0)
        linker.func_wrap(
            "env",
            "_session_set_cookie",
            |_: Caller<'_, PluginState>, _: i32, _: i32| -> i32 { 0 },
        )?;
        // _http_set_cookie: (cookie_ptr: i32, cookie_len: i32) -> i32
        linker.func_wrap(
            "env",
            "_http_set_cookie",
            |_: Caller<'_, PluginState>, _: i32, _: i32| -> i32 { 0 },
        )?;

        // Server and state stubs (Layer 3 — runtime-only, no-op during compilation)
        // _server_sleep: (ms: i64) -> void
        linker.func_wrap(
            "env",
            "_server_sleep",
            |_: Caller<'_, PluginState>, _: i64| {},
        )?;
        // _state_reset_all: () -> void
        linker.func_wrap("env", "_state_reset_all", |_: Caller<'_, PluginState>| {})?;
        // _state_reset_named: (name_ptr: i32) -> void
        linker.func_wrap(
            "env",
            "_state_reset_named",
            |_: Caller<'_, PluginState>, _: i32| {},
        )?;

        // Response manipulation stubs (Layer 3 — runtime-only, no-op during compilation)
        // _res_set_header: (name_ptr: i32, name_len: i32, value_ptr: i32, value_len: i32) -> i32
        linker.func_wrap(
            "env",
            "_res_set_header",
            |_: Caller<'_, PluginState>, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
        )?;
        // _res_redirect: (url_ptr: i32, url_len: i32, status_code: i32) -> i32
        linker.func_wrap(
            "env",
            "_res_redirect",
            |_: Caller<'_, PluginState>, _: i32, _: i32, _: i32| -> i32 { 0 },
        )?;
        // _res_status: (code: i32) -> void
        linker.func_wrap(
            "env",
            "_res_status",
            |_: Caller<'_, PluginState>, _: i32| {},
        )?;

        // Auth stubs - Raw imports use (ptr, len) pairs for strings
        linker.func_wrap(
            "env",
            "_auth_get_session",
            |_: Caller<'_, PluginState>| -> i32 { 0 },
        )?;
        linker.func_wrap(
            "env",
            "_auth_require_auth",
            |_: Caller<'_, PluginState>| -> i32 { 0 },
        )?;
        // _auth_require_role: (role_ptr: i32, role_len: i32) -> i32
        linker.func_wrap(
            "env",
            "_auth_require_role",
            |_: Caller<'_, PluginState>, _: i32, _: i32| -> i32 { 0 },
        )?;
        // _auth_can: (permission_ptr: i32, permission_len: i32) -> i32
        linker.func_wrap(
            "env",
            "_auth_can",
            |_: Caller<'_, PluginState>, _: i32, _: i32| -> i32 { 0 },
        )?;
        // _auth_has_any_role: (roles_json_ptr: i32, roles_json_len: i32) -> i32
        linker.func_wrap(
            "env",
            "_auth_has_any_role",
            |_: Caller<'_, PluginState>, _: i32, _: i32| -> i32 { 0 },
        )?;

        // error(ptr: i32) -> i32 — stub for plugin sandbox; plugins that call error()
        // compile with this import but the sandbox only needs a no-op return.
        linker.func_wrap(
            "env",
            "error",
            |_: Caller<'_, PluginState>, _: i32| -> i32 { 0 },
        )?;
        // console_error is the underlying host import that error() maps to in codegen.
        linker.func_wrap(
            "env",
            "console_error",
            |_: Caller<'_, PluginState>, _: i32| -> i32 { 0 },
        )?;

        // JSON stubs
        linker.func_wrap(
            "env",
            "_json_encode",
            |mut caller: Caller<'_, PluginState>, _: i32| -> i32 {
                caller.data_mut().allocate(4) as i32
            },
        )?;
        linker.func_wrap(
            "env",
            "_json_decode",
            |mut caller: Caller<'_, PluginState>, _: i32, _: i32| -> i32 {
                caller.data_mut().allocate(4) as i32
            },
        )?;

        Ok(())
    }

    /// Parse plugin-generated Clean Language source code using the production parser pipeline.
    ///
    /// Plugin output must be parsed with the same `SpecificationLexer` + `SpecificationParser`
    /// pipeline used by the main compiler. The legacy `ErrorRecoveringParser` (used by
    /// `CleanParser::parse_program`) uses a different pest grammar that does not correctly
    /// handle all valid Clean Language constructs (e.g., class declarations with multiple
    /// `functions:` blocks containing whitespace-only lines).
    fn parse_plugin_code(&self, source: &str) -> Result<crate::ast::Program> {
        use crate::lexer::specification_lexer::{SourceCode, SpecificationLexer};
        use crate::parser::SpecificationParser;

        let source_code = SourceCode::new(source.to_string(), "<plugin-output>".to_string());
        let mut lexer = SpecificationLexer::new(&source_code);
        let tokens = lexer
            .tokenize()
            .map_err(|e| anyhow!("Failed to tokenize plugin output: {}", e))?;
        let mut parser = SpecificationParser::new(tokens, "<plugin-output>".to_string());
        // Plugin output may have sections in any order (e.g., external: before functions: before start:)
        parser.set_lenient_section_order(true);
        parser
            .parse_program()
            .map_err(|e| anyhow!("Failed to parse plugin output: {}", e))
    }

    /// Call the expand function in the WASM module
    ///
    /// Plugin ABI: expand(block_name: string, attributes: string, body: string) -> string
    /// Clean Language strings are pointers to [4-byte length][data] structures
    ///
    /// IMPORTANT: Clean Language uses pointer equality for string comparison.
    /// To match string literals in the plugin, we must find and reuse the
    /// existing string pointers from the plugin's data section.
    fn call_expand(&self, block: &FrameworkBlock) -> Result<Vec<Statement>> {
        // Entry-level telemetry: emit BEFORE doing any work so the framework
        // session can observe the trajectory even if the call traps later.
        tracing::debug!(
            target: "compiler::plugins::arena",
            block = block.name.trim_end_matches(':'),
            entry = "call_expand",
            plugin = %self.name,
            "plugin arena: expand-block entry",
        );

        // ── Plugin Contracts v3 early-exit ────────────────────────────────────
        // Resolve dispatch BEFORE creating the v1 store/linker so that a v3
        // plugin is never instantiated with the wrong linker.
        {
            let block_name_pre = block.name.trim_end_matches(':');
            let dispatch_pre = self.manifest.resolve_block_dispatch(block_name_pre);
            if dispatch_pre.version >= 3 {
                let (extra_attrs, actual_body) = extract_inline_attrs(&block.content);
                let mut pairs: Vec<String> = block
                    .attributes
                    .iter()
                    .map(|attr| {
                        if let Some(ref val) = attr.value {
                            let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
                            format!("\"{}\":\"{}\"", attr.name, escaped)
                        } else {
                            format!("\"{}\":true", attr.name)
                        }
                    })
                    .collect();
                pairs.extend(extra_attrs);
                let attributes_str = if pairs.is_empty() {
                    String::new()
                } else {
                    format!("{{{}}}", pairs.join(","))
                };
                let expansion = self.call_expand_typed(
                    block,
                    &dispatch_pre.export,
                    block_name_pre,
                    &attributes_str,
                    &actual_body,
                )?;
                // call_expand returns Vec<Statement> — v3 inline statements only.
                return Ok(expansion.statements);
            }
        }
        // ── v1 path continues below ───────────────────────────────────────────

        let mut store = self.create_store();
        let linker = self.get_linker()?;

        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|e| anyhow!("Failed to instantiate plugin module: {}", e))?;

        // Get memory
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("Plugin does not export memory"))?;

        // Fix heap pointer: The plugin's global[0] is the heap pointer used by malloc.
        // Some plugins have it initialized to 1024 but data section extends beyond that.
        // We need to set it to a safe value after the data section to prevent corruption.
        // Collect exported globals first, then check and fix
        let globals: Vec<_> = instance
            .exports(&mut store)
            .filter_map(|e| e.into_global())
            .collect();
        for global in globals {
            if let wasmtime::Val::I32(val) = global.get(&mut store) {
                if val == 1024 {
                    let _ = global.set(&mut store, wasmtime::Val::I32(8192));
                    break;
                }
            }
        }

        // Strip trailing colon from block name (e.g., "server:" -> "server")
        let block_name = block.name.trim_end_matches(':');

        // Binding-header preservation for ORM-style typed expression blocks
        // (`<type> <name> = Model.<verb>: ...`).
        //
        // The expander encodes the variable binding as the first line of the block
        // content (see plugins/expander.rs ~892). The plugin's expand_block expects
        // just the body — the binding is the compiler's responsibility to reattach
        // afterwards. Strip the header here, remember it, and the post-call
        // reassembly below stitches the resulting expression back into a complete
        // VariableDecl.
        //
        // The previous design had a hardcoded whitelist of "ORM verbs" that took a
        // separate direct-dispatch path through verb-specific exports. That violated
        // the architecture boundary (the compiler knew the plugin's verb list) and
        // silently dropped the binding for any verb not in the list — see bug SEM007.
        // Routing every Model.<verb>: block through the single generic expand_block
        // call removes the whitelist while preserving the binding for all verbs,
        // known or future.
        //
        // Detection heuristic: the first line is a binding header iff it contains an
        // `=` and the text before `=` has ≥2 whitespace-separated tokens (the type
        // and the identifier — `list<User> rows`). Field assignments inside e.g. an
        // `insert:` body (`name = "Alice"`) have only one token before `=`, so they
        // don't trip this detector.
        let is_dotted_verb_block = block_name.contains('.');
        let (binding_header, effective_content): (Option<String>, std::borrow::Cow<'_, str>) =
            if is_dotted_verb_block {
                let content = &block.content;
                let has_binding_header = {
                    let first_line = content.lines().next().unwrap_or("").trim();
                    if let Some(eq_pos) = first_line.find('=') {
                        let before_eq = first_line[..eq_pos].trim();
                        before_eq.split_whitespace().count() >= 2
                    } else {
                        false
                    }
                };
                if has_binding_header {
                    if let Some(newline_pos) = content.find('\n') {
                        (
                            Some(content[..newline_pos].to_string()),
                            std::borrow::Cow::Owned(content[newline_pos + 1..].to_string()),
                        )
                    } else {
                        (Some(content.clone()), std::borrow::Cow::Borrowed(""))
                    }
                } else {
                    (None, std::borrow::Cow::Borrowed(content.as_str()))
                }
            } else {
                (None, std::borrow::Cow::Borrowed(block.content.as_str()))
            };

        // Try to find an existing string pointer in the plugin's memory that matches
        // Clean Language uses pointer equality for string comparison, so we need
        // to return the same pointer the plugin uses for its string literals
        let block_name_ptr = self.find_or_write_string(&mut store, &memory, block_name)?;

        // Extract inline key="value" pairs from the first line of content
        let (extra_attrs, actual_body) = extract_inline_attrs(&effective_content);

        // Format attributes as JSON object for plugin consumption
        // Plugins expect: {"tag":"site-header","client":"off"}
        let mut pairs: Vec<String> = block
            .attributes
            .iter()
            .map(|attr| {
                if let Some(ref val) = attr.value {
                    let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
                    format!("\"{}\":\"{}\"", attr.name, escaped)
                } else {
                    format!("\"{}\":true", attr.name)
                }
            })
            .collect();
        pairs.extend(extra_attrs);
        let attributes_str = if pairs.is_empty() {
            String::new()
        } else {
            format!("{{{}}}", pairs.join(","))
        };
        let attributes_ptr = self.find_or_write_string(&mut store, &memory, &attributes_str)?;

        let body_ptr = self.find_or_write_string(&mut store, &memory, &actual_body)?;

        // v1 string-emission path — dispatch.version is guaranteed < 3 here
        // because the v3 early-exit at the top of this function already returned.
        let dispatch = self.manifest.resolve_block_dispatch(block_name);
        let expand_fn_name = &dispatch.export;
        let expand: TypedFunc<(i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, expand_fn_name)
            .map_err(|e| {
                anyhow!(
                    "Plugin does not export '{}' function: {}",
                    expand_fn_name,
                    e
                )
            })?;
        let result_ptr = expand
            .call(&mut store, (block_name_ptr, attributes_ptr, body_ptr))
            .map_err(|e| {
                let oom = store.data().oom_during_call.clone();
                anyhow!(
                    "{}",
                    describe_plugin_trap_with_oom(
                        &e,
                        &format!("expand_full block `{}`", block_name),
                        oom.as_deref(),
                    )
                )
            })?;

        // Check for errors
        if let Some(error) = store.data().last_error.clone() {
            return Err(anyhow!("Plugin error: {}", error));
        }

        // Read the result (Clean string format)
        let result_bytes = self.read_result(&store, &memory, result_ptr)?;

        // The plugin returns Clean Language source code, which we parse
        let generated_code = std::str::from_utf8(&result_bytes)
            .map_err(|e| anyhow!("Invalid UTF-8 in plugin response: {}", e))?;

        tracing::trace!(
            generated_code_len = generated_code.len(),
            "Plugin expansion result"
        );

        // Handle empty result
        if generated_code.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Reattach any binding header that was stripped on the way in.
        //
        // When the block was a typed expression of the form `<type> <name> = Model.verb: ...`,
        // we stripped the `<type> <name> =` header from the body before calling the plugin
        // so the plugin's expand_block doesn't need to know about the binding (see the
        // strip site above). If the plugin's response is a bare expression (the common case),
        // we prepend the header and let the parser produce the VariableDecl.
        //
        // If the plugin happened to emit a complete `<type> <name> = ...` statement itself
        // (e.g. older frame.data versions before binding-header detection was hoisted into
        // the compiler), we detect that and skip the prefix so we don't double up.
        //
        // The "already bound" detector compares the start of the plugin's output to the
        // exact binding header text we stripped on the way in. A naive `first_line.find('=')`
        // is unsafe here: plugin outputs routinely contain `=` inside lowered SQL string
        // literals (e.g. `_db_query("... WHERE x = 5 ...", ...)` produced by
        // `Model.find: where: x == 5`), which would false-positive every typed find/select
        // with a `where:` clause — the user's binding would silently disappear and every
        // later read of the variable would surface as `SEM007: Undefined variable`.
        // Reported as #6a754781d652 against compiler 0.30.370/0.30.371.
        let generated_code: std::borrow::Cow<'_, str> = if let Some(header) = &binding_header {
            let trimmed = generated_code.trim_start();
            let already_bound = starts_with_binding_header(trimmed, header);
            if already_bound {
                std::borrow::Cow::Borrowed(generated_code)
            } else {
                std::borrow::Cow::Owned(format!("{} {}", header, trimmed))
            }
        } else {
            std::borrow::Cow::Borrowed(generated_code)
        };
        let generated_code: &str = &generated_code;

        // Plugin output may be:
        //   (a) A complete program with a start: block  →  extract start_function.body
        //   (b) A complete program with class/function declarations  →  not suitable here;
        //       callers that need classes should use call_expand_full instead
        //   (c) A bare start: block (without a surrounding program)
        //
        // Use the production parser (SpecificationLexer + SpecificationParser) so that
        // all valid Clean Language constructs are accepted.
        if let Ok(program) = self.parse_plugin_code(generated_code) {
            if let Some(start_fn) = program.start_function {
                return Ok(start_fn.body);
            }
            // Plugin returned classes/functions but no start block —
            // there are no imperative statements to return.
            return Ok(Vec::new());
        }

        // Fallback: the plugin may have returned only the body of a start block
        // (without the "start:" header). Wrap it and try again.
        let code_without_start = if generated_code.trim().starts_with("start:") {
            generated_code
                .lines()
                .skip(1) // Skip "start:" line
                .filter(|line| !line.trim().is_empty()) // Skip whitespace-only lines
                .map(|line| {
                    // Remove one level of indentation (tab or 4 spaces)
                    if let Some(stripped) = line.strip_prefix('\t') {
                        stripped
                    } else if let Some(stripped) = line.strip_prefix("    ") {
                        stripped
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            generated_code.trim().to_string()
        };

        let wrapper = format!(
            "start:\n\t{}",
            code_without_start.trim().replace('\n', "\n\t")
        );
        let program = self.parse_plugin_code(&wrapper).map_err(|e| {
            anyhow!(
                "Failed to parse plugin output '{}' (wrapped: '{}'): {}",
                generated_code.chars().take(100).collect::<String>(),
                wrapper.chars().take(100).collect::<String>(),
                e
            )
        })?;

        // Extract statements from the start function (if present)
        let statements = program.start_function.map(|f| f.body).unwrap_or_default();

        // Telemetry: log host arena peak for this expand-block invocation.
        tracing::debug!(
            target: "compiler::plugins::arena",
            peak_bytes = store.data().peak_alloc_offset,
            final_mark_depth = store.data().arena_marks.len(),
            block = block.name.trim_end_matches(':'),
            "plugin arena: expand-block exit",
        );

        Ok(statements)
    }

    /// Call the expand function and return full expansion result
    ///
    /// This version preserves the start function if the plugin generates one
    fn call_expand_full(&self, block: &FrameworkBlock) -> Result<PluginExpansion> {
        tracing::debug!(
            target: "compiler::plugins::arena",
            block = block.name.trim_end_matches(':'),
            entry = "call_expand_full",
            plugin = %self.name,
            "plugin arena: expand-block entry",
        );

        // ── Plugin Contracts v3 early-exit ────────────────────────────────────
        // Resolve dispatch BEFORE creating the v1 store/linker so that a v3
        // plugin (which imports typed-emission bridges the v1 linker does not
        // provide) is never instantiated with the wrong linker.
        let block_name = block.name.trim_end_matches(':');
        let dispatch_pre = self.manifest.resolve_block_dispatch(block_name);
        if dispatch_pre.version >= 3 {
            let (extra_attrs, actual_body) = extract_inline_attrs(&block.content);
            let mut pairs: Vec<String> = block
                .attributes
                .iter()
                .map(|attr| {
                    if let Some(ref val) = attr.value {
                        let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
                        format!("\"{}\":\"{}\"", attr.name, escaped)
                    } else {
                        format!("\"{}\":true", attr.name)
                    }
                })
                .collect();
            pairs.extend(extra_attrs);
            let attributes_str = if pairs.is_empty() {
                String::new()
            } else {
                format!("{{{}}}", pairs.join(","))
            };
            return self.call_expand_typed(
                block,
                &dispatch_pre.export,
                block_name,
                &attributes_str,
                &actual_body,
            );
        }
        // ── v1 path continues below ───────────────────────────────────────────

        let mut store = self.create_store();
        let linker = self.get_linker()?;

        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|e| anyhow!("Failed to instantiate plugin module: {}", e))?;

        // Get memory
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("Plugin does not export memory"))?;

        // Fix heap pointer (same as in call_expand)
        let globals: Vec<_> = instance
            .exports(&mut store)
            .filter_map(|e| e.into_global())
            .collect();
        for global in globals {
            if let wasmtime::Val::I32(val) = global.get(&mut store) {
                if val == 1024 {
                    let _ = global.set(&mut store, wasmtime::Val::I32(8192));
                    break;
                }
            }
        }

        // Strip trailing colon from block name
        let block_name = block.name.trim_end_matches(':');
        let block_name_ptr = self.find_or_write_string(&mut store, &memory, block_name)?;

        // Extract inline key="value" pairs from the first line of content
        // e.g., content = 'tag="site-header"\n\thtml:\n...' → attrs: {"tag":"site-header"}, body: '\thtml:\n...'
        let (extra_attrs, actual_body) = extract_inline_attrs(&block.content);

        // Format attributes as JSON object for plugin consumption
        // Plugins expect: {"tag":"site-header","client":"off"}
        let mut pairs: Vec<String> = block
            .attributes
            .iter()
            .map(|attr| {
                if let Some(ref val) = attr.value {
                    let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
                    format!("\"{}\":\"{}\"", attr.name, escaped)
                } else {
                    format!("\"{}\":true", attr.name)
                }
            })
            .collect();
        pairs.extend(extra_attrs);
        let attributes_str = if pairs.is_empty() {
            String::new()
        } else {
            format!("{{{}}}", pairs.join(","))
        };
        let attributes_ptr = self.find_or_write_string(&mut store, &memory, &attributes_str)?;
        let body_ptr = self.find_or_write_string(&mut store, &memory, &actual_body)?;

        // v1 string-emission path — dispatch.version is guaranteed < 3 here
        // because the v3 early-exit above has already returned for v3 blocks.
        let dispatch = self.manifest.resolve_block_dispatch(block_name);
        let expand_fn_name = &dispatch.export;
        let expand: TypedFunc<(i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, expand_fn_name)
            .map_err(|e| {
                anyhow!(
                    "Plugin does not export '{}' function: {}",
                    expand_fn_name,
                    e
                )
            })?;
        let result_ptr = expand
            .call(&mut store, (block_name_ptr, attributes_ptr, body_ptr))
            .map_err(|e| {
                let oom = store.data().oom_during_call.clone();
                anyhow!(
                    "{}",
                    describe_plugin_trap_with_oom(
                        &e,
                        &format!("expand block `{}`", block_name),
                        oom.as_deref(),
                    )
                )
            })?;

        // Check for errors
        if let Some(error) = store.data().last_error.clone() {
            return Err(anyhow!("Plugin error: {}", error));
        }

        // Read the result
        let result_bytes = self.read_result(&store, &memory, result_ptr)?;
        let generated_code = std::str::from_utf8(&result_bytes)
            .map_err(|e| anyhow!("Invalid UTF-8 in plugin response: {}", e))?;

        // Handle empty result
        if generated_code.trim().is_empty() {
            return Ok(PluginExpansion::default());
        }

        tracing::debug!(
            plugin = %self.name,
            block = %block.name,
            output_len = generated_code.len(),
            "Plugin generated code"
        );

        // Try parsing as a full program using the production parser pipeline.
        // This preserves start functions, classes, and top-level functions.
        // Using SpecificationLexer + SpecificationParser (not the legacy pest-based
        // ErrorRecoveringParser) so all valid Clean Language constructs are accepted.
        match self.parse_plugin_code(generated_code) {
            Ok(program) => {
                tracing::debug!(
                    plugin = %self.name,
                    classes = program.classes.len(),
                    functions = program.functions.len(),
                    statements = program.statements.len(),
                    has_start = program.start_function.is_some(),
                    "Plugin output parsed successfully"
                );
                return Ok(PluginExpansion {
                    statements: program.statements,
                    start_function: program.start_function,
                    functions: program.functions,
                    classes: program.classes,
                    externals: program.externals,
                    state: program.state,
                });
            }
            Err(ref _e) => {
                tracing::debug!(
                    plugin = %self.name,
                    error = %_e,
                    "Direct parse failed, trying start: wrapper fallback"
                );
            }
        }

        // Fallback: the plugin may have returned only the body of a start: block.
        // Wrap it in a start: header and try again.
        //
        // CRITICAL: if the plugin output contains top-level section keywords such as
        // `functions:` or `external:` (i.e. lines that are NOT indented and start with
        // those keywords), we must NOT wrap the entire output in `start:\n\t...`.
        //
        // Wrapping structured output causes `parse_block` inside `parse_start_function`
        // to stop at the `functions:` token, producing an empty start: body.  The actual
        // `start:` block inside the wrapped content then becomes a second top-level start
        // function that `parse_program` silently discards (it keeps only the first match
        // returned by `functions.iter().find(|f| f.name == "start")`).
        //
        // Instead, when the output is structured, we re-order the sections so that
        // `functions:` (and `external:`) come before `start:` and retry the parse.
        // This handles the common case where the plugin emits `functions:` before
        // `start:` which is a valid ordering that the lenient parser accepts.
        let has_top_level_sections = {
            let trimmed = generated_code.trim();
            trimmed.lines().any(|line| {
                // A "top-level" line has no leading whitespace (or only at indentation level 0)
                let stripped = line.trim_start_matches('\t').trim_start_matches("    ");
                let is_unindented = !line.starts_with('\t') && !line.starts_with("    ");
                is_unindented
                    && (stripped.starts_with("functions:") || stripped.starts_with("external:"))
            })
        };

        if has_top_level_sections {
            // The output has structured sections; the start: wrapper would corrupt parsing.
            // Attempt a reordered parse: collect all non-start sections first, then start:.
            let trimmed = generated_code.trim();

            // Split the output into logical sections delimited by unindented section headers.
            let mut start_section_lines: Vec<&str> = Vec::new();
            let mut other_sections_lines: Vec<&str> = Vec::new();
            let mut in_start_section = false;

            for line in trimmed.lines() {
                let is_unindented = !line.starts_with('\t') && !line.starts_with("    ");
                if is_unindented && line.trim_start() == "start:" {
                    in_start_section = true;
                    start_section_lines.push(line);
                } else if is_unindented
                    && (line.trim_start().starts_with("functions:")
                        || line.trim_start().starts_with("external:"))
                {
                    in_start_section = false;
                    other_sections_lines.push(line);
                } else if in_start_section {
                    start_section_lines.push(line);
                } else {
                    other_sections_lines.push(line);
                }
            }

            // Rebuild with functions/external sections before start:
            let reordered = format!(
                "{}\n{}",
                other_sections_lines.join("\n"),
                start_section_lines.join("\n")
            );

            tracing::debug!(
                plugin = %self.name,
                "Retrying parse with reordered sections (functions/external before start)"
            );

            let program = self
                .parse_plugin_code(&reordered)
                .map_err(|e| anyhow!("Failed to parse plugin output (structured): {}", e))?;

            return Ok(PluginExpansion {
                statements: Vec::new(),
                start_function: program.start_function,
                functions: program.functions,
                classes: program.classes,
                externals: program.externals,
                state: program.state,
            });
        }

        // Simple fallback: the plugin returned only the body of a start: block
        // (no top-level sections).  Wrap it in a start: header and retry.
        let code_without_start = if generated_code.trim().starts_with("start:") {
            generated_code
                .lines()
                .skip(1)
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    if let Some(stripped) = line.strip_prefix('\t') {
                        stripped
                    } else if let Some(stripped) = line.strip_prefix("    ") {
                        stripped
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            generated_code.trim().to_string()
        };

        let wrapper = format!(
            "start:\n\t{}",
            code_without_start.trim().replace('\n', "\n\t")
        );
        let program = self
            .parse_plugin_code(&wrapper)
            .map_err(|e| anyhow!("Failed to parse plugin output: {}", e))?;

        // Telemetry: log host arena peak for this expand-block invocation.
        tracing::debug!(
            target: "compiler::plugins::arena",
            peak_bytes = store.data().peak_alloc_offset,
            final_mark_depth = store.data().arena_marks.len(),
            block = block.name.trim_end_matches(':'),
            "plugin arena: expand-block exit",
        );

        Ok(PluginExpansion {
            statements: Vec::new(),
            start_function: program.start_function,
            functions: program.functions,
            classes: program.classes,
            externals: program.externals,
            state: program.state,
        })
    }

    /// Find an existing string in plugin memory or write a new one
    ///
    /// Clean Language uses pointer equality for string comparison, so we scan
    /// the plugin's data section for matching strings and return the existing
    /// pointer if found. This allows `block_name == "server"` to work correctly.
    fn find_or_write_string(
        &self,
        store: &mut Store<PluginState>,
        memory: &Memory,
        s: &str,
    ) -> Result<i32> {
        let bytes = s.as_bytes();
        let len = bytes.len();

        // Scan the plugin's data section for a matching string
        // Clean strings are stored as [4-byte length][data]
        // Data section starts around 1024 and can extend to 32KB+ for large plugins
        let data = memory.data(&*store);
        let scan_start = 1024usize;
        let scan_end = std::cmp::min(32768usize, data.len().saturating_sub(4 + len));

        for ptr in scan_start..scan_end {
            // Check if this looks like a string with our length
            if ptr + 4 + len <= data.len() {
                let stored_len =
                    u32::from_le_bytes([data[ptr], data[ptr + 1], data[ptr + 2], data[ptr + 3]])
                        as usize;

                if stored_len == len {
                    // Check if content matches
                    let stored_data = &data[ptr + 4..ptr + 4 + len];
                    if stored_data == bytes {
                        // Found a match - return the pointer to the existing string
                        return Ok(ptr as i32);
                    }
                }
            }
        }

        // No match found, write a new string
        self.write_clean_string(store, memory, s)
    }

    /// Write a Clean string to WASM memory
    ///
    /// Clean string memory layout (from the string pointer):
    /// - Offset 0: string length (u32)
    /// - Offset 4: string data bytes
    ///
    /// This is the standard Clean Language string format used throughout the runtime.
    fn write_clean_string(
        &self,
        store: &mut Store<PluginState>,
        memory: &Memory,
        s: &str,
    ) -> Result<i32> {
        let bytes = s.as_bytes();
        let len = bytes.len();

        // Layout: [length:4][data:len]
        // Align to 8 bytes for safety
        let total_size = (4 + len).div_ceil(8) * 8;

        let ptr = store.data_mut().allocate(total_size);

        // Ensure memory is large enough
        let required_pages = ((ptr + total_size) / 65536) + 1;
        let current_pages = memory.size(&mut *store) as usize;

        if required_pages > current_pages {
            memory
                .grow(&mut *store, (required_pages - current_pages) as u64)
                .map_err(|e| anyhow!("Failed to grow memory: {}", e))?;
        }

        // Write length at offset 0 (4 bytes, little-endian)
        let len_bytes = (len as u32).to_le_bytes();
        memory.write(&mut *store, ptr, &len_bytes)?;

        // Write string data at offset 4
        if !bytes.is_empty() {
            memory.write(&mut *store, ptr + 4, bytes)?;
        }

        Ok(ptr as i32)
    }

    /// Read result bytes from WASM memory
    /// Format: [4 bytes length][data bytes]
    fn read_result(
        &self,
        store: &Store<PluginState>,
        memory: &Memory,
        ptr: i32,
    ) -> Result<Vec<u8>> {
        let data = memory.data(store);

        // Read length (first 4 bytes)
        let len_start = ptr as usize;
        if len_start + 4 > data.len() {
            return Err(anyhow!("Result pointer out of bounds"));
        }

        let len_bytes: [u8; 4] = data[len_start..len_start + 4]
            .try_into()
            .map_err(|_| anyhow!("Failed to read result length"))?;
        let len = u32::from_le_bytes(len_bytes) as usize;

        // Read data
        let data_start = len_start + 4;
        let data_end = data_start + len;

        if data_end > data.len() {
            return Err(anyhow!("Result data out of bounds"));
        }

        Ok(data[data_start..data_end].to_vec())
    }

    /// Plugin Contracts v2 — invoke a lifecycle slot with the JSON build
    /// context per `lifecycle.md` §2. The WASM signature is
    /// `(param i32) (result i32)` where the param is a Clean LP pointer to a
    /// length-prefixed UTF-8 JSON string and the result is a Clean LP pointer
    /// to a JSON response in the expansion output format.
    ///
    /// `slot_name` is used for diagnostic messages only; the actual export
    /// called is `export_name` (read from `manifest.lifecycle.<slot>`).
    fn call_lifecycle_slot_v2(
        &self,
        slot_name: &str,
        export_name: &str,
        context: &crate::plugins::BuildContext,
    ) -> Result<PluginExpansion> {
        tracing::debug!(
            target: "compiler::plugins::arena",
            slot = slot_name,
            export = export_name,
            entry = "call_lifecycle_slot_v2",
            plugin = %self.name,
            "plugin arena: lifecycle-slot entry",
        );

        let mut store = self.create_store();
        let linker = self.get_linker()?;

        let instance = linker.instantiate(&mut store, &self.module).map_err(|e| {
            anyhow!(
                "Failed to instantiate plugin module for lifecycle slot `{}`: {}",
                slot_name,
                e
            )
        })?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("Plugin does not export memory"))?;

        // Same heap-pointer fix as call_expand_full — keeps plugin allocator
        // from colliding with the WASM data section.
        let globals: Vec<_> = instance
            .exports(&mut store)
            .filter_map(|e| e.into_global())
            .collect();
        for global in globals {
            if let wasmtime::Val::I32(val) = global.get(&mut store) {
                if val == 1024 {
                    let _ = global.set(&mut store, wasmtime::Val::I32(8192));
                    break;
                }
            }
        }

        // Serialize the build context to JSON and write to plugin memory.
        let context_json = serde_json::to_string(context).map_err(|e| {
            anyhow!(
                "Failed to serialize build context for lifecycle slot `{}`: {}",
                slot_name,
                e
            )
        })?;
        let context_ptr = self.find_or_write_string(&mut store, &memory, &context_json)?;

        // Call the slot — signature `(param i32) -> i32`.
        let slot: TypedFunc<i32, i32> =
            instance
                .get_typed_func(&mut store, export_name)
                .map_err(|e| {
                    anyhow!(
                        "Plugin does not export lifecycle slot `{}` (export `{}`): {}",
                        slot_name,
                        export_name,
                        e
                    )
                })?;

        let result_ptr = slot.call(&mut store, context_ptr).map_err(|e| {
            anyhow!(
                "{}",
                describe_plugin_trap(&e, &format!("lifecycle slot `{}`", slot_name))
            )
        })?;

        if let Some(error) = store.data().last_error.clone() {
            return Err(anyhow!(
                "Plugin error in lifecycle slot `{}`: {}",
                slot_name,
                error
            ));
        }

        let result_bytes = self.read_result(&store, &memory, result_ptr)?;
        // Empty response → empty expansion (the plugin chose to contribute
        // nothing for this build context, e.g. no components to hydrate).
        if result_bytes.is_empty() {
            return Ok(PluginExpansion::default());
        }
        let response_str = std::str::from_utf8(&result_bytes).map_err(|e| {
            anyhow!(
                "Invalid UTF-8 in lifecycle slot `{}` response: {}",
                slot_name,
                e
            )
        })?;

        // Parse `{"statements": "<source>"}` and lift to a PluginExpansion by
        // re-parsing the source statements using the same path the block
        // expander uses for plugin output.
        #[derive(serde::Deserialize, Default)]
        struct SlotResponse {
            #[serde(default)]
            statements: String,
            #[serde(default)]
            error: Option<String>,
        }
        let response: SlotResponse = serde_json::from_str(response_str).map_err(|e| {
            anyhow!(
                "Failed to parse lifecycle slot `{}` response as JSON: {} — raw: {}",
                slot_name,
                e,
                &response_str[..response_str.len().min(256)]
            )
        })?;
        if let Some(err) = response.error {
            return Err(anyhow!(
                "Lifecycle slot `{}` reported error: {}",
                slot_name,
                err
            ));
        }
        if response.statements.trim().is_empty() {
            return Ok(PluginExpansion::default());
        }

        // Re-parse the contributed Clean source so the statements integrate
        // into the program AST like any other plugin-produced code.
        //
        // The lifecycle-slot contract says the `statements` field carries
        // statement-level code (no top-level `start:` / `functions:` wrapper),
        // but the Clean parser only accepts whole programs. Wrap the slot
        // output in a synthetic `start:` block — tab-indented so each line
        // becomes part of the block body — parse, then lift the statements
        // out of the resulting start_function.
        //
        // If the output happens to already contain top-level keywords
        // (legacy plugins shipping a full `start:` block), fall back to
        // parsing as-is so we don't break that shape.
        let parsed = match self.parse_plugin_code(&response.statements) {
            Ok(p) => p,
            Err(_) => {
                let wrapped = wrap_slot_statements_as_start(&response.statements);
                self.parse_plugin_code(&wrapped).map_err(|e| {
                    anyhow!(
                        "Lifecycle slot `{}` returned statements that could not be parsed \
                         even when wrapped in a `start:` block: {} — raw: {}",
                        slot_name,
                        e,
                        &response.statements[..response.statements.len().min(256)]
                    )
                })?
            }
        };
        // The slot is conceptually contributing statement-level code. Most
        // plugins will return just statements; for plugins that include a
        // start function (e.g. legacy compatibility shims), merge its body
        // into the statements list.
        let mut statements = parsed.statements;
        if let Some(start_fn) = parsed.start_function {
            statements.extend(start_fn.body);
        }

        // Telemetry: log host arena peak for this lifecycle slot invocation.
        tracing::debug!(
            target: "compiler::plugins::arena",
            peak_bytes = store.data().peak_alloc_offset,
            final_mark_depth = store.data().arena_marks.len(),
            slot = slot_name,
            "plugin arena: expand-block exit",
        );

        Ok(PluginExpansion {
            statements,
            start_function: None,
            functions: parsed.functions,
            classes: parsed.classes,
            externals: parsed.externals,
            state: parsed.state,
        })
    }

    /// Call the plugin's `process_html` export. The plugin transforms a raw
    /// HTML page into Clean Language source. Returns the generated source as
    /// a UTF-8 string.
    ///
    /// Signature (per `PluginExports::process_html`):
    /// `(html_ptr, path_ptr, registry_ptr, companion_ptr) -> result_ptr`
    /// where each `*_ptr` points to a length-prefixed Clean string.
    fn call_process_html(
        &self,
        html: &str,
        path: &str,
        registry_json: &str,
        companion_json: &str,
    ) -> Result<String> {
        let Some(export_name) = self.manifest.exports.process_html.as_ref() else {
            return Err(anyhow!(
                "Plugin `{}` did not declare an HTML processor (no `[exports].process_html`)",
                self.name
            ));
        };

        let mut store = self.create_store();
        let linker = self.get_linker()?;

        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|e| anyhow!("Failed to instantiate plugin module: {}", e))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("Plugin does not export memory"))?;

        // Same heap-pointer fix as call_expand_full — keeps the plugin's
        // bump allocator from colliding with the WASM data section.
        let globals: Vec<_> = instance
            .exports(&mut store)
            .filter_map(|e| e.into_global())
            .collect();
        for global in globals {
            if let wasmtime::Val::I32(val) = global.get(&mut store) {
                if val == 1024 {
                    let _ = global.set(&mut store, wasmtime::Val::I32(8192));
                    break;
                }
            }
        }

        let html_ptr = self.find_or_write_string(&mut store, &memory, html)?;
        let path_ptr = self.find_or_write_string(&mut store, &memory, path)?;
        let registry_ptr = self.find_or_write_string(&mut store, &memory, registry_json)?;
        let companion_ptr = self.find_or_write_string(&mut store, &memory, companion_json)?;

        let process: TypedFunc<(i32, i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, export_name.as_str())
            .map_err(|e| anyhow!("Plugin does not export `{}` function: {}", export_name, e))?;

        let result_ptr = process
            .call(
                &mut store,
                (html_ptr, path_ptr, registry_ptr, companion_ptr),
            )
            .map_err(|e| {
                anyhow!(
                    "{}",
                    describe_plugin_trap(&e, &format!("process_html for `{}`", path))
                )
            })?;

        if let Some(error) = store.data().last_error.clone() {
            return Err(anyhow!("Plugin error in process_html: {}", error));
        }

        let result_bytes = self.read_result(&store, &memory, result_ptr)?;
        let generated = std::str::from_utf8(&result_bytes)
            .map_err(|e| anyhow!("Invalid UTF-8 in process_html response: {}", e))?;

        Ok(generated.to_string())
    }

    /// Call a no-argument lifecycle hook that returns a length-prefixed JSON
    /// string pointer and deserialise it into `T`.
    ///
    /// This is the common pattern shared by all four lifecycle hooks:
    /// `register_server`, `register_cli`, `register_data`, `register_build`.
    fn call_lifecycle_hook<T>(&self, export_name: &str) -> Result<T>
    where
        T: DeserializeOwned + Default,
    {
        let mut store = self.create_store();
        let linker = self.get_linker()?;

        let instance = linker.instantiate(&mut store, &self.module).map_err(|e| {
            anyhow!(
                "Failed to instantiate plugin module for lifecycle hook: {}",
                e
            )
        })?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("Plugin does not export memory"))?;

        let hook: TypedFunc<(), i32> =
            instance
                .get_typed_func(&mut store, export_name)
                .map_err(|e| {
                    anyhow!(
                        "Plugin does not export lifecycle hook '{}': {}",
                        export_name,
                        e
                    )
                })?;

        let result_ptr = hook.call(&mut store, ()).map_err(|e| {
            anyhow!(
                "{}",
                describe_plugin_trap(&e, &format!("lifecycle hook `{}`", export_name))
            )
        })?;

        if let Some(error) = store.data().last_error.clone() {
            return Err(anyhow!(
                "Plugin error in lifecycle hook '{}': {}",
                export_name,
                error
            ));
        }

        let result_bytes = self.read_result(&store, &memory, result_ptr)?;

        // Empty result → use the Default value so callers never see an error
        // for hooks that return an empty JSON document or a zero-length string.
        if result_bytes.is_empty() {
            return Ok(T::default());
        }

        let json_str = std::str::from_utf8(&result_bytes).map_err(|e| {
            anyhow!(
                "Non-UTF-8 response from lifecycle hook '{}': {}",
                export_name,
                e
            )
        })?;

        serde_json::from_str(json_str).map_err(|e| {
            anyhow!(
                "Failed to parse JSON response from lifecycle hook '{}': {} — raw: {}",
                export_name,
                e,
                &json_str[..json_str.len().min(256)]
            )
        })
    }

    /// Call the `register_server` lifecycle hook and return the parsed
    /// `ServerRegistration`.  Returns `None` if the plugin does not declare
    /// this hook in its manifest.
    pub fn call_register_server(&self) -> Option<super::plugin_abi::ServerRegistration> {
        let export_name = self.manifest.exports.register_server.as_ref()?;
        match self.call_lifecycle_hook::<super::plugin_abi::ServerRegistration>(export_name) {
            Ok(reg) => Some(reg),
            Err(e) => {
                tracing::warn!(
                    plugin = %self.name,
                    export = %export_name,
                    error = %e,
                    "register_server lifecycle hook failed"
                );
                None
            }
        }
    }

    /// Call the `register_cli` lifecycle hook and return the parsed
    /// `CliRegistration`.  Returns `None` if the plugin does not declare this
    /// hook in its manifest.
    pub fn call_register_cli(&self) -> Option<super::plugin_abi::CliRegistration> {
        let export_name = self.manifest.exports.register_cli.as_ref()?;
        match self.call_lifecycle_hook::<super::plugin_abi::CliRegistration>(export_name) {
            Ok(reg) => Some(reg),
            Err(e) => {
                tracing::warn!(
                    plugin = %self.name,
                    export = %export_name,
                    error = %e,
                    "register_cli lifecycle hook failed"
                );
                None
            }
        }
    }

    /// Call the `register_data` lifecycle hook and return the parsed
    /// `DataRegistration`.  Returns `None` if the plugin does not declare this
    /// hook in its manifest.
    pub fn call_register_data(&self) -> Option<super::plugin_abi::DataRegistration> {
        let export_name = self.manifest.exports.register_data.as_ref()?;
        match self.call_lifecycle_hook::<super::plugin_abi::DataRegistration>(export_name) {
            Ok(reg) => Some(reg),
            Err(e) => {
                tracing::warn!(
                    plugin = %self.name,
                    export = %export_name,
                    error = %e,
                    "register_data lifecycle hook failed"
                );
                None
            }
        }
    }

    /// Call the `register_build` lifecycle hook and return the parsed
    /// `BuildRegistration`.  Returns `None` if the plugin does not declare
    /// this hook in its manifest.
    pub fn call_register_build(&self) -> Option<super::plugin_abi::BuildRegistration> {
        let export_name = self.manifest.exports.register_build.as_ref()?;
        match self.call_lifecycle_hook::<super::plugin_abi::BuildRegistration>(export_name) {
            Ok(reg) => Some(reg),
            Err(e) => {
                tracing::warn!(
                    plugin = %self.name,
                    export = %export_name,
                    error = %e,
                    "register_build lifecycle hook failed"
                );
                None
            }
        }
    }
}

impl FrameworkPlugin for WasmPluginAdapter {
    fn name(&self) -> &'static str {
        self.name_cache
    }

    fn handles(&self) -> &'static [&'static str] {
        // This is safe because we pre-allocated in new()
        Box::leak(self.handles_cache.clone().into_boxed_slice())
    }

    fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        self.call_expand(block)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: block.name.clone(),
                message: e.to_string(),
                location: block.location.clone(),
            })
    }

    fn expand_full(&self, block: &FrameworkBlock) -> PluginResult<PluginExpansion> {
        self.call_expand_full(block)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: block.name.clone(),
                message: e.to_string(),
                location: block.location.clone(),
            })
    }

    fn process_html(
        &self,
        html: &str,
        path: &str,
        registry_json: &str,
        companion_json: &str,
    ) -> PluginResult<Option<String>> {
        // Plugin opted out of HTML processing — return None so the caller
        // tries the next plugin.
        if self.manifest.exports.process_html.is_none() {
            return Ok(None);
        }

        self.call_process_html(html, path, registry_json, companion_json)
            .map(Some)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "__process_html".to_string(),
                message: e.to_string(),
                location: None,
            })
    }

    fn invoke_lifecycle_slot(
        &self,
        slot_name: &str,
        context: &crate::plugins::BuildContext,
    ) -> PluginResult<PluginExpansion> {
        // Plugin Contracts v2 — read the slot's export name from the manifest's
        // [lifecycle] section. A missing entry is a no-op (the plugin did not
        // opt into this slot). See contracts/lifecycle.md §3.
        let export_name = match slot_name {
            "module_helpers" => self.manifest.lifecycle.module_helpers.as_deref(),
            "program_init" => self.manifest.lifecycle.program_init.as_deref(),
            "client_init" => self.manifest.lifecycle.client_init.as_deref(),
            "server_init" => self.manifest.lifecycle.server_init.as_deref(),
            "per_request" => self.manifest.lifecycle.per_request.as_deref(),
            "artifact_emitters" => self.manifest.lifecycle.artifact_emitters.as_deref(),
            _ => None,
        };
        let Some(export_name) = export_name else {
            return Ok(PluginExpansion::default());
        };
        // Plugin Contracts v3 sub-cycle 3 — when the plugin opts into typed
        // emission at the manifest level, dispatch lifecycle slots through the
        // typed-emission path. Per typed-emission.md §7 the v3 export ships
        // with a different ABI shape (`(ctx, build_context_lp) -> i32` + arena
        // bridge calls) so the dispatch decision is per-plugin, not per-slot.
        if self.manifest.opts_into_typed_emission() {
            return self
                .call_lifecycle_slot_typed(slot_name, export_name, context)
                .map_err(|e| PluginError::ExpansionFailed {
                    plugin_name: self.name.clone(),
                    block_name: format!("__lifecycle_{}_typed", slot_name),
                    message: format!(
                        "typed lifecycle slot `{}` invocation failed: {}",
                        slot_name, e
                    ),
                    location: None,
                });
        }
        // Call the slot via the dedicated v2 protocol — single string param
        // carrying the JSON build context per contracts/lifecycle.md §2.1.
        self.call_lifecycle_slot_v2(slot_name, export_name, context)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: format!("__lifecycle_{}", slot_name),
                message: format!("lifecycle slot `{}` invocation failed: {}", slot_name, e),
                location: None,
            })
    }

    fn version(&self) -> &'static str {
        self.version_cache
    }

    fn get_keywords(&self) -> &'static [&'static str] {
        // LSP features provided by plugin's exported WASM functions
        &[]
    }

    fn get_completions(&self, _ctx: &PluginLspContext) -> Vec<PluginCompletionItem> {
        // LSP completions from plugin's exported WASM functions
        Vec::new()
    }

    fn get_hover_info(&self, _keyword: &str) -> Option<PluginHoverInfo> {
        // LSP hover info from plugin's exported WASM functions
        None
    }

    fn get_diagnostics(&self, _content: &str) -> Vec<PluginDiagnostic> {
        // LSP diagnostics from plugin's exported WASM functions
        Vec::new()
    }

    fn description(&self) -> &'static str {
        self.description_cache
    }

    fn expression_patterns(&self) -> &[String] {
        &self.expression_patterns_cache
    }

    fn assemble(
        &self,
        input: &crate::plugins::plugin_abi::AssembleInput,
    ) -> crate::plugins::PluginResult<crate::plugins::plugin_abi::AssembleOutput> {
        use crate::plugins::plugin_abi::AssembleOutput;

        let export_name = match self.manifest.exports.assemble.as_deref() {
            Some(name) => name.to_owned(),
            None => return Ok(AssembleOutput::default()),
        };

        let input_json = serde_json::to_string(input).map_err(|e| {
            crate::plugins::PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: format!("Failed to serialize AssembleInput: {}", e),
                location: None,
            }
        })?;

        let mut store = self.create_store();
        let linker =
            self.get_linker()
                .map_err(|e| crate::plugins::PluginError::ExpansionFailed {
                    plugin_name: self.name.clone(),
                    block_name: "assemble".to_string(),
                    message: e.to_string(),
                    location: None,
                })?;

        let instance = linker.instantiate(&mut store, &self.module).map_err(|e| {
            crate::plugins::PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: format!("Failed to instantiate plugin: {}", e),
                location: None,
            }
        })?;

        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
            crate::plugins::PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: "Plugin does not export memory".to_string(),
                location: None,
            }
        })?;

        let input_ptr = self
            .find_or_write_string(&mut store, &memory, &input_json)
            .map_err(|e| crate::plugins::PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: format!("Failed to write input to WASM memory: {}", e),
                location: None,
            })?;

        let assemble_fn: wasmtime::TypedFunc<i32, i32> = instance
            .get_typed_func(&mut store, &export_name)
            .map_err(|e| crate::plugins::PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: format!(
                    "Plugin does not export assemble function '{}': {}",
                    export_name, e
                ),
                location: None,
            })?;

        let result_ptr = assemble_fn.call(&mut store, input_ptr).map_err(|e| {
            crate::plugins::PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: format!("assemble() call failed: {}", e),
                location: None,
            }
        })?;

        if let Some(error) = store.data().last_error.clone() {
            return Err(crate::plugins::PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: format!("Plugin error in assemble: {}", error),
                location: None,
            });
        }

        let result_bytes = self.read_result(&store, &memory, result_ptr).map_err(|e| {
            crate::plugins::PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: format!("Failed to read assemble result: {}", e),
                location: None,
            }
        })?;

        if result_bytes.is_empty() {
            // Empty result from a non-trivial input is almost always the
            // signature of an out-of-memory failure inside the plugin:
            // `mem_alloc` returns 0 once `memory.grow()` can't extend further
            // (capped by the plugin's `[memory]` max), the plugin propagates
            // that 0 through its accumulator strings, and the final result
            // pointer is also 0 — which we then read as zero bytes. Without
            // a warning the failure is invisible (downstream SEM007 makes it
            // look like the plugin "just didn't emit the renamed function")
            // and bisecting takes hours. Surface it the first time we see it.
            //
            // The 0-input edge case (no source files at all) is legitimately
            // empty — only warn if we sent the plugin something to chew on.
            if input_json.len() > 256 {
                eprintln!(
                    "warning: plugin '{}' assemble() returned 0 bytes for {}-byte input. \
                     This almost always means the plugin exhausted its WASM memory \
                     (`memory.grow()` failed). If you control the plugin, rebuild it with \
                     `--target=plugin` so it gets the 1 GB plugin tier instead of the \
                     32 MB standard tier. If you don't, please report this — the host can't \
                     safely fall back to anything useful here.",
                    self.name,
                    input_json.len()
                );
            }
            return Ok(AssembleOutput::default());
        }

        let json_str = std::str::from_utf8(&result_bytes).map_err(|e| {
            crate::plugins::PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: format!("Non-UTF-8 response from assemble: {}", e),
                location: None,
            }
        })?;

        serde_json::from_str(json_str).map_err(|e| crate::plugins::PluginError::ExpansionFailed {
            plugin_name: self.name.clone(),
            block_name: "assemble".to_string(),
            message: format!(
                "Failed to parse assemble JSON response: {} — raw: {}",
                e,
                &json_str[..json_str.len().min(256)]
            ),
            location: None,
        })
    }
}

/// An opaque mark into the host bridge arena's transient zone.
///
/// Returned by `PluginState::arena_mark` and consumed by
/// `PluginState::arena_reset`. Marked `#[must_use]` so callers that
/// take a mark and forget to reset get a compiler warning.
#[must_use]
#[derive(Debug, Clone, Copy)]
struct ArenaMark(usize);

/// State held by the WASM store
pub(crate) struct PluginState {
    /// Current bump-allocator top. All active allocations live in
    /// `[stable_zone_end, alloc_offset)`. The stable zone
    /// `[524288, stable_zone_end)` holds pointers that must survive
    /// an `arena_reset` (e.g. `cached_empty_lp_ptr`).
    alloc_offset: usize,
    /// Floor below which `arena_reset` will never rewind. Starts at
    /// 524288 (the initial `alloc_offset`) and is advanced by each
    /// `allocate_stable` call. All pointers in `[524288, stable_zone_end)`
    /// are long-lived and are never reclaimed within a store lifetime.
    stable_zone_end: usize,
    /// Peak `alloc_offset` observed during this store's lifetime.
    /// Updated by `allocate` on every successful bump. Logged at the
    /// end of each expand-block call for arena-pressure telemetry.
    peak_alloc_offset: usize,
    /// Stack of save marks for nested `_arena_scope_push` / `_arena_scope_pop`
    /// calls. Each entry is the `alloc_offset` at the time of the push.
    /// `arena_reset` pops the top entry and rewinds `alloc_offset` to it.
    arena_marks: Vec<usize>,
    /// Last error reported by plugin
    last_error: Option<String>,
    /// Pointer to a pre-allocated empty LP-string in plugin memory.
    /// Lazily initialized on first use by `write_clean_string` / the
    /// `_build_state_get` bridge so repeated empty returns reuse the same
    /// stable address instead of bump-allocating a fresh 4-byte zero block
    /// on every call. See `compiler-build-state-bridge-runtime-trap.md`.
    /// Lives in the stable zone — always below `stable_zone_end`.
    cached_empty_lp_ptr: Option<i32>,
    /// First memory-allocation failure observed during the current plugin
    /// call, with site-of-failure context. Bridge functions that allocate
    /// (mem_alloc, string.concat, etc.) set this when `memory.grow` is
    /// refused by the host (typically because the Plugin tier 1GB cap was
    /// reached) and then return 0 to the WASM caller — the plugin treats
    /// 0 as a valid pointer, propagates it through subsequent string ops,
    /// and ultimately hits an out-of-bounds memory access on a read of
    /// what looks like a length-prefix at address 0. Without this field,
    /// the surfaced error is the bare wasmtime trap message ("wasm trap:
    /// out of bounds memory access") with no indication that the
    /// underlying cause was OOM. The expand-block call site checks this
    /// after the trap is reported and prepends the structured diagnostic.
    /// See COMPILER-MEM-ALLOC-NO-GROW-RECURRENCE (fp b80c2f907c71).
    oom_during_call: Option<String>,
    /// Plugin Contracts v3 typed-emission arena. Installed by
    /// `call_expand_typed` before instantiation and taken after the expand
    /// call returns. The `Option<>` is `None` on all v1 call paths.
    pub(crate) emit_arena: Option<crate::plugins::typed_emission::EmitArena>,
}

/// Emit arena telemetry on every store drop — including trap/error paths.
///
/// The success-path telemetry sites at `call_expand` / `call_expand_full` /
/// `call_lifecycle_slot_v2` only fire after `Ok(_)` returns. If the WASM
/// call traps (e.g. host bridge OOM at memory.grow, OOB on the page-boundary
/// corruption, scope_pop with a stale handle), the `?` operator propagates
/// the error and those sites are skipped. The framework session observed
/// this directly at 0.30.396: a 24,511-line `RUST_LOG=debug` log of an N=24
/// expand_endpoints trap with zero matches for `arena|alloc_offset|peak`.
///
/// `Drop` fires unconditionally — even on panic unwind. So this is where
/// we anchor the always-emit arena trajectory record. Success-path sites
/// keep their extra context (block name, structured exit reason) but the
/// Drop record is the lower bound on observability.
///
/// Safety: `tracing::debug!` is safe in `Drop` — it does not allocate on
/// the failure path (the macro is a pre-formatted span emission), and on
/// the success path any allocation failure is swallowed by `tracing` itself.
impl Drop for PluginState {
    fn drop(&mut self) {
        // Skip the trivial no-allocation case — a PluginState that never
        // saw a bridge call is just initial state. Keeps logs clean for
        // test harnesses that instantiate state but never run anything.
        if self.peak_alloc_offset == 524288 && self.arena_marks.is_empty() {
            return;
        }
        let oom_marker = self
            .oom_during_call
            .as_deref()
            .map(|s| s.chars().take(120).collect::<String>())
            .unwrap_or_default();
        tracing::debug!(
            target: "compiler::plugins::arena",
            peak_bytes = self.peak_alloc_offset,
            stable_zone_end = self.stable_zone_end,
            final_alloc_offset = self.alloc_offset,
            arena_marks_depth = self.arena_marks.len(),
            oom_during_call = %oom_marker,
            "plugin arena: store drop (covers both success and trap paths)",
        );
    }
}

impl PluginState {
    fn new() -> Self {
        Self {
            // Start allocations at 512KB to avoid collision with WASM data section
            // and the module's internal heap which can grow from lower addresses.
            // The WASM module's static data starts at 4KB and heap follows the data
            // section. For large plugins with many string operations, 512KB is safe.
            alloc_offset: 524288,
            stable_zone_end: 524288,
            peak_alloc_offset: 524288,
            arena_marks: Vec::new(),
            last_error: None,
            cached_empty_lp_ptr: None,
            oom_during_call: None,
            emit_arena: None,
        }
    }

    /// Bump-allocate `size` bytes from the host-side bridge arena.
    ///
    /// **Returns 0 (exhaustion sentinel) when the next allocation would
    /// push the bump pointer past `i32::MAX`.** Plugin bridges return the
    /// allocated pointer to WASM as `i32`; if `alloc_offset` ever exceeds
    /// `i32::MAX as usize` (≈ 2.15 GB), the `as i32` cast wraps to a
    /// negative value, the plugin treats it as a valid pointer, and
    /// subsequent pointer arithmetic lands on low data-section addresses
    /// — clobbering literals and producing the panic recorded as
    /// COMPILER-PLUGIN-STRING-COMPARE-PANIC-OUT-OF-BOUNDS
    /// (dashboard fingerprint `e4a5886bd58d26a8...`).
    ///
    /// The fix:
    ///   1. Once a request would push `alloc_offset` past `i32::MAX`,
    ///      return 0 — every bridge function already handles 0 as a
    ///      failure case (e.g. `if memory.write(...).is_err() { return 0 }`).
    ///   2. Cap `alloc_offset` at `i32::MAX` so subsequent calls are
    ///      idempotent and the offset cannot wrap `usize` on long-running
    ///      plugins.
    ///
    /// See `system-documents/diagnostics/COMPILER-PLUGIN-STRING-COMPARE-PANIC-DIAGNOSIS.md`
    /// for the full root cause analysis.
    fn allocate(&mut self, size: usize) -> usize {
        let aligned = (size + 7) & !7;
        // Use `checked_add` so a malicious / runaway plugin can't wrap
        // `usize` itself with a huge `size` argument.
        let Some(new_top) = self.alloc_offset.checked_add(aligned) else {
            return 0;
        };
        if new_top > i32::MAX as usize {
            // Exhaustion. Cap `alloc_offset` so subsequent calls also
            // return 0 without further advancing.
            self.alloc_offset = i32::MAX as usize;
            return 0;
        }
        let ptr = self.alloc_offset;
        self.alloc_offset = new_top;
        // Track high-water mark for telemetry (see peak_alloc_offset field).
        if new_top > self.peak_alloc_offset {
            self.peak_alloc_offset = new_top;
        }
        ptr
    }

    /// Bump-allocate `size` bytes from the **stable** zone.
    ///
    /// Stable allocations are excluded from `arena_reset` — they survive
    /// for the entire lifetime of the store. Today the only caller is the
    /// `cached_empty_lp_ptr` initialization in `write_clean_string`.
    ///
    /// # Panics (debug builds only)
    ///
    /// Panics if called while any arena scope is open (`arena_marks` is
    /// non-empty), because advancing `stable_zone_end` while a scope is
    /// active would corrupt the reset invariant.
    fn allocate_stable(&mut self, size: usize) -> usize {
        debug_assert!(
            self.arena_marks.is_empty(),
            "allocate_stable called while arena scope is open (arena_marks.len={})",
            self.arena_marks.len()
        );
        let ptr = self.allocate(size);
        if ptr != 0 {
            // Advance the stable zone to cover this new allocation so
            // future `arena_reset` calls can never rewind past it.
            self.stable_zone_end = self.alloc_offset;
        }
        ptr
    }

    /// Save the current transient-zone top.
    ///
    /// Returns an `ArenaMark` that can be passed to `arena_reset` to
    /// reclaim every allocation made after this point. Push the mark
    /// onto the internal stack so nested scopes compose correctly.
    fn arena_mark(&mut self) -> ArenaMark {
        let mark = ArenaMark(self.alloc_offset);
        self.arena_marks.push(self.alloc_offset);
        mark
    }

    /// Reset the transient zone to `mark`, reclaiming all allocations
    /// made since the corresponding `arena_mark` call.
    ///
    /// This function only rewinds `alloc_offset`; it does NOT pop from
    /// `arena_marks`. Stack management is the caller's responsibility.
    /// The `_arena_scope_pop` bridge calls this after popping the mark
    /// from the stack.
    ///
    /// # Panics (debug builds only)
    ///
    /// - If `mark` is below `stable_zone_end` (would reclaim stable data).
    /// - If `mark` is above the current `alloc_offset` (invalid mark —
    ///   was the store mutated concurrently?).
    fn arena_reset(&mut self, mark: ArenaMark) {
        debug_assert!(
            mark.0 >= self.stable_zone_end,
            "arena_reset: mark {} is below stable_zone_end {} — would reclaim stable data",
            mark.0,
            self.stable_zone_end
        );
        debug_assert!(
            mark.0 <= self.alloc_offset,
            "arena_reset: mark {} is above current alloc_offset {} — invalid mark",
            mark.0,
            self.alloc_offset
        );
        self.alloc_offset = mark.0;
    }
}

#[cfg(test)]
mod arena_tests {
    use super::PluginState;

    #[test]
    fn mark_and_reset_reuses_offset() {
        let mut state = PluginState::new();
        let mark = state.arena_mark();
        let ptr1 = state.allocate(1024);
        assert_ne!(ptr1, 0, "first allocation must succeed");
        state.arena_reset(mark);
        let ptr2 = state.allocate(1024);
        assert_eq!(
            ptr1, ptr2,
            "allocation after reset must reuse the same offset"
        );
    }

    #[test]
    fn nested_mark_reset_reuses_inner_offset() {
        let mut state = PluginState::new();
        let outer_mark = state.arena_mark();
        let _outer_alloc = state.allocate(512);
        let inner_mark = state.arena_mark();
        let inner_ptr = state.allocate(256);
        assert_ne!(inner_ptr, 0);
        // arena_reset rewinds the offset but does NOT pop the stack;
        // the caller is responsible for stack cleanup (as the bridge does).
        state.arena_reset(inner_mark);
        // Pop the inner mark from the stack manually (mirrors bridge behavior).
        state.arena_marks.pop();
        let reuse_ptr = state.allocate(256);
        assert_eq!(
            inner_ptr, reuse_ptr,
            "inner reset must reclaim only the inner allocation"
        );
        // Outer reset recovers everything after the outer mark.
        let outer_ptr_before_outer_reset = state.alloc_offset;
        state.arena_reset(outer_mark);
        state.arena_marks.pop();
        assert!(
            state.alloc_offset < outer_ptr_before_outer_reset,
            "outer reset must rewind past inner allocations"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    fn allocate_stable_while_scope_open_panics() {
        use std::panic;
        let result = panic::catch_unwind(|| {
            let mut state = PluginState::new();
            let _mark = state.arena_mark();
            // This must panic in debug builds.
            state.allocate_stable(8);
        });
        assert!(
            result.is_err(),
            "allocate_stable while a scope is open must panic in debug builds"
        );
    }
}

/// Helper to write a UTF-8 string as a Clean length-prefixed allocation
/// and return its pointer. Wraps `write_clean_string` with a `Result`-style
/// signature that yields `None` on memory failure rather than a sentinel 0.
/// Format a wasmtime `.call()` error with the trap kind (when available)
/// prepended to the default backtrace. Lets the framework see, e.g.,
/// "out of bounds memory access at wasm function 276" instead of the bare
/// `<unknown>!<wasm function 276>`. See
/// `compiler-build-state-bridge-runtime-trap.md`.
fn describe_plugin_trap(err: &anyhow::Error, context: &str) -> String {
    let mut prefix = format!("Plugin trap in {}", context);
    // Walk the error chain for the first wasmtime::Trap. wasmtime returns
    // the Trap inside an anyhow::Error whose root cause is the Trap value.
    let trap = err
        .chain()
        .find_map(|cause| cause.downcast_ref::<wasmtime::Trap>());
    if let Some(trap) = trap {
        prefix.push_str(&format!(" [{}]", trap));
    }
    let base = format!("{}: {}", prefix, err);

    // Epoch-interrupt traps mean we hit the per-call deadline set by
    // `create_store`. Spell out for the user what happened, since
    // `wasm trap: interrupt` alone is opaque.
    if matches!(trap, Some(wasmtime::Trap::Interrupt)) {
        let timeout = super::wasm_loader::plugin_timeout_secs();
        format!(
            "{}\n\n\
            note: this plugin call exceeded the configured timeout ({} s).\n\
                  the WASM backtrace above shows where the plugin was\n\
                  running when the deadline expired — that path is either\n\
                  in an infinite loop or genuinely too slow for this\n\
                  workload. raise the budget with CLN_PLUGIN_TIMEOUT_SECS=N\n\
                  (or CLN_PLUGIN_TIMEOUT_SECS=0 to disable the deadline)\n\
                  and re-run to confirm. report a compiler bug if the\n\
                  backtrace points at a small plugin function that\n\
                  obviously terminates on the given input.",
            base, timeout,
        )
    } else {
        base
    }
}

/// Like `describe_plugin_trap`, but additionally surfaces the host-bridge
/// OOM context (if any) captured during the call. When `memory.grow` is
/// refused mid-expand, host bridges (`mem_alloc`, `string.concat`) record
/// the site of failure in `PluginState::oom_during_call` and then return 0
/// to the plugin. The plugin treats 0 as a valid pointer, propagates it
/// through subsequent string ops, and ultimately traps on an
/// out-of-bounds memory access reading what looks like a length-prefix at
/// or near address 0. The raw trap message (`wasm trap: out of bounds
/// memory access`) gives no hint that OOM was the underlying cause — so
/// when oom context exists, prepend a structured note pointing at the
/// real root cause and offer concrete remediation steps.
fn describe_plugin_trap_with_oom(
    err: &anyhow::Error,
    context: &str,
    oom_context: Option<&str>,
) -> String {
    let base = describe_plugin_trap(err, context);
    let Some(oom) = oom_context else {
        return base;
    };
    format!(
        "{}\n\n\
        note: host bridge ran out of memory during this call. site: {}\n\
              the plugin tier caps WASM linear memory at 1 GiB. the bridge\n\
              recorded this OOM and returned 0 to the plugin; the plugin\n\
              then propagated the 0 pointer through string ops until a\n\
              downstream read tried to interpret it as a length-prefix,\n\
              producing the wasm out-of-bounds trap above.\n\n\
              to compile larger inputs, either reduce the workload (split\n\
              the source file, simplify expansions) or rebuild the plugin\n\
              with the freeing allocator path so intermediate strings are\n\
              reclaimable mid-call. see COMPILER-MEM-ALLOC-NO-GROW-RECURRENCE\n\
              for the durable fix design (per-expand-block arena).",
        base, oom,
    )
}

/// Bounds-checked read of a Clean LP-string from a borrowed memory slice.
/// Returns `None` for any pointer that cannot be safely interpreted as a
/// `[4-byte LE length][data]` value (negative, OOB header, length larger
/// than what's actually addressable).
///
/// Host bridges that read LP-strings without using this helper panic on
/// the raw slice when a plugin passes a pointer to clobbered memory —
/// the COMPILER-PLUGIN-STRING-COMPARE-PANIC-OUT-OF-BOUNDS symptom.
fn read_lp_from_data(data: &[u8], ptr: i32) -> Option<&[u8]> {
    if ptr < 0 {
        return None;
    }
    let len_start = ptr as usize;
    let after_header = len_start.checked_add(4)?;
    if after_header > data.len() {
        return None;
    }
    let len_bytes: [u8; 4] = data[len_start..after_header].try_into().ok()?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let data_end = after_header.checked_add(len)?;
    if data_end > data.len() {
        return None;
    }
    Some(&data[after_header..data_end])
}

/// Plugin Contracts v2 — register the `_build_state_set` / `_build_state_get`
/// bridges on the given linker, sharing the provided `BuildState` store.
///
/// **Calling convention:** both bridges follow the canonical contract in
/// `foundation/platform-architecture/function-registry.toml`, which the
/// registry loader expands with `expand_strings=true` — every `"string"`
/// param becomes a raw `(ptr: i32, len: i32)` pair, and `"void"` returns
/// nothing. The compiler emits an unpacking wrapper on the plugin side, so
/// plugins can still call these bridges with a Clean `string` value; the
/// host just sees the unpacked pair.
fn register_build_state_bridges(
    linker: &mut Linker<PluginState>,
    build_state: &crate::plugins::BuildState,
) -> Result<()> {
    // `_build_state_set(key_ptr, key_len, value_ptr, value_len)` — stores a
    // value under a key in the per-build state. `void` return per the
    // canonical contract; the WASM import has no result.
    let state_for_set = std::sync::Arc::clone(build_state);
    linker.func_wrap(
        "env",
        "_build_state_set",
        move |mut caller: Caller<'_, PluginState>,
              key_ptr: i32,
              key_len: i32,
              value_ptr: i32,
              value_len: i32| {
            let Some(key) = read_raw_string(&mut caller, key_ptr, key_len) else {
                return;
            };
            if key.is_empty() {
                return;
            }
            let Some(value) = read_raw_string(&mut caller, value_ptr, value_len) else {
                return;
            };
            if let Ok(mut guard) = state_for_set.lock() {
                guard.insert(key, value);
            }
        },
    )?;

    // `_build_state_get(key_ptr, key_len) -> string_lp_ptr` — returns an LP
    // pointer to the value, or an LP-pointer to a cached empty string when
    // the key is absent. The empty-string LP pointer is cached in
    // `PluginState` so warmup-time misses don't churn the bump allocator.
    let state_for_get = std::sync::Arc::clone(build_state);
    linker.func_wrap(
        "env",
        "_build_state_get",
        move |mut caller: Caller<'_, PluginState>, key_ptr: i32, key_len: i32| -> i32 {
            let Some(key) = read_raw_string(&mut caller, key_ptr, key_len) else {
                return 0;
            };
            let value = state_for_get
                .lock()
                .ok()
                .and_then(|g| g.get(&key).cloned())
                .unwrap_or_default();
            if value.is_empty() {
                if let Some(cached) = caller.data().cached_empty_lp_ptr {
                    return cached;
                }
            }
            // Non-empty strings use the normal transient allocator.
            // Empty strings are written via the stable allocator so that
            // `cached_empty_lp_ptr` is never reclaimed by an `arena_reset`.
            let ptr = if value.is_empty() {
                write_clean_string_stable(&mut caller, b"")
            } else {
                write_clean_string(&mut caller, value.as_bytes())
            };
            if value.is_empty() && ptr != 0 {
                caller.data_mut().cached_empty_lp_ptr = Some(ptr);
            }
            ptr
        },
    )?;

    Ok(())
}

/// Read a raw `(ptr, len)` UTF-8 string from plugin memory. This is the
/// counterpart to `read_clean_string` for bridges that follow the
/// `expand_strings=true` convention from `function-registry.toml`, where a
/// `"string"` param is emitted as two i32s (pointer + byte length) rather
/// than a single LP pointer.
fn read_raw_string(caller: &mut Caller<'_, PluginState>, ptr: i32, len: i32) -> Option<String> {
    if ptr < 0 || len < 0 {
        return None;
    }
    let memory = caller.get_export("memory").and_then(|e| e.into_memory())?;
    let data = memory.data(&*caller);
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    if end > data.len() {
        return None;
    }
    std::str::from_utf8(&data[start..end])
        .ok()
        .map(str::to_string)
}

fn read_clean_string(caller: &mut Caller<'_, PluginState>, ptr: i32) -> Option<String> {
    let memory = caller.get_export("memory").and_then(|e| e.into_memory())?;
    let data = memory.data(&*caller);

    let len_start = ptr as usize;
    if ptr < 0 || len_start.checked_add(4)? > data.len() {
        return None;
    }

    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().ok()?;
    let len = u32::from_le_bytes(len_bytes) as usize;

    let data_start = len_start + 4;
    let data_end = data_start.checked_add(len)?;

    if data_end > data.len() {
        return None;
    }

    std::str::from_utf8(&data[data_start..data_end])
        .ok()
        .map(|s| s.to_string())
}

/// Write a byte slice as a Clean length-prefixed string into WASM memory,
/// allocating from the **stable** zone so the pointer survives any future
/// `arena_reset`. Only used for long-lived pointers that must outlive any
/// transient scope (today: `cached_empty_lp_ptr`).
///
/// Returns the pointer on success or 0 on failure.
fn write_clean_string_stable(caller: &mut Caller<'_, PluginState>, data: &[u8]) -> i32 {
    let data_len = data.len();
    let total_size = 4 + data_len;

    // Allocate from the stable zone.
    let ptr = caller.data_mut().allocate_stable(total_size);
    if ptr == 0 {
        return 0;
    }

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

/// Helper to write a byte slice as a Clean length-prefixed string into WASM memory.
///
/// Allocates space via the bump allocator, grows the module's linear memory if the
/// allocation exceeds the current `data_size`, then writes `[4-byte LE length][data]`.
///
/// Returns the pointer on success or 0 if memory cannot be obtained / grown.
fn write_clean_string(caller: &mut Caller<'_, PluginState>, data: &[u8]) -> i32 {
    let data_len = data.len();
    let total_size = 4 + data_len;

    // Allocate via the bump allocator.
    let ptr = caller.data_mut().allocate(total_size);

    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => m,
        None => return 0,
    };

    // Grow the module's linear memory if the allocation extends past the current limit.
    // Use reborrows (&mut *caller) so the borrow-checker knows we re-use the same caller.
    let current_size = memory.data_size(&mut *caller);
    let required_size = ptr + total_size;
    if required_size > current_size {
        let pages_needed = (required_size - current_size).div_ceil(65536);
        if memory.grow(&mut *caller, pages_needed as u64).is_err() {
            return 0;
        }
    }

    // Write length header then content.
    let len_bytes = (data_len as u32).to_le_bytes();
    if memory.write(&mut *caller, ptr, &len_bytes).is_err() {
        return 0;
    }
    if memory.write(&mut *caller, ptr + 4, data).is_err() {
        return 0;
    }

    ptr as i32
}

/// Extract inline `key="value"` pairs from the first line of block content.
///
/// For content like `tag="site-header"\n\thtml:\n\t\t<h1>...`, this extracts:
/// - attrs: `["\"tag\":\"site-header\""]` (JSON key-value pairs)
/// - body: `\thtml:\n\t\t<h1>...` (remaining content after the attribute line)
///
/// If the first line has no `key="value"` pattern, returns empty attrs and full content.
/// Wrap statement-level slot output in a synthetic `start:` block so the
/// program parser accepts it. Used by `call_lifecycle_slot_v2` when the
/// plugin's slot returned raw statements without any top-level wrapper.
///
/// Each input line is tab-indented so it becomes part of the start block
/// body. Empty trailing lines are preserved to match the Clean grammar's
/// terminator requirements.
fn wrap_slot_statements_as_start(source: &str) -> String {
    let mut wrapped = String::with_capacity(source.len() + 16);
    wrapped.push_str("start:\n");
    for line in source.lines() {
        wrapped.push('\t');
        wrapped.push_str(line);
        wrapped.push('\n');
    }
    wrapped
}

fn extract_inline_attrs(content: &str) -> (Vec<String>, String) {
    // Split at first newline
    let (first_line, rest) = match content.find('\n') {
        Some(pos) => (&content[..pos], &content[pos + 1..]),
        None => (content, ""),
    };

    let trimmed = first_line.trim();

    // Check if the first line contains key="value" patterns (not indented block content)
    if trimmed.is_empty() || trimmed.starts_with('\t') || trimmed.starts_with('<') {
        return (Vec::new(), content.to_string());
    }

    // Parse key="value" pairs from the first line
    let mut pairs = Vec::new();
    let mut remaining = trimmed;

    while !remaining.is_empty() {
        // Find key=
        if let Some(eq_pos) = remaining.find('=') {
            let key = remaining[..eq_pos].trim();
            let after_eq = &remaining[eq_pos + 1..];

            if let Some(after_quote) = after_eq.strip_prefix('"') {
                // Find closing quote
                if let Some(close_pos) = after_quote.find('"') {
                    let value = &after_quote[..close_pos];
                    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                    pairs.push(format!("\"{}\":\"{}\"", key, escaped));
                    remaining = after_quote[close_pos + 1..].trim_start();
                    continue;
                }
            }
        }
        // If we can't parse, treat the whole line as non-attribute content
        return (Vec::new(), content.to_string());
    }

    if pairs.is_empty() {
        (Vec::new(), content.to_string())
    } else {
        (pairs, rest.to_string())
    }
}

/// True when `generated_code` already starts with the binding header we stripped
/// before dispatch — i.e. the plugin chose to re-emit the `<type> <name> =`
/// prefix itself (older frame.data behavior).  In that case the compiler must
/// NOT prepend the header again or the result would be `<type> <name> = <type> <name> = ...`.
///
/// The comparison is whitespace-tolerant: both sides are reduced to a
/// space-separated token sequence and the plugin output is accepted when its
/// first tokens match the binding header's tokens in order, followed by `=`.
///
/// This replaces an earlier `first_line.find('=')` heuristic that misfired
/// whenever the plugin output's first line happened to contain `=` inside a
/// string literal — a routine occurrence in `_db_query("... WHERE x = 5 ...", ...)`
/// expressions emitted by `Model.find: where: x == 5`.  When the heuristic
/// flagged false-positive "already bound", the compiler skipped re-prepending
/// the user's `<type> <name> =`, the variable was never declared, and every
/// later read of it surfaced as SEM007 with no source location.  Reported as
/// #6a754781d652 against 0.30.370/0.30.371.
fn starts_with_binding_header(generated_code: &str, binding_header: &str) -> bool {
    let header_tokens: Vec<&str> = binding_header
        .trim()
        .trim_end_matches('=')
        .split_whitespace()
        .collect();
    if header_tokens.is_empty() {
        return false;
    }

    let first_line = generated_code.lines().next().unwrap_or("").trim_start();
    let mut cursor = first_line;
    for tok in &header_tokens {
        cursor = cursor.trim_start();
        if let Some(rest) = cursor.strip_prefix(tok) {
            // Token must be followed by whitespace or `=` — otherwise we matched
            // a longer identifier (`string_pool` would otherwise consume `string`).
            match rest.chars().next() {
                Some(c) if c.is_whitespace() || c == '=' => cursor = rest,
                None => cursor = rest,
                _ => return false,
            }
        } else {
            return false;
        }
    }
    let after_header = cursor.trim_start();
    after_header.starts_with('=') && !after_header.starts_with("==")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression for #6a754781d652 — SEM007 in 0.30.370/0.30.371.
    ///
    /// The compiler's "already bound" detection in `call_expand` previously
    /// used `first_line.find('=')` to decide whether the plugin had already
    /// emitted its own `<type> <name> = ...` prefix.  Plugin outputs for
    /// `Model.find: where: x == 5` lower to single-expression form like
    /// `json.get(json.get(_db_query("... WHERE x = 5", "[]"), "data"), "rows")`.
    /// The naive heuristic spotted the `=` inside the SQL string literal,
    /// concluded the plugin had bound the variable, and skipped re-prepending
    /// `string result = `.  The user's `result` was never declared and
    /// downstream reads surfaced as SEM007 with no source location.
    ///
    /// The fix compares against the actual `binding_header` token sequence.
    #[test]
    fn test_starts_with_binding_header_recognises_genuine_binding() {
        // Older plugins re-emit the header verbatim; treat as already bound.
        assert!(starts_with_binding_header(
            "string result = json.get(_db_query(\"SELECT *\", \"[]\"), \"data\")",
            "string result =",
        ));
        // Whitespace tolerance.
        assert!(starts_with_binding_header(
            "string  result   =   json.get(...)",
            "string result =",
        ));
    }

    #[test]
    fn test_starts_with_binding_header_ignores_eq_inside_string_literal() {
        // The bug repro: the `=` lives inside the SQL string literal, not at
        // the start of a binding.  Must NOT be treated as already-bound.
        let plugin_output = "json.get(json.get(_db_query(\"SELECT * FROM languages \
                             WHERE \" + \"is_active = true\" + \" ORDER BY name ASC\", \
                             \"[]\"), \"data\"), \"rows\")";
        assert!(!starts_with_binding_header(
            plugin_output,
            "string result ="
        ));
    }

    #[test]
    fn test_starts_with_binding_header_rejects_unrelated_internal_binding() {
        // Old tenant-aware path emits its own helper-local declarations like
        // `string __tf_sql = "..."` — that binds a DIFFERENT variable, not the
        // user's `result`.  Must NOT be treated as already-bound.
        assert!(!starts_with_binding_header(
            "string __tf_sql = \"SELECT *\"\nstring __tf_tenant = tenant_getId()",
            "string result =",
        ));
    }

    #[test]
    fn test_starts_with_binding_header_rejects_longer_prefix() {
        // `string_pool` must not match the `string` token: the lexer would
        // tokenise as a single identifier, so a textual match would be wrong.
        assert!(!starts_with_binding_header(
            "string_pool = something",
            "string result =",
        ));
    }

    #[test]
    fn test_starts_with_binding_header_rejects_equality_compare() {
        // `string result == ...` is a comparison, not a binding (and would be
        // syntactically invalid at the statement level anyway — but the
        // detector should still reject it cleanly).
        assert!(!starts_with_binding_header(
            "string result == foo",
            "string result =",
        ));
    }

    #[test]
    fn test_plugin_state_allocation() {
        let mut state = PluginState::new();

        let ptr1 = state.allocate(100);
        // Starts at 512KB (524288) to avoid collision with WASM data section
        assert_eq!(ptr1, 524288);

        let ptr2 = state.allocate(200);
        // 524288 + 100 = 524388, aligned to 8 = 524392
        assert_eq!(ptr2, 524392);

        let ptr3 = state.allocate(50);
        // 524392 + 200 = 524592, aligned to 8 = 524592
        assert_eq!(ptr3, 524592);
    }

    /// Regression for COMPILER-PLUGIN-STRING-COMPARE-PANIC-OUT-OF-BOUNDS
    /// (dashboard fingerprint `e4a5886bd58d26a8...`).
    ///
    /// Without the fix: a sustained chain of `string.concat` bridge calls
    /// (frame.ui 2.12.23's `fuse_literal_concats` loop) advances
    /// `alloc_offset` past `i32::MAX` (≈ 2.15 GB). The returned pointer
    /// wraps to negative `i32`; the plugin treats it as a valid base for
    /// pointer arithmetic and writes plugin-generated bytes into low
    /// data-section addresses, clobbering literals. `string_compare` later
    /// reads the clobbered literal's length (now garbage) and panics on
    /// the slice index.
    ///
    /// With the fix: `allocate` caps at `i32::MAX` and returns 0 — every
    /// bridge already handles 0 as a clean failure.
    /// See `system-documents/diagnostics/COMPILER-PLUGIN-STRING-COMPARE-PANIC-DIAGNOSIS.md`.
    #[test]
    fn allocate_never_returns_pointer_that_wraps_to_negative_i32() {
        let mut state = PluginState::new();
        let mut chunk = 1024usize;
        let mut seen_exhaustion = false;

        for _ in 0..40 {
            let ptr = state.allocate(chunk);
            assert!(
                (ptr as i32) >= 0,
                "allocate returned {} (raw usize), {} as i32 — would corrupt \
                 plugin pointer arithmetic. chunk={}, alloc_offset_after={}",
                ptr,
                ptr as i32,
                chunk,
                state.alloc_offset,
            );
            if state.alloc_offset >= i32::MAX as usize {
                assert_eq!(
                    ptr, 0,
                    "after crossing i32::MAX, allocate must return 0, got {}",
                    ptr
                );
                seen_exhaustion = true;
            }
            chunk = chunk.saturating_mul(2);
        }

        assert!(
            seen_exhaustion,
            "test must drive alloc_offset past i32::MAX to exercise the overflow path"
        );
    }

    #[test]
    fn allocate_after_exhaustion_remains_at_cap() {
        let mut state = PluginState::new();
        // Drive to exhaustion in one massive request.
        let _first = state.allocate(i32::MAX as usize);
        let saturated_offset = state.alloc_offset;
        assert!(saturated_offset >= i32::MAX as usize);

        for _ in 0..1000 {
            let ptr = state.allocate(8);
            assert_eq!(ptr, 0, "post-exhaustion allocate must return 0");
            assert_eq!(
                state.alloc_offset, saturated_offset,
                "post-exhaustion alloc_offset must not advance"
            );
        }
    }

    /// Plugin Contracts v2 §2.5 smoke test — exercises `_build_state_set`
    /// and `_build_state_get` end-to-end via a hand-crafted WAT module that
    /// imports them with the **canonical** signatures from
    /// `foundation/platform-architecture/function-registry.toml`
    /// (`expand_strings=true`):
    ///
    ///   `_build_state_set(key_ptr, key_len, value_ptr, value_len) -> ()`
    ///   `_build_state_get(key_ptr, key_len) -> i32`  (LP-pointer)
    ///
    /// Calls the production `register_build_state_bridges` rather than
    /// redeclaring stubs, so the import-shape contract between this test and
    /// the linker is what every shipped plugin actually sees.
    ///
    /// Guards against:
    /// - Signature drift between the host adapter and the registry
    ///   (CMP-PLUGIN-ABI-BUILD-STATE-SET-MISMATCH).
    /// - Param convention, return type, and runtime trap regressions on the
    ///   empty-return path (0.30.258–0.30.260).
    #[test]
    fn test_build_state_bridges_round_trip() {
        use crate::plugins::new_build_state;
        use wasmtime::{Engine, Linker, Module, Store};

        let engine = Engine::default();
        let build_state = new_build_state();

        // Mini plugin matching the canonical expand_strings=true shape: the
        // data section holds raw UTF-8 bytes (no LP prefix) for "k" / "v" /
        // "missing", and the wrappers pass each as a (ptr, len) pair.
        let wat = r#"
            (module
              (import "env" "_build_state_set"
                (func $set (param i32 i32 i32 i32)))
              (import "env" "_build_state_get"
                (func $get (param i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 16) "k")
              (data (i32.const 32) "v")
              (data (i32.const 48) "missing")
              (func (export "round_trip") (result i32)
                i32.const 16
                i32.const 1
                i32.const 32
                i32.const 1
                call $set
                i32.const 16
                i32.const 1
                call $get)
              (func (export "missing_get") (result i32)
                i32.const 48
                i32.const 7
                call $get)
              (func (export "missing_get_again") (result i32)
                i32.const 48
                i32.const 7
                call $get))
        "#;

        let module = Module::new(&engine, wat).expect("compile mini plugin");
        let mut store = Store::new(&engine, PluginState::new());
        let mut linker: Linker<PluginState> = Linker::new(&engine);

        // Exercise the production code path. If the signature here drifts
        // away from what plugins import, instantiation will fail with the
        // exact "incompatible import type for `env::_build_state_set`" error
        // that bit frame.ui 2.12.36.
        super::register_build_state_bridges(&mut linker, &build_state)
            .expect("register_build_state_bridges");

        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let memory = instance.get_memory(&mut store, "memory").expect("memory");

        // 1. Round-trip: set("k","v") then get("k") returns LP pointer with "v".
        let round_trip: TypedFunc<(), i32> =
            instance.get_typed_func(&mut store, "round_trip").unwrap();
        let result_ptr = round_trip.call(&mut store, ()).expect("round_trip ok");
        let data = memory.data(&store);
        let len = u32::from_le_bytes(
            data[result_ptr as usize..result_ptr as usize + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let bytes = &data[result_ptr as usize + 4..result_ptr as usize + 4 + len];
        assert_eq!(bytes, b"v", "round_trip should retrieve stored value");

        // 2. Missing key returns a stable empty LP pointer (length 0, not null).
        let missing_get: TypedFunc<(), i32> =
            instance.get_typed_func(&mut store, "missing_get").unwrap();
        let missing_ptr = missing_get.call(&mut store, ()).expect("missing_get ok");
        assert!(
            missing_ptr != 0,
            "missing-key returns valid LP pointer, not 0 sentinel"
        );
        let data = memory.data(&store);
        let len = u32::from_le_bytes(
            data[missing_ptr as usize..missing_ptr as usize + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(len, 0, "missing-key LP pointer length must be 0");

        // 3. The empty-string pointer is cached and reused across calls.
        let missing_again: TypedFunc<(), i32> = instance
            .get_typed_func(&mut store, "missing_get_again")
            .unwrap();
        let second_ptr = missing_again
            .call(&mut store, ())
            .expect("missing_get_again ok");
        assert_eq!(
            second_ptr, missing_ptr,
            "second empty-string lookup must reuse the cached LP pointer"
        );
    }

    /// Integration test: verify the frame.ui plugin compiled with the CURRENT compiler
    /// loads and instantiates cleanly via the full WasmPluginAdapter path.
    ///
    /// This test requires /tmp/test_plugins/frame.ui/2.6.6/plugin.wasm to exist.
    /// Build it with:
    ///   ./target/debug/cln compile \
    ///     /path/to/clean-framework/plugins/frame.ui/src/main.cln \
    ///     -o /tmp/test_plugins/frame.ui/2.6.6/plugin.wasm --target=plugin
    ///
    /// If the file is missing, the test is skipped (not failed) so CI doesn't break
    /// on machines where the framework checkout isn't available.
    #[test]
    fn test_frame_ui_plugin_expand_html_block_nonempty() {
        use std::path::PathBuf;

        let plugin_wasm = PathBuf::from("/tmp/test_plugins/frame.ui/2.6.6/plugin.wasm");
        if !plugin_wasm.exists() {
            eprintln!("SKIP: /tmp/test_plugins/frame.ui/2.6.6/plugin.wasm not found");
            return;
        }

        let loader_result = super::super::wasm_loader::WasmPluginLoader::with_plugins_dir(
            PathBuf::from("/tmp/test_plugins"),
        );
        let mut loader = match loader_result {
            Ok(l) => l,
            Err(e) => panic!("Failed to create plugin loader: {}", e),
        };

        let registry = loader
            .load_plugins(&["frame.ui".to_string()])
            .expect("Failed to load frame.ui from /tmp/test_plugins");

        // Create a minimal html: FrameworkBlock that exercises expand_html_block
        // with {!expr} raw interpolations — the pattern that was broken.
        use crate::ast::SourceLocation;
        use crate::plugins::FrameworkBlock;
        let block = FrameworkBlock {
            name: "html".to_string(),
            content: "{!head}\n{!nav}\n<div class=\"container\"><h1>Hello</h1></div>\n{!foot}"
                .to_string(),
            attributes: vec![],
            location: Some(SourceLocation {
                file: "test".into(),
                line: 1,
                column: 1,
                byte_start: None,
                byte_end: None,
            }),
        };

        let stmts = registry.expand(&block).expect("expand_block must not fail");

        // The result must be non-empty — if it's empty the complex-function bug is back
        assert!(!stmts.is_empty(), "expand_html_block returned 0 statements — complex-function empty-return bug is present");

        // The generated code must reference head, nav, foot as raw variables
        let code: String = format!("{:?}", stmts);
        assert!(
            code.contains("head") || code.contains("__html"),
            "Output must reference the html: expansion variables: {:?}",
            stmts
        );
    }

    /// Integration test: directly call the plugin's own WASM html_block_to_code function
    /// via call_expand_full, calling the plugin WASM's html_block_to_code directly.
    ///
    /// call_expand_full calls html_block_to_code on the plugin WASM directly (not the shim).
    /// If the complex-function-returns-empty bug (0.30.49+) is still present, this test
    /// will fail because html_block_to_code will return "".
    ///
    /// Requires /tmp/test_plugins/frame.ui/2.6.6/plugin.wasm (skipped if absent).
    #[test]
    fn test_frame_ui_plugin_html_block_to_code_direct_wasm() {
        use std::path::PathBuf;

        let plugin_wasm = PathBuf::from("/tmp/test_plugins/frame.ui/2.6.6/plugin.wasm");
        if !plugin_wasm.exists() {
            eprintln!("SKIP: /tmp/test_plugins/frame.ui/2.6.6/plugin.wasm not found");
            return;
        }

        let loader_result = super::super::wasm_loader::WasmPluginLoader::with_plugins_dir(
            PathBuf::from("/tmp/test_plugins"),
        );
        let mut loader = match loader_result {
            Ok(l) => l,
            Err(e) => panic!("Failed to create plugin loader: {}", e),
        };

        let registry = loader
            .load_plugins(&["frame.ui".to_string()])
            .expect("Failed to load frame.ui from /tmp/test_plugins");

        use crate::ast::SourceLocation;
        use crate::plugins::FrameworkBlock;
        let block = FrameworkBlock {
            name: "html".to_string(),
            content: "<div class=\"container\"><h1>Hello</h1></div>".to_string(),
            attributes: vec![],
            location: Some(SourceLocation {
                file: "test".into(),
                line: 1,
                column: 1,
                byte_start: None,
                byte_end: None,
            }),
        };

        // expand_full routes through call_expand_full which calls html_block_to_code
        // on the plugin WASM directly — NOT the Rust shim.
        let expansion = registry
            .expand_full(&block)
            .expect("expand_full must not fail");

        // If the complex-function-returns-empty bug is present, statements and
        // functions will be empty because the plugin's WASM html_block_to_code returns "".
        let has_content = !expansion.statements.is_empty()
            || !expansion.functions.is_empty()
            || expansion.start_function.is_some();
        assert!(
            has_content,
            "Plugin WASM html_block_to_code returned empty — complex-function bug still present in this build. \
             Consider rebuilding frame.ui with a newer compiler. \
             Expansion: {:?}",
            expansion
        );

        // Verify attribute handling: <div class="container"> must produce "class" in output.
        // Local variable index mismatch bug (0.30.7-era) would drop the attribute name,
        // producing '="container"' instead of 'class="container"'.
        // The generated code may be in statements (direct parse) or start_function (fallback).
        let content_str = if !expansion.statements.is_empty() {
            format!("{:?}", expansion.statements)
        } else {
            format!("{:?}", expansion.start_function)
        };
        assert!(
            content_str.contains("class"),
            "Attribute name 'class' missing from expansion — local variable index mismatch bug may be present: {}",
            content_str
        );
        assert!(
            content_str.contains("container"),
            "Attribute value 'container' missing from expansion: {}",
            content_str
        );
    }

    /// Regression test for Bug SYN007 / fingerprint ad3b3521f8fa:
    /// When plugin output has `functions:` before `start:`, the `start:` block
    /// must NOT be silently dropped.
    ///
    /// This tests `parse_plugin_code` (the production parser pipeline used by
    /// `call_expand_full`) directly, without needing a real WASM plugin binary.
    #[test]
    fn test_start_block_not_dropped_when_functions_before_start() {
        use crate::lexer::specification_lexer::{SourceCode, SpecificationLexer};
        use crate::parser::SpecificationParser;

        // Minimal plugin output: functions: section before start:
        // This is the shape that expand_endpoints() produces.
        // Note: Clean Language function syntax in functions: block does NOT use a trailing ':'
        // after the signature — indentation alone delimits the body.
        let plugin_output = "\
functions:
\tinteger handleHome()
\t\treturn 0

start:
\t_http_route(\"GET\", \"/\", handleHome)
";
        let source_code = SourceCode::new(
            plugin_output.to_string(),
            "<test-plugin-output>".to_string(),
        );
        let mut lexer = SpecificationLexer::new(&source_code);
        let tokens = lexer
            .tokenize()
            .expect("tokenize must not fail for valid plugin output");
        let mut parser = SpecificationParser::new(tokens, "<test-plugin-output>".to_string());
        // Mimic parse_plugin_code: lenient section ordering for plugin output
        parser.set_lenient_section_order(true);
        let program = parser
            .parse_program()
            .expect("parse_program must not fail for functions:-before-start: output");

        // The start: block must NOT be dropped.
        assert!(
            program.start_function.is_some(),
            "start: block was silently dropped when functions: appears before start: — Bug SYN007 regression"
        );

        let start_fn = program.start_function.unwrap();
        assert!(
            !start_fn.body.is_empty(),
            "start: block body is empty — route calls were dropped"
        );

        // The functions: block must also be preserved.
        // (parse_program moves start into start_function AND keeps it in functions,
        //  so we check for at least one non-start function.)
        let has_handle_home = program.functions.iter().any(|f| f.name == "handleHome");
        assert!(
            has_handle_home,
            "handleHome function from functions: block was lost during parse"
        );
    }

    /// Regression test for the fallback reordering path in `call_expand_full`.
    ///
    /// If the primary `parse_plugin_code` call fails AND the output has a top-level
    /// `functions:` section, the fallback must reorder sections (functions first,
    /// then start:) instead of wrapping the whole output in `start:\n\t...`, which
    /// would produce an empty start body.
    #[test]
    fn test_fallback_reorder_does_not_wrap_functions_in_start() {
        use crate::lexer::specification_lexer::{SourceCode, SpecificationLexer};
        use crate::parser::SpecificationParser;

        // Simulate what the fallback reordering produces when given:
        //   functions: <...> start: <...>
        // The reordered string must parse into a program where start: is non-empty.
        let reordered = "\
functions:
\tinteger handleHome()
\t\treturn 0

start:
\t_http_route(\"GET\", \"/\", handleHome)
";
        let source_code = SourceCode::new(reordered.to_string(), "<test-reordered>".to_string());
        let mut lexer = SpecificationLexer::new(&source_code);
        let tokens = lexer.tokenize().expect("tokenize must not fail");
        let mut parser = SpecificationParser::new(tokens, "<test-reordered>".to_string());
        parser.set_lenient_section_order(true);
        let program = parser
            .parse_program()
            .expect("reordered plugin output must parse");

        assert!(
            program.start_function.is_some(),
            "start: block dropped from reordered plugin output"
        );
        let start_fn = program.start_function.unwrap();
        assert!(
            !start_fn.body.is_empty(),
            "start: block body empty after reorder parse"
        );
    }
}
