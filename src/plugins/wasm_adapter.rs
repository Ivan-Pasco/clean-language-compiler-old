//! WASM Plugin Adapter
//!
//! Wraps a WASM module to implement the FrameworkPlugin trait
//! Provides full Clean Language runtime environment for plugin execution

use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicI32, Ordering};
use wasmtime::{
    AsContextMut, Caller, Engine, ExternType, Linker, Memory, Module, Store, TypedFunc, ValType,
};

use crate::ast::{FrameworkBlock, Statement};
use crate::plugins::{
    FrameworkPlugin, HtmlEntryOutput, PluginError, PluginExpansion, PluginResult,
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
    /// Block types this plugin handles, interned (`intern_str_list`) so
    /// `FrameworkPlugin::handles` can hand out the `'static` slice.
    handles_cache: &'static [&'static str],
    /// Cached ORM expression patterns from `[handles] expressions`
    expression_patterns_cache: Vec<String>,
    /// Cached name string
    name_cache: &'static str,
    /// Cached version string
    version_cache: &'static str,
    /// Cached description string
    description_cache: &'static str,
    /// v1 linker — set up once, reused across all v1 calls. The Linker is
    /// bound to the Engine (not the Store), so it can be shared across
    /// multiple Store/Instance pairs.
    linker: Linker<PluginState>,
    /// Typed-emission linker (v3 calls), built on first use.
    typed_emission_linker: std::sync::OnceLock<Linker<PluginState>>,
    /// Contract 5 lint linker, built on first use.
    lint_linker: std::sync::OnceLock<Linker<PluginState>>,
    /// Plugin Contracts v2 — shared per-build state. Handed to every store
    /// this adapter creates, where the `_build_state_set` /
    /// `_build_state_get` bridges read it, so every plugin loaded with the
    /// same `BuildState` shares the same keystore.
    /// See `foundation/spec/plugins/contracts/lifecycle.md` §2.5.
    build_state: crate::plugins::BuildState,
    /// Monotonically-increasing counter for typed-emission ctx handles.
    /// Each `call_expand_typed` call allocates a fresh ctx by fetching-and-
    /// incrementing this counter. Because ctx values are never recycled, a
    /// plugin cannot accidentally re-use a handle from a prior call even if
    /// it stored the old ctx in its `_build_state_set` keystore.
    next_ctx_counter: AtomicI32,
    /// Keywords from the plugin's `[exports].get_keywords` export, read on
    /// first use.
    keywords_cache: std::sync::OnceLock<&'static [&'static str]>,
}

/// Strings interned for the process lifetime; see `intern_str`.
static INTERNED_STRS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<&'static str>>,
> = std::sync::OnceLock::new();

/// String lists interned for the process lifetime; see `intern_str_list`.
#[allow(clippy::type_complexity)]
static INTERNED_STR_LISTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<Vec<&'static str>, &'static [&'static str]>>,
> = std::sync::OnceLock::new();

/// A `'static` copy of `s`, allocated once per distinct value for the life
/// of the process.
fn intern_str(s: &str) -> &'static str {
    let mut set = INTERNED_STRS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(&interned) = set.get(s) {
        return interned;
    }
    let interned: &'static str = Box::leak(s.to_owned().into_boxed_str());
    set.insert(interned);
    interned
}

/// A `'static` copy of `items`, allocated once per distinct list.
fn intern_str_list(items: &[String]) -> &'static [&'static str] {
    let key: Vec<&'static str> = items.iter().map(|s| intern_str(s)).collect();
    let mut map = INTERNED_STR_LISTS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(&interned) = map.get(&key) {
        return interned;
    }
    let interned: &'static [&'static str] = Box::leak(key.clone().into_boxed_slice());
    map.insert(key, interned);
    interned
}

/// Verify at load time that a plugin's declared `exports.assemble` export
/// matches one of the two ABI shapes described in
/// `foundation/spec/plugins/contracts/assemble.md` §5.3:
///
/// - v1 signature: `(i32) -> i32`  — `assemble(input_lp) -> output_lp`
/// - v3 signature: `(i32, i32) -> i32` — `assemble_typed(ctx, input_lp) -> status`
///
/// Any other signature — including a missing export, a non-function export,
/// mismatched arity, or non-i32 params/return — is rejected here with a
/// diagnostic that names the plugin, the offending export, and the contract
/// document. This gives plugin authors a precise failure at load time instead
/// of a generic "does not export assemble" error surfaced on the first call.
fn validate_assemble_export_signature(
    module: &Module,
    plugin_name: &str,
    export_name: &str,
) -> Result<()> {
    let export = module.get_export(export_name).ok_or_else(|| {
        anyhow!(
            "plugin `{}`: manifest declares `exports.assemble = \"{}\"` but the WASM module has \
             no export by that name. See foundation/spec/plugins/contracts/assemble.md §5.3.",
            plugin_name,
            export_name
        )
    })?;

    let func_ty = match export {
        ExternType::Func(ft) => ft,
        other => {
            return Err(anyhow!(
                "plugin `{}`: export `{}` is a {:?}, not a function. The assemble hook must be \
                 a function with one of the two signatures in \
                 foundation/spec/plugins/contracts/assemble.md §5.3.",
                plugin_name,
                export_name,
                other
            ));
        }
    };

    let params: Vec<ValType> = func_ty.params().collect();
    let results: Vec<ValType> = func_ty.results().collect();

    let all_i32 = |ts: &[ValType]| ts.iter().all(|t| matches!(t, ValType::I32));

    let matches_v1 =
        params.len() == 1 && all_i32(&params) && results.len() == 1 && all_i32(&results);
    let matches_v3 =
        params.len() == 2 && all_i32(&params) && results.len() == 1 && all_i32(&results);

    if matches_v1 || matches_v3 {
        return Ok(());
    }

    Err(anyhow!(
        "plugin `{}`: export `{}` has signature ({} params, {} results) which matches neither \
         the v1 assemble signature `(i32) -> i32` nor the v3 assemble_typed signature \
         `(i32, i32) -> i32`. See foundation/spec/plugins/contracts/assemble.md §5.3.",
        plugin_name,
        export_name,
        params.len(),
        results.len()
    ))
}

/// Verify at load time that a plugin's declared `exports.lint` export
/// matches the Contract 5 §2 signature `(i32) -> i32`:
///   - Parameter: LP-string pointer to the project-context JSON
///   - Return: LP-string pointer to the diagnostic-array (or error-object) JSON
///
/// Any other shape — missing export, non-function, arity mismatch, non-i32
/// params/return — is rejected here so plugin authors get a precise failure
/// at load time (surfaced as PLUGIN001) instead of a generic "does not
/// export lint_project" error on first invocation. Mirrors the assemble
/// validator above.
fn validate_lint_export_signature(
    module: &Module,
    plugin_name: &str,
    export_name: &str,
) -> Result<()> {
    let export = module.get_export(export_name).ok_or_else(|| {
        anyhow!(
            "plugin `{}`: manifest declares `exports.lint = \"{}\"` but the WASM module has \
             no export by that name. See foundation/spec/framework/contracts/lint-extension.md §2.",
            plugin_name,
            export_name
        )
    })?;

    let func_ty = match export {
        ExternType::Func(ft) => ft,
        other => {
            return Err(anyhow!(
                "plugin `{}`: export `{}` is a {:?}, not a function. The lint hook must be a \
                 function with signature `(i32) -> i32`. See \
                 foundation/spec/framework/contracts/lint-extension.md §2.",
                plugin_name,
                export_name,
                other
            ));
        }
    };

    let params: Vec<ValType> = func_ty.params().collect();
    let results: Vec<ValType> = func_ty.results().collect();
    let all_i32 = |ts: &[ValType]| ts.iter().all(|t| matches!(t, ValType::I32));

    if params.len() == 1 && all_i32(&params) && results.len() == 1 && all_i32(&results) {
        return Ok(());
    }

    Err(anyhow!(
        "plugin `{}`: export `{}` has signature ({} params, {} results) which does not match \
         the Contract 5 lint signature `(i32) -> i32`. See \
         foundation/spec/framework/contracts/lint-extension.md §2.",
        plugin_name,
        export_name,
        params.len(),
        results.len()
    ))
}

impl WasmPluginAdapter {
    /// Create a new WASM plugin adapter
    pub fn new(
        name: String,
        manifest: PluginManifest,
        module: Module,
        engine: Engine,
    ) -> Result<Self> {
        // `FrameworkPlugin` hands these out as `'static`. They are interned,
        // so a long-running process (language server, MCP server) that
        // loads the same plugins again does not leak a fresh copy each time.
        let name_cache = intern_str(&name);
        let version_cache = intern_str(&manifest.plugin.version);
        let description_cache = intern_str(&manifest.plugin.description);
        let handles_cache = intern_str_list(&manifest.handles.blocks);

        // Expression patterns come exclusively from the plugin's
        // `[handles] expressions` manifest declaration. The previous fallback
        // (a hardcoded `orm_dispatch_verbs` list scanned against the WASM
        // module exports for `expand_{verb}` functions) was removed in
        // Phase D S5 alongside the schema-driven bridge parity gate — all
        // shipped plugins (frame.data 3.0.1 and later) declare their full
        // `[handles].expressions` set in `plugin.toml`. A missing entry is
        // now a plugin manifest bug, not something the compiler silently
        // patches over.
        let expression_patterns_cache: Vec<String> = manifest.handles.expressions.clone();

        // Assemble hook signature validation.
        //
        // `foundation/spec/plugins/contracts/assemble.md` §5.3 requires the
        // compiler to detect the ABI shape (v1 `(i32) -> i32` vs v3
        // `(i32, i32) -> i32`) FROM THE EXPORT'S FUNCTION SIGNATURE at load
        // time, and to reject any other signature with a diagnostic that
        // names the assemble contract. Doing the check here (instead of
        // deferring to the first assemble() call) means malformed plugins
        // fail to load with a precise message, rather than silently loading
        // and failing later with a generic "does not export assemble" error.
        if let Some(export_name) = manifest.exports.assemble.as_deref() {
            validate_assemble_export_signature(&module, &name, export_name)?;
        }

        // Lint hook signature validation (Contract 5 Phase B).
        //
        // `foundation/spec/framework/contracts/lint-extension.md` §2 requires
        // the compiler to reject a `[exports].lint` that names an export the
        // plugin doesn't actually provide (dangling export) or whose signature
        // isn't `(i32) -> i32`. Doing the check here surfaces PLUGIN001 at
        // load time with a precise message pointing at the contract spec,
        // rather than a generic "does not export lint_project" error on first
        // `cln lint` invocation (which is the entry point wired in Cycle 2).
        if let Some(export_name) = manifest.exports.lint.as_deref() {
            validate_lint_export_signature(&module, &name, export_name)?;
        }

        // Build the v1 linker once — it sets up ~150 host functions and is
        // expensive to do per call.
        let linker = Self::setup_linker(&engine)?;

        Ok(Self {
            name,
            manifest,
            module,
            engine,
            handles_cache,
            expression_patterns_cache,
            name_cache,
            version_cache,
            description_cache,
            linker,
            typed_emission_linker: std::sync::OnceLock::new(),
            lint_linker: std::sync::OnceLock::new(),
            build_state: crate::plugins::new_build_state(),
            next_ctx_counter: AtomicI32::new(1),
            keywords_cache: std::sync::OnceLock::new(),
        })
    }

    /// Replace the adapter's per-build state container so multiple plugins
    /// loaded into the same registry share a single keystore. Called by
    /// `WasmPluginLoader::load_plugins` immediately after `new()`. Every
    /// store created afterwards carries the shared state.
    pub fn set_build_state(&mut self, state: crate::plugins::BuildState) {
        self.build_state = state;
    }

    /// Create a store for one plugin call around `state`, carrying the
    /// shared build state.
    ///
    /// The store inherits the engine's epoch-based interruption (see
    /// `wasm_loader::build_engine`) and sets a per-call deadline derived
    /// from `CLN_PLUGIN_TIMEOUT_SECS` (default 30 s). A plugin call that
    /// runs past the deadline traps with `wasm trap: interrupt`, surfacing
    /// a WASM backtrace instead of hanging the compiler indefinitely —
    /// the diagnostic the framework team asked for under
    /// COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS.
    /// Setting the env var to `0` disables the deadline.
    fn create_store_with_state(&self, mut state: PluginState) -> Store<PluginState> {
        state.build_state = std::sync::Arc::clone(&self.build_state);
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

    /// Create a store for a v1 call (no typed-emission or lint arena).
    fn create_store(&self) -> Store<PluginState> {
        self.create_store_with_state(PluginState::new())
    }

    /// Instantiate the plugin module in `store` for a call: returns the
    /// instance and its exported memory. `what` names the call in
    /// diagnostics.
    fn instantiate(
        &self,
        store: &mut Store<PluginState>,
        linker: &Linker<PluginState>,
        what: &str,
    ) -> Result<(wasmtime::Instance, Memory)> {
        let instance = linker.instantiate(&mut *store, &self.module).map_err(|e| {
            anyhow!(
                "failed to instantiate plugin `{}` for {}: {}",
                self.name,
                what,
                e
            )
        })?;
        let memory = instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| anyhow!("plugin `{}` does not export memory", self.name))?;
        Ok((instance, memory))
    }

    /// The typed-emission linker used by every v3 call (`expand_block_typed`,
    /// typed lifecycle slots, typed `process_html`, `assemble`). Built on
    /// first use and cached: the bridges keep no per-call state of their own
    /// — they reach the per-call arena through the store's `PluginState`.
    fn typed_emission_linker(&self) -> Result<&Linker<PluginState>> {
        if let Some(linker) = self.typed_emission_linker.get() {
            return Ok(linker);
        }
        let mut linker = Linker::new(&self.engine);
        // Register the full stdlib first so any Clean Language code inside a v3
        // plugin body (string.concat, env::print, float_to_string, math ops, etc.)
        // resolves at instantiation time. Without this, wasmtime rejects the module
        // with "unknown import" for every stdlib function the plugin references.
        //
        // Bug: f4b7d6977f05 (TYPED-EMISSION-LINKER-INCOMPLETE) — this call was
        // missing, causing frame.locale (and any other v3 plugin that calls stdlib
        // from its expand_block_typed body) to fail instantiation.
        Self::register_plugin_stdlib_functions(&mut linker)?;
        // Then overlay the typed-emission bridge functions on top of the stdlib.
        // These are the v3-specific handles: _emit_function, _stmt_block, _stmt_return, etc.
        crate::plugins::typed_emission::register_typed_emission_bridges(&mut linker)?;
        // Contract 5 Phase C: a plugin that exports both expand_block_typed
        // and lint_project imports the union of both surfaces. Stub the lint
        // bridges so instantiation succeeds; a wrong-context call traps with
        // a `BRIDGE-WRONG-CONTEXT` diagnostic. Prompt f9ee2728-8597-11f1-9d55-da25a95a496b.
        crate::plugins::bridge_stubs::register_lint_stubs(&mut linker, "expand")?;
        Ok(self.typed_emission_linker.get_or_init(|| linker))
    }

    /// Allocate the next monotonic ctx_handle for typed-emission.
    fn next_ctx_handle(&self) -> i32 {
        self.next_ctx_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// The lint linker used by Contract 5 `lint_project` calls. Built on
    /// first use and cached, like `typed_emission_linker`.
    ///
    /// Registers the read-only AST accessor bridges (see
    /// `foundation/spec/framework/contracts/lint-extension.md` §4) on top of
    /// the plugin stdlib. Kept separate from the v1 and typed-emission
    /// linkers so a lint call gets exactly the lint surface — no expand
    /// bridges, no typed-emission constructors. Each call still runs in a
    /// fresh store and instance, matching the spec's "fresh WASM instance
    /// per lint call" execution model.
    fn lint_linker(&self) -> Result<&Linker<PluginState>> {
        if let Some(linker) = self.lint_linker.get() {
            return Ok(linker);
        }
        let mut linker = Linker::new(&self.engine);
        // Plugins that write their `lint_project` in Clean Language will
        // reference stdlib functions (string ops, json helpers, math) the
        // same way expand-block bodies do. Register the full stdlib first
        // so any such import resolves at instantiation time — matching the
        // fix that landed for the typed-emission linker (fp f4b7d6977f05).
        Self::register_plugin_stdlib_functions(&mut linker)?;
        crate::plugins::lint::register_lint_bridges(&mut linker)?;
        // Contract 5 Phase C: see typed_emission_linker for rationale.
        crate::plugins::bridge_stubs::register_typed_emission_stubs(&mut linker, "lint")?;
        Ok(self.lint_linker.get_or_init(|| linker))
    }

    /// Contract 5 lint call. See
    /// `foundation/spec/framework/contracts/lint-extension.md` §3.
    ///
    /// Runs the plugin's `lint_project` export in a fresh WASM instance
    /// with the lint arena pre-installed and returns the raw JSON string
    /// the plugin wrote to memory. Cycle 2 hands that JSON up to the CLI
    /// verbatim; Cycle 3 parses it into structured diagnostics and routes
    /// them through the compiler renderer.
    ///
    /// Failure model (§5):
    /// - Trap or 30 s timeout → returns `Err(anyhow!("LINT001 ..."))` so
    ///   the caller emits LINT001 and continues with other plugins.
    /// - Malformed LP pointer or missing memory export → same as above.
    /// - Empty return (0 or empty LP) → returns `Ok(String::new())`;
    ///   caller treats this as "no diagnostics".
    ///
    /// The `program` snapshot must be the **pre-expansion** AST (see
    /// spec §6 — lint runs after typecheck but before block expansion).
    /// Passing a post-expansion program would make `_ast_list_blocks`
    /// return empty for every block name, since expansion consumes the
    /// `FrameworkBlock` statements.
    pub(crate) fn call_lint_project(
        &self,
        program: &crate::ast::Program,
        config_level: &str,
    ) -> Result<String> {
        // Only plugins that declare `[exports].lint` in their manifest
        // participate. Callers should have already filtered by this, but
        // guard here defensively so a stray call at the trait level
        // becomes a clean no-op.
        let export_name = match self.manifest.exports.lint.as_deref() {
            Some(n) => n,
            None => return Ok(String::new()),
        };

        let handle = self.next_ctx_handle();
        let arena = crate::plugins::lint::LintArena::new(handle, program.clone());
        debug_assert_eq!(
            arena.handle(),
            handle,
            "LintArena must carry the ctx handle it was constructed with"
        );
        let mut state = PluginState::new();
        state.lint_arena = Some(arena);
        let mut store = self.create_store_with_state(state);

        let (instance, memory) = self
            .instantiate(&mut store, self.lint_linker()?, "lint")
            .map_err(|e| anyhow!("LINT001: {}", e))?;

        // Build the project-context JSON (spec §3.1) and write it into
        // plugin memory as an LP string.
        let compiler_version = env!("CARGO_PKG_VERSION");
        let level = match config_level {
            "error" | "warning" | "info" => config_level,
            _ => "warning",
        };
        let ctx_json = format!(
            "{{\"project_ast_handle\":{},\"compiler_version\":\"{}\",\
             \"contract_version\":\"1.0.0\",\"runtime_abi_version\":\"1.0.0\",\
             \"config\":{{\"level\":\"{}\"}}}}",
            handle, compiler_version, level
        );
        let input_ptr = self.write_clean_string(&mut store, &instance, &memory, &ctx_json)?;

        // Call the plugin's `lint_project(input_lp) -> output_lp`.
        let lint_fn: TypedFunc<i32, i32> = instance
            .get_typed_func(&mut store, export_name)
            .map_err(|e| {
                anyhow!(
                    "LINT001: plugin `{}` does not export `{}` as (i32) -> i32: {}",
                    self.name,
                    export_name,
                    e
                )
            })?;

        let output_ptr = lint_fn.call(&mut store, input_ptr).map_err(|e| {
            if let Some(message) = plugin_raised_message(&e) {
                return anyhow!(
                    "LINT001: plugin `{}` raised an error during lint_project: {}",
                    self.name,
                    message
                );
            }
            let oom = store.data().oom_during_call.clone();
            anyhow!(
                "LINT001: plugin `{}` trapped during lint_project: {}{}",
                self.name,
                e,
                oom.as_deref()
                    .map(|s| format!(" [OOM: {}]", s))
                    .unwrap_or_default(),
            )
        })?;

        // A zero return means "no diagnostics" per spec — no LP header to read.
        if output_ptr == 0 {
            return Ok(String::new());
        }

        let data = memory.data(&store);
        match read_lp_from_data(data, output_ptr) {
            Some(bytes) => Ok(String::from_utf8_lossy(bytes).into_owned()),
            None => Err(anyhow!(
                "LINT001: plugin `{}` returned a malformed LP pointer ({}) from lint_project",
                self.name,
                output_ptr
            )),
        }
    }

    /// Run the plugin's `[exports].validate` on a block (plugin-contract.md
    /// §5.2). The export takes the same `(block_name, attributes, body)`
    /// strings as the block's expand export and returns a string: empty when
    /// the block is valid, otherwise the error message. Returns `None` when
    /// the block is valid or the plugin declares no validate export.
    fn call_validate(&self, block: &FrameworkBlock) -> Result<Option<String>> {
        let Some(export_name) = self.manifest.exports.validate.as_deref() else {
            return Ok(None);
        };
        let block_name = block.name.trim_end_matches(':');
        let dispatch_version = self.manifest.resolve_block_dispatch(block_name).version;
        // plugin-contract.md §5.2: validate takes exactly what the block's
        // expand export receives.
        let inputs = expand_inputs(block_name, block, dispatch_version);
        let (attributes, body) = (inputs.attributes.as_str(), inputs.body.as_str());
        let what = format!("validate block `{}`", block_name);
        let mut message = String::new();
        let mut run = |store: &mut Store<PluginState>,
                       instance: &wasmtime::Instance,
                       memory: &Memory|
         -> Result<wasmtime::Result<i32>> {
            let validate: TypedFunc<(i32, i32, i32), i32> = instance
                .get_typed_func(&mut *store, export_name)
                .map_err(|e| {
                    anyhow!(
                        "plugin `{}` declares [exports].validate = \"{}\" but does not export \
                         it as (block_name, attributes, body) -> string: {}",
                        self.name,
                        export_name,
                        e
                    )
                })?;
            let block_name_ptr = self.write_clean_string(store, instance, memory, block_name)?;
            let attributes_ptr = self.write_clean_string(store, instance, memory, attributes)?;
            let body_ptr = self.write_clean_string(store, instance, memory, body)?;
            let result = validate.call(&mut *store, (block_name_ptr, attributes_ptr, body_ptr));
            if let Ok(ptr) = result {
                // A Clean string is never at address 0: a 0 result is the
                // plugin's allocator failing, not an empty message.
                if ptr == 0 {
                    let oom = store.data().oom_during_call.clone();
                    return Err(anyhow!(
                        "plugin `{}` {} returned a null string pointer{}",
                        self.name,
                        what,
                        oom.map(|s| format!(" [OOM: {}]", s)).unwrap_or_default()
                    ));
                }
                let bytes = self.read_result(store, memory, ptr)?;
                message = String::from_utf8_lossy(&bytes).trim().to_string();
            }
            Ok(result.map(|_| 0))
        };
        if dispatch_version >= 3 {
            let ctx = self.next_ctx_handle();
            let arena = crate::plugins::typed_emission::EmitArena::new(ctx);
            let (arena, _) = self.run_typed_call(&what, arena, run)?;
            // Diagnostics the plugin emitted while validating (typed-emission
            // §5) are routed like an expansion's; an error-severity one
            // fails validation.
            let (_, diagnostics, saw_error) = arena.finish();
            let errors = self.route_typed_diagnostics(&what, &diagnostics);
            if saw_error && message.is_empty() {
                message = errors.unwrap_or_else(|| {
                    format!("plugin `{}` reported an error during {}", self.name, what)
                });
            }
        } else {
            let mut store = self.create_store();
            let (instance, memory) = self.instantiate(&mut store, &self.linker, &what)?;
            run(&mut store, &instance, &memory)?
                .map_err(|e| anyhow!("{}", describe_plugin_trap_with_oom(&e, &what, None)))?;
        }
        Ok((!message.is_empty()).then_some(message))
    }

    /// Call the plugin's `[exports].get_keywords` export, `() -> string`,
    /// which returns a JSON array of keyword strings
    /// (plugin-contract.md §1.1). Empty when the plugin declares none.
    fn call_get_keywords(&self) -> Result<Vec<String>> {
        let Some(export_name) = self.manifest.exports.get_keywords.as_deref() else {
            return Ok(Vec::new());
        };
        let what = "get_keywords";
        let mut json = String::new();
        let mut run = |store: &mut Store<PluginState>,
                       instance: &wasmtime::Instance,
                       memory: &Memory|
         -> Result<wasmtime::Result<i32>> {
            let get_keywords: TypedFunc<(), i32> = instance
                .get_typed_func(&mut *store, export_name)
                .map_err(|e| {
                    anyhow!(
                        "plugin `{}` declares [exports].get_keywords = \"{}\" but does not \
                         export it as () -> string: {}",
                        self.name,
                        export_name,
                        e
                    )
                })?;
            let result = get_keywords.call(&mut *store, ());
            if let Ok(ptr) = result {
                let bytes = self.read_result(store, memory, ptr)?;
                json = String::from_utf8_lossy(&bytes).into_owned();
            }
            Ok(result.map(|_| 0))
        };
        if self.manifest.opts_into_typed_emission() {
            let arena = crate::plugins::typed_emission::EmitArena::new(self.next_ctx_handle());
            self.run_typed_call(what, arena, run)?;
        } else {
            let mut store = self.create_store();
            let (instance, memory) = self.instantiate(&mut store, &self.linker, what)?;
            run(&mut store, &instance, &memory)?
                .map_err(|e| anyhow!("{}", describe_plugin_trap(&e, what)))?;
        }
        serde_json::from_str(&json).map_err(|e| {
            anyhow!(
                "plugin `{}` get_keywords did not return a JSON array of strings: {} — raw: {}",
                self.name,
                e,
                diagnostic_prefix(&json)
            )
        })
    }

    /// Plugin Contracts v3 typed expansion. Called when
    /// `dispatch.version >= 3` in `call_expand_full`.
    ///
    /// Invokes `expand_block_typed(ctx, block_name_lp, attrs_lp, body_lp) -> i32`
    /// with a fresh `EmitArena` and returns the accumulated `PluginExpansion`.
    fn call_expand_typed(
        &self,
        export_name: &str,
        block: &FrameworkBlock,
        block_name: &str,
        attributes: &str,
        body: &str,
    ) -> Result<PluginExpansion> {
        let what = format!("typed emission of block `{}`", block_name);
        let ctx = self.next_ctx_handle();
        let mut arena = crate::plugins::typed_emission::EmitArena::new(ctx);
        arena.fragment_origin = Some(crate::plugins::typed_emission::FragmentOrigin::new(
            &self.name,
            Some(block),
        ));
        let (arena, status) = self.run_typed_call(&what, arena, |store, instance, memory| {
            let block_name_ptr = self.write_clean_string(store, instance, memory, block_name)?;
            let attributes_ptr = self.write_clean_string(store, instance, memory, attributes)?;
            let body_ptr = self.write_clean_string(store, instance, memory, body)?;
            let expand: TypedFunc<(i32, i32, i32, i32), i32> = instance
                .get_typed_func(&mut *store, export_name)
                .map_err(|e| {
                    anyhow!(
                        "typed-emission: plugin `{}` does not export `{}`: {}",
                        self.name,
                        export_name,
                        e
                    )
                })?;
            Ok(expand.call(&mut *store, (ctx, block_name_ptr, attributes_ptr, body_ptr)))
        })?;
        let expansion = self.finish_typed_expansion(&what, arena, status)?;
        tracing::debug!(
            target: "compiler::plugins::arena",
            plugin = %self.name,
            block = block_name,
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
    /// The plugin's typed slot export is invoked as
    /// `(ctx, build_context_lp) -> i32` with the serialized build context;
    /// see `call_typed_json_export`.
    ///
    /// Per typed-emission.md §7: every v1 lifecycle slot has a typed-emission
    /// counterpart with the same call site but a different ABI shape.
    fn call_lifecycle_slot_typed(
        &self,
        slot_name: &str,
        export_name: &str,
        context: &crate::plugins::BuildContext,
    ) -> Result<PluginExpansion> {
        let context_json = serde_json::to_string(context).map_err(|e| {
            anyhow!(
                "typed-emission: failed to serialize build context for slot `{}`: {}",
                slot_name,
                e
            )
        })?;
        self.call_typed_json_export(
            &format!("lifecycle slot `{}`", slot_name),
            export_name,
            &context_json,
        )
    }

    /// Typed-emission variant of `process_html`, used when the plugin opts
    /// into typed emission. The export is invoked as `(ctx, ctx_json_lp) -> i32`
    /// where `ctx_json` is a JSON object whose string fields `html`, `path`,
    /// `registry_json` and `companion_json` carry the same four inputs the v1
    /// string ABI passes positionally. The page is returned as the emitted
    /// `PluginExpansion`.
    fn call_process_html_typed(
        &self,
        export_name: &str,
        html: &str,
        path: &str,
        registry_json: &str,
        companion_json: &str,
    ) -> Result<PluginExpansion> {
        let ctx_json = serde_json::json!({
            "html": html,
            "path": path,
            "registry_json": registry_json,
            "companion_json": companion_json,
        })
        .to_string();
        self.call_typed_json_export(
            &format!("process_html for `{}`", path),
            export_name,
            &ctx_json,
        )
    }

    /// Shared typed-emission call shape for exports taking `(ctx, json_lp) -> i32`.
    ///
    /// The export is called with the JSON input written into plugin memory
    /// under a fresh `EmitArena`, and the accumulated `PluginExpansion` is
    /// returned. `what` names the call in diagnostics (e.g. "lifecycle slot
    /// `program_init`").
    fn call_typed_json_export(
        &self,
        what: &str,
        export_name: &str,
        input_json: &str,
    ) -> Result<PluginExpansion> {
        let ctx = self.next_ctx_handle();
        let arena = crate::plugins::typed_emission::EmitArena::new(ctx);
        let (arena, status) = self.run_typed_call(what, arena, |store, instance, memory| {
            let input_ptr = self.write_clean_string(store, instance, memory, input_json)?;
            let export: TypedFunc<(i32, i32), i32> = instance
                .get_typed_func(&mut *store, export_name)
                .map_err(|e| {
                    anyhow!(
                        "typed-emission: plugin `{}` does not export {} as `{}(ctx, json) -> i32`: {}",
                        self.name,
                        what,
                        export_name,
                        e
                    )
                })?;
            Ok(export.call(&mut *store, (ctx, input_ptr)))
        })?;
        self.finish_typed_expansion(what, arena, status)
    }

    /// Run one typed-emission export call in a fresh store holding `arena`.
    ///
    /// The plugin is instantiated with the typed-emission linker and `call`
    /// writes the inputs into plugin memory (the outer `Result`) and invokes
    /// the export (the inner `Result`, carrying its status). A trap is
    /// reported as PLUGIN009 with the trap kind and any host-bridge OOM
    /// context. Returns the arena (with everything the plugin
    /// emitted into it) and the export's status.
    fn run_typed_call(
        &self,
        what: &str,
        arena: crate::plugins::typed_emission::EmitArena,
        call: impl FnOnce(
            &mut Store<PluginState>,
            &wasmtime::Instance,
            &Memory,
        ) -> Result<wasmtime::Result<i32>>,
    ) -> Result<(crate::plugins::typed_emission::EmitArena, i32)> {
        let mut arena = arena;
        if arena.fragment_origin.is_none() {
            // No user block: code parsed from fragments is the plugin's.
            arena.fragment_origin = Some(crate::plugins::typed_emission::FragmentOrigin::new(
                &self.name, None,
            ));
        }
        let mut state = PluginState::new();
        state.emit_arena = Some(arena);
        let mut store = self.create_store_with_state(state);
        let (instance, memory) = self
            .instantiate(&mut store, self.typed_emission_linker()?, what)
            .map_err(|e| anyhow!("typed-emission: {}", e))?;
        let status = call(&mut store, &instance, &memory)?.map_err(|e| {
            let oom = store.data().oom_during_call.clone();
            anyhow!(
                "PLUGIN009: plugin `{}`: {}",
                self.name,
                describe_plugin_trap_with_oom(&e, &format!("typed {}", what), oom.as_deref())
            )
        })?;
        let arena = store.data_mut().emit_arena.take().ok_or_else(|| {
            anyhow!(
                "typed-emission: the emit arena was taken during {} — cross-call arena reuse",
                what
            )
        })?;
        Ok((arena, status))
    }

    /// Drain a typed-emission arena into its `PluginExpansion`, routing the
    /// plugin's diagnostics. A non-zero export status or any error-severity
    /// diagnostic fails the call.
    fn finish_typed_expansion(
        &self,
        what: &str,
        arena: crate::plugins::typed_emission::EmitArena,
        status: i32,
    ) -> Result<PluginExpansion> {
        let (expansion, diagnostics, saw_error) = arena.finish();
        let error_summary = self.route_typed_diagnostics(what, &diagnostics);
        if status != 0 || saw_error {
            return Err(anyhow!(
                "plugin `{}` failed typed {} (return={}, errors: {})",
                self.name,
                what,
                status,
                error_summary.as_deref().unwrap_or("(none)")
            ));
        }
        Ok(expansion)
    }

    /// Log a typed-emission call's diagnostics on the compiler diagnostic
    /// channel and return the joined error-severity messages, if any.
    fn route_typed_diagnostics(
        &self,
        what: &str,
        diagnostics: &[crate::plugins::typed_emission::error::EmitDiagnostic],
    ) -> Option<String> {
        for diag in diagnostics {
            if diag.severity >= 2 {
                tracing::error!(
                    target: "compiler::plugins::typed_emission",
                    plugin = %self.name,
                    call = what,
                    code = %diag.code,
                    message = %diag.message,
                    span = %diag.span_json,
                    "plugin typed-emission error"
                );
            } else {
                tracing::warn!(
                    target: "compiler::plugins::typed_emission",
                    plugin = %self.name,
                    call = what,
                    code = %diag.code,
                    message = %diag.message,
                    span = %diag.span_json,
                    "plugin typed-emission warning"
                );
            }
        }
        let errors: Vec<String> = diagnostics
            .iter()
            .filter(|d| d.severity >= 2)
            .map(|d| format!("[{}] {}", d.code, d.message))
            .collect();
        (!errors.is_empty()).then(|| errors.join("; "))
    }

    /// Register all stdlib host functions that any plugin (v1 or v3) may import.
    ///
    /// The same surface backs every linker (v1, typed emission, lint). Some
    /// groups are real implementations a plugin can use while it expands a
    /// block; the rest are stubs that only satisfy the import, because the
    /// sandbox has no network, file system, server or request context:
    ///
    /// - real: `register_env_core_functions` (print, conversions, JSON,
    ///   `error`), `register_string_dot_functions`,
    ///   `register_string_underscore_functions`, `register_list_functions`,
    ///   `register_memory_runtime_functions`, `register_math_functions`,
    ///   `register_build_state_bridges` (Plugin Contracts v2+)
    /// - stubs returning an empty string or 0: `register_http_client_functions`
    ///   (http_get/post/...), `register_http_server_functions`,
    ///   `register_dev_capture_functions`, `register_request_context_functions`,
    ///   `register_file_functions`, `register_http_auth_stubs`
    fn register_plugin_stdlib_functions(linker: &mut Linker<PluginState>) -> Result<()> {
        // Core I/O and conversions (print, float_to_string, string.concat, etc.)
        Self::register_env_core_functions(linker)?;
        // string.* dot-namespace functions (slice, substring, indexOf, replace, …)
        Self::register_string_dot_functions(linker)?;
        // string_* underscore-namespace functions (string_compare, _str_eq, …)
        Self::register_string_underscore_functions(linker)?;
        // list.* functions (push_f64, …)
        Self::register_list_functions(linker)?;
        // Memory / allocator runtime helpers
        Self::register_memory_runtime_functions(linker)?;
        // Outbound HTTP client (http_get/post/…)
        Self::register_http_client_functions(linker)?;
        // HTTP server stubs (_http_route, _http_listen — zero returns)
        Self::register_http_server_functions(linker)?;
        // Dev capture stub (_dev_snapshot — LP empty string, real impl in clean-server)
        Self::register_dev_capture_functions(linker)?;
        // Request context stubs (_req_param, _req_query, … — zero/empty returns)
        Self::register_request_context_functions(linker)?;
        // File I/O stubs
        Self::register_file_functions(linker)?;
        // Math namespace (Math.sin, Math.cos, Math.sqrt, Math.random, …)
        Self::register_math_functions(linker)?;
        // Auth / session / redirect stubs (_req_cookie, _http_redirect, _session_*, …)
        Self::register_http_auth_stubs(linker)?;
        // Build-state bridges (_build_state_set, _build_state_get — Plugin Contracts v2+)
        register_build_state_bridges(linker)?;
        Ok(())
    }

    /// Set up the v1 linker with host functions.
    /// Provides the full Clean Language runtime environment for v1 plugins.
    /// Delegates to `register_plugin_stdlib_functions` so the stdlib surface is
    /// always in sync with the v3 linker.
    ///
    /// After registration, verifies the linker's `(module, name)` surface
    /// matches `foundation/platform-architecture/runtime-abi/v1.toml` — the
    /// canonical catalog authored in Phase D S2. Any drift (missing or extra
    /// bridge) fails plugin adapter construction with a diagnostic pointing
    /// at the offending name(s).
    fn setup_linker(engine: &Engine) -> Result<Linker<PluginState>> {
        let mut linker = Linker::new(engine);
        Self::register_plugin_stdlib_functions(&mut linker)?;

        // Parity gate: verify the registered bridge surface matches
        // runtime-abi/v1.toml. Uses an ephemeral store solely for
        // `Linker::iter` (wasmtime 10 requires it) — the store is dropped
        // immediately after.
        let mut ephemeral = Store::new(engine, PluginState::new());
        let registered: Vec<(String, String)> = linker
            .iter(&mut ephemeral)
            .map(|(module, name, _)| (module.to_string(), name.to_string()))
            .collect();
        drop(ephemeral);

        if let Err(msg) =
            super::runtime_abi_schema::verify_registrations_against_schema(&registered)
        {
            return Err(anyhow!(
                "Runtime ABI v1 parity failure: {}. See \
                 foundation/platform-architecture/runtime-abi/v1.toml.",
                msg
            ));
        }

        // v1 calls (process_html, v1 lifecycle slots, v1 expansion)
        // instantiate plugins that may also export expand_block_typed /
        // lint_project and therefore import those bridge surfaces. Stub them
        // (after the ABI parity gate, which covers only the v1 surface) so
        // instantiation succeeds; a call from the wrong context traps with
        // BRIDGE-WRONG-CONTEXT.
        crate::plugins::bridge_stubs::register_typed_emission_stubs(&mut linker, "v1 plugin call")?;
        crate::plugins::bridge_stubs::register_lint_stubs(&mut linker, "v1 plugin call")?;

        Ok(linker)
    }

    // =========================================
    // ENV NAMESPACE - Core I/O and conversions
    // =========================================
    fn register_env_core_functions(linker: &mut Linker<PluginState>) -> Result<()> {
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
                write_clean_string(&mut caller, b"")
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

                write_clean_string(&mut caller, bytes)
            },
        )?;

        // env.float_to_string - Convert float to string
        linker.func_wrap(
            "env",
            "float_to_string",
            |mut caller: Caller<'_, PluginState>, value: f64| -> i32 {
                let s = value.to_string();
                let bytes = s.as_bytes();

                write_clean_string(&mut caller, bytes)
            },
        )?;

        // env.bool_to_string - Convert boolean to string
        linker.func_wrap(
            "env",
            "bool_to_string",
            |mut caller: Caller<'_, PluginState>, value: i32| -> i32 {
                let s = if value != 0 { "true" } else { "false" };
                let bytes = s.as_bytes();

                write_clean_string(&mut caller, bytes)
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

                // Read both length-prefixed strings; an unreadable pointer
                // yields 0 (null result) rather than a host panic.
                let data = memory.data(&caller);
                let (Some(s1), Some(s2)) = (
                    read_lp_from_data(data, str1_ptr),
                    read_lp_from_data(data, str2_ptr),
                ) else {
                    return 0;
                };
                let mut result = Vec::with_capacity(s1.len() + s2.len());
                result.extend_from_slice(s1);
                result.extend_from_slice(s2);

                // Allocate and write result (length-prefixed)
                write_clean_string(&mut caller, &result)
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
                    // Empty delimiter: one part per character (`split("")`
                    // yields the characters between empty leading and
                    // trailing parts, which are dropped).
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

                let heap = SharedHeap::of_caller(&mut caller);
                let list_ptr = host_allocate(&mut caller, heap, total, PluginState::allocate);
                if list_ptr == 0 {
                    return 0;
                }

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
    fn register_string_dot_functions(linker: &mut Linker<PluginState>) -> Result<()> {
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
                // Lexicographic order per function-registry.toml: -1 / 0 / 1.
                match s1.cmp(s2) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
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
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                // Read all three strings first
                let (string_val, search, replace) = {
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
                Ok(write_clean_string(&mut caller, result_bytes))
            },
        )?;

        // env.string_repeat - Repeat a string N times
        // string_repeat(str_ptr: i32, str_len: i32, count: i32) -> i32
        linker.func_wrap(
            "env",
            "string_repeat",
            |mut caller: Caller<'_, PluginState>,
             str_ptr: i32,
             _str_len: i32,
             count: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let Some(unit) =
                    read_lp_from_data(memory.data(&caller), str_ptr).map(<[u8]>::to_vec)
                else {
                    return Ok(0);
                };
                Ok(write_repeated_lp_string(
                    &mut caller,
                    &unit,
                    count.max(0) as usize,
                ))
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
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let Some(bytes) = read_lp_from_data(memory.data(&caller), str_ptr) else {
                    return Ok(0);
                };
                let s = String::from_utf8_lossy(bytes).to_string();
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
                Ok(if matched { 1 } else { 0 })
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
                write_clean_string(&mut caller, result_bytes)
            },
        )?;

        // env.string.substring - Extract a substring from a string
        // string.substring(ptr: i32, start: i32, end: i32) -> i32
        linker.func_wrap(
            "env",
            "string.substring",
            |mut caller: Caller<'_, PluginState>,
             ptr: i32,
             start: i32,
             end: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    let Some(bytes) = read_lp_from_data(data, ptr) else {
                        return Ok(0);
                    };
                    bytes.to_vec()
                };

                // Byte offsets, clamped to the string and ordered. Slicing bytes
                // (not a `str`) means an offset inside a multi-byte character
                // cannot panic.
                let str_len = string_val.len() as i32;
                let actual_start = start.clamp(0, str_len) as usize;
                let actual_end = (end.clamp(0, str_len) as usize).max(actual_start);
                Ok(write_clean_string(
                    &mut caller,
                    &string_val[actual_start..actual_end],
                ))
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
                read_lp_from_data(data, ptr).map_or(0, |bytes| bytes.len() as i32)
            },
        )?;

        // env.string.indexOf - Find the first occurrence of a substring
        linker.func_wrap(
            "env",
            "string.indexOf",
            |mut caller: Caller<'_, PluginState>,
             string_ptr: i32,
             search_ptr: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let (string_val, search) = {
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };
                Ok(match string_val.find(&search) {
                    Some(idx) => idx as i32,
                    None => -1,
                })
            },
        )?;

        // env.string.lastIndexOf - Find the last occurrence of a substring
        linker.func_wrap(
            "env",
            "string.lastIndexOf",
            |mut caller: Caller<'_, PluginState>,
             string_ptr: i32,
             search_ptr: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let (string_val, search) = {
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };
                Ok(match string_val.rfind(&search) {
                    Some(idx) => idx as i32,
                    None => -1,
                })
            },
        )?;

        // env.string.charAt - Get character at index
        linker.func_wrap(
            "env",
            "string.charAt",
            |mut caller: Caller<'_, PluginState>, ptr: i32, index: i32| -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
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
                Ok(write_clean_string(&mut caller, result_bytes))
            },
        )?;

        // env.string.charCodeAt - Get character code at index
        linker.func_wrap(
            "env",
            "string.charCodeAt",
            |mut caller: Caller<'_, PluginState>, ptr: i32, index: i32| -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };
                Ok(if index >= 0 && (index as usize) < string_val.len() {
                    string_val.chars().nth(index as usize).unwrap_or('\0') as i32
                } else {
                    -1
                })
            },
        )?;

        // env.string.startsWith - Check if string starts with prefix
        linker.func_wrap(
            "env",
            "string.startsWith",
            |mut caller: Caller<'_, PluginState>,
             string_ptr: i32,
             prefix_ptr: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let (string_val, prefix) = {
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, prefix_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };
                Ok(if string_val.starts_with(&prefix) {
                    1
                } else {
                    0
                })
            },
        )?;

        // env.string.endsWith - Check if string ends with suffix
        linker.func_wrap(
            "env",
            "string.endsWith",
            |mut caller: Caller<'_, PluginState>,
             string_ptr: i32,
             suffix_ptr: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let (string_val, suffix) = {
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, suffix_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };
                Ok(if string_val.ends_with(&suffix) { 1 } else { 0 })
            },
        )?;

        // env.string.includes - Check if string contains substring
        linker.func_wrap(
            "env",
            "string.includes",
            |mut caller: Caller<'_, PluginState>,
             string_ptr: i32,
             search_ptr: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let (string_val, search) = {
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };
                Ok(if string_val.contains(&search) { 1 } else { 0 })
            },
        )?;

        // env.string.toUpperCase - Convert to uppercase
        linker.func_wrap(
            "env",
            "string.toUpperCase",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = string_val.to_uppercase();
                let result_bytes = result.as_bytes();
                Ok(write_clean_string(&mut caller, result_bytes))
            },
        )?;

        // env.string.toLowerCase - Convert to lowercase
        linker.func_wrap(
            "env",
            "string.toLowerCase",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = string_val.to_lowercase();
                let result_bytes = result.as_bytes();
                Ok(write_clean_string(&mut caller, result_bytes))
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
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let (string_val, search, replace) = {
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
                Ok(write_clean_string(&mut caller, result_bytes))
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
            |mut caller: Caller<'_, PluginState>,
             ptr: i32,
             _str_len: i32,
             count: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let count = count.max(0) as usize;
                let result = string_val.repeat(count);
                let result_bytes = result.as_bytes();
                Ok(write_clean_string(&mut caller, result_bytes))
            },
        )?;

        // env.string.reverse - Reverse the string
        linker.func_wrap(
            "env",
            "string.reverse",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result: String = string_val.chars().rev().collect();
                let result_bytes = result.as_bytes();
                Ok(write_clean_string(&mut caller, result_bytes))
            },
        )?;

        Ok(())
    }

    // =========================================
    // UNDERSCORE-STYLE STRING ALIASES
    // For compatibility with different WASM naming conventions
    // =========================================
    fn register_string_underscore_functions(linker: &mut Linker<PluginState>) -> Result<()> {
        // env.string_substring - Alias for string.substring
        linker.func_wrap(
            "env",
            "string_substring",
            |mut caller: Caller<'_, PluginState>,
             ptr: i32,
             start: i32,
             end: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    let Some(bytes) = read_lp_from_data(data, ptr) else {
                        return Ok(0);
                    };
                    bytes.to_vec()
                };

                // Byte offsets, as in `string.substring`.
                let str_len = string_val.len() as i32;
                let actual_start = start.clamp(0, str_len) as usize;
                let actual_end = (end.clamp(0, str_len) as usize).max(actual_start);
                Ok(write_clean_string(
                    &mut caller,
                    &string_val[actual_start..actual_end],
                ))
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
                read_lp_from_data(data, ptr).map_or(0, |bytes| bytes.len() as i32)
            },
        )?;

        // env.string_indexOf - Alias for string.indexOf
        linker.func_wrap(
            "env",
            "string_indexOf",
            |mut caller: Caller<'_, PluginState>,
             string_ptr: i32,
             search_ptr: i32|
             -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let (string_val, search) = {
                    let data = memory.data(&caller);
                    let s1 =
                        String::from_utf8_lossy(read_lp_from_data(data, string_ptr).unwrap_or(&[]))
                            .to_string();
                    let s2 =
                        String::from_utf8_lossy(read_lp_from_data(data, search_ptr).unwrap_or(&[]))
                            .to_string();

                    (s1, s2)
                };
                Ok(match string_val.find(&search) {
                    Some(idx) => idx as i32,
                    None => -1,
                })
            },
        )?;

        // env.string_charAt - Alias for string.charAt
        linker.func_wrap(
            "env",
            "string_charAt",
            |mut caller: Caller<'_, PluginState>, ptr: i32, index: i32| -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
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
                Ok(write_clean_string(&mut caller, result_bytes))
            },
        )?;

        // env.string_charCodeAt - Alias for string.charCodeAt
        linker.func_wrap(
            "env",
            "string_charCodeAt",
            |mut caller: Caller<'_, PluginState>, ptr: i32, index: i32| -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };
                Ok(if index >= 0 && (index as usize) < string_val.len() {
                    string_val.chars().nth(index as usize).unwrap_or('\0') as i32
                } else {
                    -1
                })
            },
        )?;

        // env.string_toUpperCase - Alias for string.toUpperCase
        linker.func_wrap(
            "env",
            "string_toUpperCase",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = string_val.to_uppercase();
                let result_bytes = result.as_bytes();
                Ok(write_clean_string(&mut caller, result_bytes))
            },
        )?;

        // env.string_toLowerCase - Alias for string.toLowerCase
        linker.func_wrap(
            "env",
            "string_toLowerCase",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> wasmtime::Result<i32> {
                let memory = caller_memory(&mut caller)?;
                let string_val = {
                    let data = memory.data(&caller);
                    String::from_utf8_lossy(read_lp_from_data(data, ptr).unwrap_or(&[])).to_string()
                };

                let result = string_val.to_lowercase();
                let result_bytes = result.as_bytes();
                Ok(write_clean_string(&mut caller, result_bytes))
            },
        )?;

        Ok(())
    }

    // =========================================
    // LIST NAMESPACE - List/array operations
    // =========================================
    fn register_list_functions(linker: &mut Linker<PluginState>) -> Result<()> {
        // env.list.push_f64(list, value) -> list — append an f64 element.
        // Returns the list pointer the caller must use from then on: the
        // same list when it had room, or a grown copy.
        linker.func_wrap(
            "env",
            "list.push_f64",
            |caller: Caller<'_, PluginState>, list_ptr: i32, value: f64| -> i32 {
                list_push_f64(caller, list_ptr, value)
            },
        )?;

        Ok(())
    }

    // =========================================
    // MEMORY_RUNTIME NAMESPACE - Memory management
    // =========================================
    fn register_memory_runtime_functions(linker: &mut Linker<PluginState>) -> Result<()> {
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

                let heap = SharedHeap::of_caller(&mut caller);
                let ptr = host_allocate(&mut caller, heap, size, PluginState::allocate);
                let state = caller.data_mut();
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

        // memory_runtime.mem_scope_push - Push scope mark for arena allocation.
        //
        // Mirrors `wasmtime_runner`'s implementation: save the host bump
        // pointer AND the WASM-side `__heap_ptr` global. Without saving
        // `__heap_ptr`, the paired `mem_scope_pop` cannot rewind WASM-side
        // `malloc` allocations, and later reads of arena objects
        // (list<string> elements, substrings, etc.) hit stale pointers whose
        // backing memory has been reused — corrupting output silently, or
        // producing `substring(start > end)` traps at wasm function 207 when
        // the reused bytes decode as a bogus length.
        // PLG001 (fp 81ee7e44) / framework fp d9f480ae.
        linker.func_wrap(
            "memory_runtime",
            "mem_scope_push",
            |mut caller: Caller<'_, PluginState>| {
                let host_mark = open_heap_scope(&mut caller);
                let heap_mark = match SharedHeap::of_caller(&mut caller) {
                    Some(heap) => heap.top(&mut caller.as_context_mut()),
                    None => host_mark,
                };
                caller
                    .data_mut()
                    .mem_scope_marks
                    .push((host_mark, heap_mark));
            },
        )?;

        // memory_runtime.mem_scope_pop - Pop the top scope mark and rewind
        // both the host bump pointer and the WASM `__heap_ptr` global to the
        // saved values, reclaiming every allocation made between the paired
        // push and pop. Defensive: no-op if the stack is empty (e.g. a
        // plugin trap mid-scope left it unbalanced).
        // PLG001 (fp 81ee7e44) / framework fp d9f480ae.
        linker.func_wrap(
            "memory_runtime",
            "mem_scope_pop",
            |mut caller: Caller<'_, PluginState>| {
                if let Some((host_mark, heap_mark)) = caller.data_mut().mem_scope_marks.pop() {
                    close_heap_scope(&mut caller, host_mark, heap_mark);
                }
            },
        )?;

        // env._arena_scope_push — push a save mark onto the host arena stack.
        //
        // Returns the new depth (always >= 1) as an opaque handle. The plugin
        // must pass this handle to the paired `_arena_scope_pop` call.
        //
        // Called by HIR-rewritten loops to bracket per-iteration scratch
        // allocations. Every allocation made between a push and the
        // corresponding pop — by a host bridge or by the plugin's own
        // allocator, which share one heap — is reclaimed in O(1) by pop,
        // as in `wasmtime_runner`.
        linker.func_wrap(
            "env",
            "_arena_scope_push",
            |mut caller: Caller<'_, PluginState>| -> i32 {
                open_heap_scope(&mut caller);
                let state = caller.data_mut();
                // Push the (joined) bump pointer onto the mark stack.
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
                // mid-scope) by truncating; the outermost popped mark wins.
                let mut reset_to = None;
                while state.arena_marks.len() > target_depth {
                    reset_to = state.arena_marks.pop();
                }
                if let Some(mark) = reset_to {
                    close_heap_scope(&mut caller, mark, mark);
                }
                // Trace-level: emit reclaim trajectory so Bug A's diagnosis
                // can see whether mark/reset actually shrinks the arena.
                let state = caller.data();
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
    fn register_http_client_functions(linker: &mut Linker<PluginState>) -> Result<()> {
        // env.http_get
        linker.func_wrap(
            "env",
            "http_get",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                // Return empty response
                write_clean_string(&mut caller, b"")
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
             -> i32 { write_clean_string(&mut caller, b"") },
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
             -> i32 { write_clean_string(&mut caller, b"") },
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
             -> i32 { write_clean_string(&mut caller, b"") },
        )?;

        // env.http_delete
        linker.func_wrap(
            "env",
            "http_delete",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;

        // env.http_head
        linker.func_wrap(
            "env",
            "http_head",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;

        // env.http_options
        linker.func_wrap(
            "env",
            "http_options",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                write_clean_string(&mut caller, b"")
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
             -> i32 { write_clean_string(&mut caller, b"") },
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
             -> i32 { write_clean_string(&mut caller, b"") },
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
             -> i32 { write_clean_string(&mut caller, b"") },
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
             -> i32 { write_clean_string(&mut caller, b"") },
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
             -> i32 { write_clean_string(&mut caller, b"") },
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
             -> i32 { write_clean_string(&mut caller, b"") },
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

        // env.http_encode_url — stub: plugins never make HTTP calls while
        // expanding; returns an empty string.
        linker.func_wrap(
            "env",
            "http_encode_url",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;

        // env.http_decode_url — stub, empty string (see http_encode_url).
        linker.func_wrap(
            "env",
            "http_decode_url",
            |mut caller: Caller<'_, PluginState>, _url_ptr: i32, _url_len: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;

        // env.http_build_query
        linker.func_wrap(
            "env",
            "http_build_query",
            |mut caller: Caller<'_, PluginState>, _params_ptr: i32, _params_len: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;

        Ok(())
    }

    // =========================================
    // HTTP SERVER NAMESPACE - Server functions (stubs)
    // These are for Frame runtime, stubbed for plugin execution
    // =========================================
    fn register_http_server_functions(linker: &mut Linker<PluginState>) -> Result<()> {
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
    // DEV CAPTURE - Server-only runtime snapshot (stub)
    // Real impl lives in clean-server (src/dev_capture.rs); sandbox returns
    // an LP empty string so plugin WASM importing _dev_snapshot instantiates
    // cleanly. See foundation/platform-architecture/function-registry.toml
    // category=dev_capture and SERVER_EXTENSIONS.md §Dev-mode Capture.
    // =========================================
    fn register_dev_capture_functions(linker: &mut Linker<PluginState>) -> Result<()> {
        linker.func_wrap(
            "env",
            "_dev_snapshot",
            |mut caller: Caller<'_, PluginState>| -> i32 { write_clean_string(&mut caller, b"") },
        )?;
        Ok(())
    }

    // =========================================
    // REQUEST CONTEXT ACCESS - Stubs for plugins
    // =========================================
    fn register_request_context_functions(linker: &mut Linker<PluginState>) -> Result<()> {
        // env._req_param - Get path parameter (stub)
        linker.func_wrap(
            "env",
            "_req_param",
            |mut caller: Caller<'_, PluginState>, _name_ptr: i32, _name_len: i32| -> i32 {
                // Return empty string (stub)
                write_clean_string(&mut caller, b"")
            },
        )?;

        // env._req_query - Get query parameter (stub)
        linker.func_wrap(
            "env",
            "_req_query",
            |mut caller: Caller<'_, PluginState>, _name_ptr: i32, _name_len: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;

        // env._req_body - Get request body (stub)
        linker.func_wrap(
            "env",
            "_req_body",
            |mut caller: Caller<'_, PluginState>| -> i32 { write_clean_string(&mut caller, b"") },
        )?;

        // env._req_body_bytes - Get request body as opaque byte handle (stub).
        // Returns a 4-byte allocation ([len=0][no bytes]) so plugin sandbox
        // instantiation succeeds; real byte payloads live in the server hosts.
        linker.func_wrap(
            "env",
            "_req_body_bytes",
            |mut caller: Caller<'_, PluginState>| -> i32 { write_clean_string(&mut caller, b"") },
        )?;

        // env._req_header - Get request header (stub)
        linker.func_wrap(
            "env",
            "_req_header",
            |mut caller: Caller<'_, PluginState>, _name_ptr: i32, _name_len: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;

        // env._req_method - Get request method (stub)
        linker.func_wrap(
            "env",
            "_req_method",
            |mut caller: Caller<'_, PluginState>| -> i32 { write_clean_string(&mut caller, b"") },
        )?;

        // env._req_path - Get request path (stub)
        linker.func_wrap(
            "env",
            "_req_path",
            |mut caller: Caller<'_, PluginState>| -> i32 { write_clean_string(&mut caller, b"") },
        )?;

        Ok(())
    }

    // =========================================
    // FILE NAMESPACE - File operations (stubs)
    // =========================================
    fn register_file_functions(linker: &mut Linker<PluginState>) -> Result<()> {
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
                write_clean_string(&mut caller, b"")
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

        // env._fs_write_bytes - Write opaque byte handle to path (stub).
        // Params: (path_ptr, path_len, handle). Handle is a pointer to a
        // length-prefixed byte buffer. Real hosts do the actual write;
        // the plugin sandbox stub returns 0 (success) so compilation
        // and plugin instantiation don't fail.
        linker.func_wrap(
            "env",
            "_fs_write_bytes",
            |_: Caller<'_, PluginState>, _path_ptr: i32, _path_len: i32, _handle: i32| -> i32 {
                0 // Success
            },
        )?;

        // env._crypto_sha256_bytes - SHA-256 of length-prefixed byte handle (stub).
        // Params: (handle). Handle is a pointer to a length-prefixed byte buffer
        // ([4-byte LE length][bytes]) — same layout as _req_body_bytes output.
        // Returns a length-prefixed lowercase hex string (64 chars in real hosts).
        // Third bridge in the opaque-handle triad (_req_body_bytes →
        // _crypto_sha256_bytes → _fs_write_bytes). Real hosts compute the actual
        // digest; the plugin sandbox stub allocates a 4-byte empty result so
        // plugin instantiation and callers reading the returned pointer as a
        // length-prefixed string do not crash. See spec/type-system.md §9b.
        linker.func_wrap(
            "env",
            "_crypto_sha256_bytes",
            |mut caller: Caller<'_, PluginState>, _handle: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;

        Ok(())
    }

    // =========================================
    // MATH NAMESPACE - Math operations
    // Required by compiled WASM modules for stdlib functions
    // =========================================
    fn register_math_functions(linker: &mut Linker<PluginState>) -> Result<()> {
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
    fn register_http_auth_stubs(linker: &mut Linker<PluginState>) -> Result<()> {
        // Additional HTTP request stubs (not defined above)
        linker.func_wrap(
            "env",
            "_req_cookie",
            |mut caller: Caller<'_, PluginState>, _: i32, _: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;
        // String-returning request stubs hand back an empty string, like
        // `_req_cookie`, so a plugin reading the result sees "" rather than
        // a null pointer.
        linker.func_wrap(
            "env",
            "_req_form",
            |mut caller: Caller<'_, PluginState>| -> i32 { write_clean_string(&mut caller, b"") },
        )?;
        linker.func_wrap(
            "env",
            "_req_ip",
            |mut caller: Caller<'_, PluginState>| -> i32 { write_clean_string(&mut caller, b"") },
        )?;
        linker.func_wrap(
            "env",
            "_req_headers",
            |mut caller: Caller<'_, PluginState>| -> i32 { write_clean_string(&mut caller, b"") },
        )?;

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
                write_clean_string(&mut caller, b"")
            },
        )?;
        // _session_create: same signature as _session_store (old API name used by frame.data <=2.0.0)
        linker.func_wrap(
            "env",
            "_session_create",
            |mut caller: Caller<'_, PluginState>, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 {
                write_clean_string(&mut caller, b"")
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
            |mut caller: Caller<'_, PluginState>| -> i32 { write_clean_string(&mut caller, b"") },
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

        // error(message: string) -> i32 — the codegen contract is "print and
        // trap" (codegen_module_builder.rs, `register_console_imports`). In
        // the sandbox the call traps with the message, so the plugin call
        // fails with the plugin's own error (`describe_plugin_trap`).
        linker.func_wrap(
            "env",
            "error",
            |mut caller: Caller<'_, PluginState>, ptr: i32| -> wasmtime::Result<i32> {
                let message = read_clean_string(&mut caller, ptr)
                    .unwrap_or_else(|| format!("<unreadable message at pointer {}>", ptr));
                Err(wasmtime::Error::new(PluginRaisedError(message)))
            },
        )?;
        // console_error(message) logs to stderr; the sandbox drops plugin log output.
        linker.func_wrap(
            "env",
            "console_error",
            |_: Caller<'_, PluginState>, _: i32| -> i32 { 0 },
        )?;

        // JSON bridges (stdlib-reference §8): the real implementation, so a
        // plugin can parse and serialize JSON while it expands a block.
        crate::runtime::json_bridge::register_json_bridges(linker, json_bridge_alloc)?;
        // The pre-Delivery-2 `_json_encode` / `_json_decode` names stay
        // catalogued in runtime-abi/v1.toml until the [P5] post-soak
        // cleanup; no compiler emits them, so the sandbox keeps them as
        // empty-result stubs that only satisfy the import.
        linker.func_wrap(
            "env",
            "_json_encode",
            |mut caller: Caller<'_, PluginState>, _: i32| -> i32 {
                write_clean_string(&mut caller, b"")
            },
        )?;
        linker.func_wrap(
            "env",
            "_json_decode",
            |mut caller: Caller<'_, PluginState>, _: i32, _: i32| -> i32 {
                write_clean_string(&mut caller, b"")
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
        let mut parser = SpecificationParser::new(tokens);
        // Plugin output may have sections in any order (e.g., external: before functions: before start:)
        parser.set_lenient_section_order(true);
        parser
            .parse_program()
            .map_err(|e| anyhow!("Failed to parse plugin output: {}", e))
    }

    /// Parse v1 plugin output: a Clean Language snippet that is either a
    /// whole program (top-level sections such as `functions:` and `start:`,
    /// in any order) or plain statements (plugin-contract.md §5.3,
    /// lifecycle.md §2.1), which are parsed as the body of a `start:` block.
    /// Output that is neither is the plugin's error; both parse errors are
    /// reported with the start of the output.
    fn parse_plugin_snippet(&self, source: &str) -> Result<crate::ast::Program> {
        let as_program = match self.parse_plugin_code(source) {
            Ok(program) => return Ok(program),
            Err(e) => e,
        };
        self.parse_plugin_code(&wrap_slot_statements_as_start(source))
            .map_err(|as_statements| {
                anyhow!(
                    "plugin output is neither a program ({}) nor statements ({}) — \
                     plugin output: {}",
                    as_program,
                    as_statements,
                    diagnostic_prefix(source)
                )
            })
    }

    /// Call the expand function in the WASM module and return the inline
    /// statements of the expansion (top-level statements followed by the
    /// start-function body).
    ///
    /// This is a thin view over `call_expand_full`; the full expansion is the
    /// single implementation of the plugin ABI call.
    fn call_expand(&self, block: &FrameworkBlock) -> Result<Vec<Statement>> {
        self.call_expand_full(block)
            .map(PluginExpansion::into_inline_statements)
    }

    /// Call the expand function and return the full expansion result.
    ///
    /// Plugin ABI: expand(block_name: string, attributes: string, body: string) -> string
    /// Clean Language strings are pointers to [4-byte length][data] structures.
    ///
    /// The result preserves the start function, functions, classes,
    /// capabilities, externals and state the plugin generates.
    fn call_expand_full(&self, block: &FrameworkBlock) -> Result<PluginExpansion> {
        tracing::debug!(
            target: "compiler::plugins::arena",
            block = block.name.trim_end_matches(':'),
            entry = "call_expand_full",
            plugin = %self.name,
            "plugin arena: expand-block entry",
        );

        // Strip trailing colon from block name (e.g., "server:" -> "server")
        let block_name = block.name.trim_end_matches(':');

        // ── Plugin Contracts v3 early-exit ────────────────────────────────────
        // Resolve dispatch BEFORE creating the v1 store/linker so that a v3
        // plugin (which imports typed-emission bridges the v1 linker does not
        // provide) is never instantiated with the wrong linker.
        let dispatch = self.manifest.resolve_block_dispatch(block_name);
        let ExpandInputs {
            binding_header,
            attributes: attributes_str,
            body: actual_body,
        } = expand_inputs(block_name, block, dispatch.version);
        if dispatch.version >= 3 {
            return self.call_expand_typed(
                &dispatch.export,
                block,
                block_name,
                &attributes_str,
                &actual_body,
            );
        }
        // ── v1 path continues below ───────────────────────────────────────────

        let mut store = self.create_store();
        let (instance, memory) = self.instantiate(
            &mut store,
            &self.linker,
            &format!("expand block `{}`", block_name),
        )?;

        let block_name_ptr = self.write_clean_string(&mut store, &instance, &memory, block_name)?;

        let attributes_ptr =
            self.write_clean_string(&mut store, &instance, &memory, &attributes_str)?;
        let body_ptr = self.write_clean_string(&mut store, &instance, &memory, &actual_body)?;

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

        // Reattach any binding header that was stripped on the way in.
        //
        // If the plugin's response is a bare expression (the common case), we
        // prepend the header and let the parser produce the VariableDecl. If
        // the plugin emitted a complete `<type> <name> = ...` statement itself,
        // we detect that and skip the prefix so we don't double up. The
        // detector compares token-wise against the exact header text (see
        // `starts_with_binding_header`, #6a754781d652).
        let generated_code: std::borrow::Cow<'_, str> = match &binding_header {
            Some(header) => {
                let trimmed = generated_code.trim_start();
                if starts_with_binding_header(trimmed, header) {
                    std::borrow::Cow::Borrowed(generated_code)
                } else {
                    std::borrow::Cow::Owned(format!("{} {}", header, trimmed))
                }
            }
            None => std::borrow::Cow::Borrowed(generated_code),
        };
        let generated_code: &str = &generated_code;

        let program = self.parse_plugin_snippet(generated_code)?;
        tracing::debug!(
            target: "compiler::plugins::arena",
            peak_bytes = store.data().peak_alloc_offset,
            final_mark_depth = store.data().arena_marks.len(),
            block = block_name,
            "plugin arena: expand-block exit",
        );

        Ok(PluginExpansion {
            statements: program.statements,
            start_function: program.start_function,
            functions: program.functions,
            classes: program.classes,
            capabilities: program.capabilities,
            externals: program.externals,
            state: program.state,
        })
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
        instance: &wasmtime::Instance,
        memory: &Memory,
        s: &str,
    ) -> Result<i32> {
        let bytes = s.as_bytes();
        let len = bytes.len();

        // Layout: [length:4][data:len]
        // Align to 8 bytes for safety
        let total_size = (4 + len).div_ceil(8) * 8;

        let heap = SharedHeap::of_instance(store, instance);
        let ptr = host_allocate(store, heap, total_size, PluginState::allocate);
        if ptr == 0 {
            // Allocator exhausted — never write over address 0.
            return Err(anyhow!(
                "Plugin host allocator exhausted while writing a {}-byte string",
                len
            ));
        }

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
        read_result_from_data(memory.data(store), ptr)
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
        let (instance, memory) = self.instantiate(
            &mut store,
            &self.linker,
            &format!("lifecycle slot `{}`", slot_name),
        )?;

        // Serialize the build context to JSON and write to plugin memory.
        let context_json = serde_json::to_string(context).map_err(|e| {
            anyhow!(
                "Failed to serialize build context for lifecycle slot `{}`: {}",
                slot_name,
                e
            )
        })?;
        let context_ptr = self.write_clean_string(&mut store, &instance, &memory, &context_json)?;

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
                diagnostic_prefix(response_str)
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
        let parsed = self
            .parse_plugin_snippet(&response.statements)
            .map_err(|e| anyhow!("Lifecycle slot `{}`: {}", slot_name, e))?;
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
            "plugin arena: lifecycle-slot exit",
        );

        Ok(PluginExpansion {
            statements,
            start_function: None,
            functions: parsed.functions,
            classes: parsed.classes,
            capabilities: parsed.capabilities,
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
        export_name: &str,
        html: &str,
        path: &str,
        registry_json: &str,
        companion_json: &str,
    ) -> Result<String> {
        let mut store = self.create_store();
        let (instance, memory) = self.instantiate(
            &mut store,
            &self.linker,
            &format!("process_html for `{}`", path),
        )?;

        let html_ptr = self.write_clean_string(&mut store, &instance, &memory, html)?;
        let path_ptr = self.write_clean_string(&mut store, &instance, &memory, path)?;
        let registry_ptr =
            self.write_clean_string(&mut store, &instance, &memory, registry_json)?;
        let companion_ptr =
            self.write_clean_string(&mut store, &instance, &memory, companion_json)?;

        let process: TypedFunc<(i32, i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, export_name)
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

        let result_bytes = self.read_result(&store, &memory, result_ptr)?;
        let generated = std::str::from_utf8(&result_bytes)
            .map_err(|e| anyhow!("Invalid UTF-8 in process_html response: {}", e))?;

        Ok(generated.to_string())
    }

    /// Call the plugin's `assemble` export. Its signature was validated at
    /// load time (`validate_assemble_export_signature`): either the v1
    /// `assemble(input_lp) -> output_lp` or the v3
    /// `assemble_typed(ctx, input_lp) -> status`.
    fn call_assemble(
        &self,
        export_name: &str,
        input: &crate::plugins::plugin_abi::AssembleInput,
    ) -> Result<crate::plugins::plugin_abi::AssembleOutput> {
        use crate::plugins::plugin_abi::AssembleOutput;

        let input_json = serde_json::to_string(input)
            .map_err(|e| anyhow!("Failed to serialize AssembleInput: {}", e))?;

        // v3 plugins may import typed-emission bridges (`_emit_class_full`,
        // `batch.*`, etc.) at the module level even when a specific export
        // (like assemble) doesn't call them at runtime. WASM instantiation
        // requires ALL imports to resolve, so assemble always runs under the
        // typed-emission linker, with a per-call arena installed for the
        // bridges to reach.
        //
        // Bug: frame.ui 3.0.0 assemble hook failed instantiation at client-mode
        // build with `unknown import: env::_emit_class_full` before this fix.
        //
        // Slot discriminator: the arena is created for `AssembleTyped` so
        // structural bridges refuse with PLUGIN018 and only
        // `_inject_source_file` / `_transform_source_file` succeed. See
        // `foundation/spec/plugins/contracts/assemble.md` §6.
        let ctx = self.next_ctx_handle();
        let mut state = PluginState::new();
        state.emit_arena = Some(crate::plugins::typed_emission::EmitArena::new_assemble_typed(ctx));
        let mut store = self.create_store_with_state(state);
        let (instance, memory) =
            self.instantiate(&mut store, self.typed_emission_linker()?, "assemble")?;
        let input_ptr = self
            .write_clean_string(&mut store, &instance, &memory, &input_json)
            .map_err(|e| anyhow!("Failed to write input to WASM memory: {}", e))?;

        if let Ok(assemble_typed) =
            instance.get_typed_func::<(i32, i32), i32>(&mut store, export_name)
        {
            let status = assemble_typed
                .call(&mut store, (ctx, input_ptr))
                .map_err(|e| anyhow!("{}", describe_plugin_trap(&e, "assemble_typed")))?;

            // Drain the arena and consume its `injected_sources` +
            // `transformed_sources`, populated by `_inject_source_file` and
            // `_transform_source_file` — the two legal typed-emission
            // bridges inside `assemble_typed` per
            // `foundation/spec/plugins/contracts/assemble.md` §6.1.
            //
            // Structural bridges refuse with PLUGIN018 upstream (see the
            // `refuse_if_assemble_typed!` gate at the entry of every
            // structural bridge in `typed_emission/bridges.rs`) and trip the
            // sticky-error flag, which `saw_error` catches below.
            let mut arena = store
                .data_mut()
                .emit_arena
                .take()
                .ok_or_else(|| anyhow!("the emit arena was taken during assemble_typed"))?;
            let injected_sources = std::mem::take(&mut arena.injected_sources);
            let transformed_sources = std::mem::take(&mut arena.transformed_sources);
            let (_expansion, diagnostics, saw_error) = arena.finish();
            let errors = self.route_typed_diagnostics("assemble", &diagnostics);
            if status != 0 {
                return Err(anyhow!(
                    "assemble_typed returned non-zero status: {} (errors: {})",
                    status,
                    errors.as_deref().unwrap_or("(none)")
                ));
            }
            if saw_error {
                return Err(anyhow!(
                    "assemble_typed emitted typed-emission errors: {}",
                    errors.unwrap_or_default()
                ));
            }
            return Ok(AssembleOutput {
                injected_sources,
                transformed_sources,
            });
        }

        // v1 assemble signature: (input_lp) -> output_lp
        let assemble_fn: TypedFunc<i32, i32> = instance
            .get_typed_func(&mut store, export_name)
            .map_err(|e| {
                anyhow!(
                    "Plugin does not export assemble function '{}': {}",
                    export_name,
                    e
                )
            })?;
        let result_ptr = assemble_fn
            .call(&mut store, input_ptr)
            .map_err(|e| anyhow!("{}", describe_plugin_trap(&e, "assemble")))?;

        let result_bytes = self
            .read_result(&store, &memory, result_ptr)
            .map_err(|e| anyhow!("Failed to read assemble result: {}", e))?;

        if result_bytes.is_empty() {
            // assemble.md §5.1: the v1 hook returns an `AssembleOutput` JSON
            // object — `{}` when it has nothing to add. An empty result is not
            // that; it is almost always an out-of-memory failure inside the
            // plugin (`mem_alloc` returns 0 once `memory.grow()` is refused,
            // the plugin propagates the 0 and the result pointer is 0).
            // Treating it as "nothing to add" silently drops the plugin's
            // sources, so it is an error.
            let oom = store.data().oom_during_call.clone();
            return Err(anyhow!(
                "plugin `{}` assemble() returned an empty result for a {}-byte input instead \
                 of an AssembleOutput JSON object{}",
                self.name,
                input_json.len(),
                oom.map(|s| format!(" [OOM: {}]", s)).unwrap_or_default()
            ));
        }

        let json_str = std::str::from_utf8(&result_bytes)
            .map_err(|e| anyhow!("Non-UTF-8 response from assemble: {}", e))?;
        serde_json::from_str(json_str).map_err(|e| {
            anyhow!(
                "Failed to parse assemble JSON response: {} — raw: {}",
                e,
                diagnostic_prefix(json_str)
            )
        })
    }
}

impl FrameworkPlugin for WasmPluginAdapter {
    fn name(&self) -> &'static str {
        self.name_cache
    }

    fn handles(&self) -> &'static [&'static str] {
        self.handles_cache
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

    /// plugin-contract.md §5.2: the declared `[exports].validate` runs once
    /// before the block is expanded (`PluginRegistry::expand_full`); a
    /// non-empty result is the developer-facing error.
    fn validate(&self, block: &FrameworkBlock) -> PluginResult<()> {
        let to_error = |message: String| PluginError::ValidationFailed {
            plugin_name: self.name.clone(),
            message,
            location: block.location.clone(),
        };
        match self.call_validate(block) {
            Ok(None) => Ok(()),
            Ok(Some(message)) => Err(to_error(message)),
            Err(e) => Err(to_error(e.to_string())),
        }
    }

    fn process_html(
        &self,
        html: &str,
        path: &str,
        registry_json: &str,
        companion_json: &str,
    ) -> PluginResult<Option<HtmlEntryOutput>> {
        // Plugin opted out of HTML processing — return None so the caller
        // tries the next plugin.
        let Some(export_name) = self.manifest.exports.process_html.as_deref() else {
            return Ok(None);
        };

        let output = if self.manifest.opts_into_typed_emission() {
            self.call_process_html_typed(export_name, html, path, registry_json, companion_json)
                .map(HtmlEntryOutput::Expansion)
        } else {
            self.call_process_html(export_name, html, path, registry_json, companion_json)
                .map(HtmlEntryOutput::Source)
        };
        output.map(Some).map_err(|e| PluginError::ExpansionFailed {
            plugin_name: self.name.clone(),
            block_name: "__process_html".to_string(),
            message: e.to_string(),
            location: None,
        })
    }

    fn lint_project(
        &self,
        program: &crate::ast::Program,
        config_level: &str,
    ) -> PluginResult<Option<String>> {
        // Plugin opted out of lint — return None so the caller skips it.
        if self.manifest.exports.lint.is_none() {
            return Ok(None);
        }

        self.call_lint_project(program, config_level)
            .map(Some)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "__lint_project".to_string(),
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
        let export_name = self.manifest.lifecycle.slot_export(slot_name);
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

    fn description(&self) -> &'static str {
        self.description_cache
    }

    fn expression_patterns(&self) -> &[String] {
        &self.expression_patterns_cache
    }

    fn get_keywords(&self) -> &'static [&'static str] {
        self.keywords_cache
            .get_or_init(|| match self.call_get_keywords() {
                Ok(keywords) => intern_str_list(&keywords),
                Err(e) => {
                    tracing::warn!(
                        target: "compiler::plugins",
                        plugin = %self.name,
                        error = %e,
                        "plugin get_keywords failed; the plugin contributes no keywords"
                    );
                    &[]
                }
            })
    }

    fn assemble(
        &self,
        input: &crate::plugins::plugin_abi::AssembleInput,
    ) -> crate::plugins::PluginResult<crate::plugins::plugin_abi::AssembleOutput> {
        let Some(export_name) = self.manifest.exports.assemble.as_deref() else {
            return Ok(crate::plugins::plugin_abi::AssembleOutput::default());
        };
        self.call_assemble(export_name, input)
            .map_err(|e| PluginError::ExpansionFailed {
                plugin_name: self.name.clone(),
                block_name: "assemble".to_string(),
                message: e.to_string(),
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
    /// Stack of save marks for nested `mem_scope_push` / `mem_scope_pop`
    /// calls. Each entry is `(host alloc_offset, wasm __heap_ptr)` at the
    /// time of the push. `mem_scope_pop` rewinds both. This is kept
    /// separate from `arena_marks` because the two APIs are emitted by
    /// different HIR rewrites and can interleave with different depths;
    /// sharing a stack risked one API popping a mark pushed by the other.
    /// PLG001 (fp 81ee7e44).
    mem_scope_marks: Vec<(usize, usize)>,
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
    /// Plugin Contract 5 (Phase B) lint arena. Installed by the
    /// `call_lint_project` path (Cycle 2) before instantiation and taken
    /// after the lint call returns. The `Option<>` is `None` on every
    /// call path other than lint — the 4 `_ast_*` accessor bridges refuse
    /// (returning a `NOT-IN-LINT-CALL` JSON error) when this field is None.
    pub(crate) lint_arena: Option<crate::plugins::lint::LintArena>,
    /// Plugin Contracts v2 per-build keystore read and written by the
    /// `_build_state_set` / `_build_state_get` bridges. The adapter hands
    /// every store its shared registry-wide state (`create_store_with_state`).
    build_state: crate::plugins::BuildState,
    /// True once the host allocator has joined the plugin's own bump heap
    /// (`SharedHeap::join`). From then on host allocations come from that
    /// heap's top instead of the separate region starting at 512 KiB.
    heap_joined: bool,
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
    pub(crate) fn new() -> Self {
        Self {
            // Start allocations at 512KB to avoid collision with WASM data section
            // and the module's internal heap which can grow from lower addresses.
            // The WASM module's static data starts at 4KB and heap follows the data
            // section. For large plugins with many string operations, 512KB is safe.
            alloc_offset: 524288,
            stable_zone_end: 524288,
            peak_alloc_offset: 524288,
            arena_marks: Vec::new(),
            mem_scope_marks: Vec::new(),
            cached_empty_lp_ptr: None,
            oom_during_call: None,
            emit_arena: None,
            lint_arena: None,
            build_state: crate::plugins::new_build_state(),
            heap_joined: false,
        }
    }

    /// Raise the bump pointer to `top` (8-byte aligned, capped at
    /// `i32::MAX`); never lowers it.
    fn raise_alloc_offset(&mut self, top: usize) {
        let top = top.min(i32::MAX as usize);
        if top > self.alloc_offset {
            self.alloc_offset = top;
            self.peak_alloc_offset = self.peak_alloc_offset.max(top);
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
    pub(crate) fn allocate(&mut self, size: usize) -> usize {
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
    fn mem_scope_marks_lifo_pair_alloc_and_heap() {
        // PLG001 (fp 81ee7e44): mem_scope_push/mem_scope_pop must save AND
        // restore both the host bump-allocator offset and the WASM
        // __heap_ptr snapshot together, and behave as a LIFO stack that is
        // independent of `arena_marks`. The bridge relies on this stack
        // holding (alloc_offset, wasm_heap_ptr) pairs so pop can rewind
        // both. Verify the stack behaves as a LIFO regardless of
        // interleaved `arena_marks` activity.
        let mut state = PluginState::new();
        state.mem_scope_marks.push((1_000, 2_000));
        // Simulate an unrelated arena scope opening — the mem_scope stack
        // must not observe it.
        let _arena = state.arena_mark();
        state.mem_scope_marks.push((3_000, 4_000));
        assert_eq!(state.mem_scope_marks.len(), 2, "both marks pushed");
        assert_eq!(
            state.mem_scope_marks.pop(),
            Some((3_000, 4_000)),
            "LIFO: top was the last push"
        );
        assert_eq!(
            state.mem_scope_marks.pop(),
            Some((1_000, 2_000)),
            "LIFO: bottom was the first push, unaffected by arena_marks"
        );
        assert_eq!(
            state.mem_scope_marks.pop(),
            None,
            "empty stack pops to None (defensive: unbalanced scopes)"
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

/// The first 256 bytes of plugin output for an error message, cut back to
/// a UTF-8 character boundary so a multi-byte character straddling the
/// limit cannot make the slice panic.
fn diagnostic_prefix(s: &str) -> &str {
    let mut end = s.len().min(256);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The trap a plugin's `error(message)` call raises (`env.error`).
#[derive(Debug)]
struct PluginRaisedError(String);

impl std::fmt::Display for PluginRaisedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "plugin error: {}", self.0)
    }
}

impl std::error::Error for PluginRaisedError {}

/// The message of the `error(message)` call that made a plugin call trap.
fn plugin_raised_message(err: &wasmtime::Error) -> Option<&str> {
    err.chain()
        .find_map(|cause| cause.downcast_ref::<PluginRaisedError>())
        .map(|raised| raised.0.as_str())
}

/// Format a wasmtime `.call()` error with the trap kind (when available)
/// prepended to the default backtrace. Lets the framework see, e.g.,
/// "out of bounds memory access at wasm function 276" instead of the bare
/// `<unknown>!<wasm function 276>`. See
/// `compiler-build-state-bridge-runtime-trap.md`.
fn describe_plugin_trap(err: &wasmtime::Error, context: &str) -> String {
    if let Some(message) = plugin_raised_message(err) {
        return format!("plugin raised an error in {}: {}", context, message);
    }
    let mut prefix = format!("Plugin trap in {}", context);
    // Walk the error chain for the first wasmtime::Trap. wasmtime::Error
    // wraps a Trap in its cause chain when a wasm-level trap occurred.
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
    err: &wasmtime::Error,
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
/// bridges on the given linker. Both operate on the calling store's
/// `PluginState::build_state`, so the linker itself holds no build state.
///
/// **Calling convention:** both bridges follow the canonical contract in
/// `foundation/platform-architecture/function-registry.toml`, which the
/// registry loader expands with `expand_strings=true` — every `"string"`
/// param becomes a raw `(ptr: i32, len: i32)` pair, and `"void"` returns
/// nothing. The compiler emits an unpacking wrapper on the plugin side, so
/// plugins can still call these bridges with a Clean `string` value; the
/// host just sees the unpacked pair.
fn register_build_state_bridges(linker: &mut Linker<PluginState>) -> Result<()> {
    // `_build_state_set(key_ptr, key_len, value_ptr, value_len)` — stores a
    // value under a key in the per-build state. `void` return per the
    // canonical contract; the WASM import has no result.
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
            if let Ok(mut guard) = caller.data().build_state.lock() {
                guard.insert(key, value);
            }
        },
    )?;

    // `_build_state_get(key_ptr, key_len) -> string_lp_ptr` — returns an LP
    // pointer to the value, or an LP-pointer to a cached empty string when
    // the key is absent. The empty-string LP pointer is cached in
    // `PluginState` so warmup-time misses don't churn the bump allocator.
    linker.func_wrap(
        "env",
        "_build_state_get",
        move |mut caller: Caller<'_, PluginState>, key_ptr: i32, key_len: i32| -> i32 {
            let Some(key) = read_raw_string(&mut caller, key_ptr, key_len) else {
                return 0;
            };
            let value = caller
                .data()
                .build_state
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

/// Read the `[4-byte LE length][data]` result a plugin export returned at
/// `ptr`. Any pointer that cannot be interpreted safely (negative, header or
/// payload out of bounds) is an error, never a panic.
fn read_result_from_data(data: &[u8], ptr: i32) -> Result<Vec<u8>> {
    read_lp_from_data(data, ptr)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| {
            anyhow!(
                "Plugin result pointer {} is out of bounds (memory size {} bytes)",
                ptr,
                data.len()
            )
        })
}

/// Read a Clean LP-string (`[4-byte LE length][UTF-8 bytes]`) from the
/// calling plugin's memory. Returns `None` for any pointer that cannot be
/// read safely or whose payload is not valid UTF-8. Shared by every host
/// bridge family (v1 stdlib, typed emission, lint).
pub(crate) fn read_clean_string(caller: &mut Caller<'_, PluginState>, ptr: i32) -> Option<String> {
    let memory = caller.get_export("memory").and_then(|e| e.into_memory())?;
    let bytes = read_lp_from_data(memory.data(&*caller), ptr)?;
    std::str::from_utf8(bytes).ok().map(str::to_string)
}

/// Write a byte slice as a Clean length-prefixed string into WASM memory,
/// allocating from the **stable** zone so the pointer survives any future
/// `arena_reset`. Only used for long-lived pointers that must outlive any
/// transient scope (today: `cached_empty_lp_ptr`).
///
/// Returns the pointer on success or 0 on failure.
fn write_clean_string_stable(caller: &mut Caller<'_, PluginState>, data: &[u8]) -> i32 {
    write_lp_string(caller, data, PluginState::allocate_stable)
}

/// Helper to write a byte slice as a Clean length-prefixed string into WASM memory.
///
/// Allocates space via the bump allocator, grows the module's linear memory if the
/// allocation exceeds the current `data_size`, then writes `[4-byte LE length][data]`.
///
/// Returns the pointer on success or 0 if the allocator is exhausted or memory
/// cannot be obtained / grown. Plugins treat 0 as an empty response.
pub(crate) fn write_clean_string(caller: &mut Caller<'_, PluginState>, data: &[u8]) -> i32 {
    write_lp_string(caller, data, PluginState::allocate)
}

/// A plugin's own bump heap: its exported `__heap_ptr` global (the top of
/// the heap, MEMORY_MODEL.md "The Heap Pointer") and its memory.
#[derive(Clone, Copy)]
struct SharedHeap {
    heap_ptr: wasmtime::Global,
    memory: Memory,
}

impl SharedHeap {
    /// The calling plugin's heap. `None` for modules that export no
    /// `__heap_ptr`; the host then allocates from its own region alone.
    fn of_caller(caller: &mut Caller<'_, PluginState>) -> Option<Self> {
        Some(Self {
            heap_ptr: caller.get_export("__heap_ptr")?.into_global()?,
            memory: caller.get_export("memory")?.into_memory()?,
        })
    }

    /// The heap of an instantiated plugin, for host writes made from outside
    /// a bridge call (the inputs written before an export is invoked).
    fn of_instance(store: &mut Store<PluginState>, instance: &wasmtime::Instance) -> Option<Self> {
        Some(Self {
            heap_ptr: instance.get_global(&mut *store, "__heap_ptr")?,
            memory: instance.get_memory(&mut *store, "memory")?,
        })
    }

    fn top(&self, cx: &mut wasmtime::StoreContextMut<'_, PluginState>) -> usize {
        self.heap_ptr.get(&mut *cx).i32().unwrap_or(0).max(0) as usize
    }

    fn set_top(&self, cx: &mut wasmtime::StoreContextMut<'_, PluginState>, top: usize) {
        let _ = self.heap_ptr.set(
            &mut *cx,
            wasmtime::Val::I32(top.min(i32::MAX as usize) as i32),
        );
    }

    /// Bring the host pointer up to the plugin's current heap top.
    ///
    /// The plugin's `__malloc` bumps `__heap_ptr` upward from the end of its
    /// data section, so host data must never sit in memory that pointer can
    /// still reach. Per MEMORY_MODEL.md the host shares that bump heap
    /// instead of keeping a separate region: on first contact the host
    /// region moves to the heap top, and every host allocation advances
    /// `__heap_ptr` past itself (`host_allocate`).
    ///
    /// First contact also reserves the stable empty LP string the
    /// `_build_state_get` bridge hands out. It sits below every save-point
    /// the plugin can take afterwards, so neither a host scope pop nor the
    /// plugin's own `__scope_pop` can hand its bytes out again.
    fn join(&self, cx: &mut wasmtime::StoreContextMut<'_, PluginState>) {
        let top = (self.top(cx) + 7) & !7;
        if cx.data().heap_joined {
            cx.data_mut().raise_alloc_offset(top);
            return;
        }
        let empty_lp = top;
        let end = empty_lp + 8;
        let size = self.memory.data_size(&mut *cx);
        if end > size
            && self
                .memory
                .grow(&mut *cx, (end - size).div_ceil(65536) as u64)
                .is_err()
        {
            // Out of memory: leave the host on its own region and let the
            // first allocation report the failure.
            cx.data_mut().raise_alloc_offset(top);
            return;
        }
        let _ = self.memory.write(&mut *cx, empty_lp, &[0u8; 4]);
        let state = cx.data_mut();
        state.heap_joined = true;
        state.alloc_offset = end;
        state.stable_zone_end = end;
        state.peak_alloc_offset = state.peak_alloc_offset.max(end);
        state.cached_empty_lp_ptr = Some(empty_lp as i32);
        self.set_top(cx, end);
    }
}

/// Allocate `size` bytes of plugin memory for data the host writes, with
/// `allocate` choosing the zone. When the plugin has its own heap the block
/// is taken at that heap's top and `__heap_ptr` is advanced past it, so the
/// plugin's later allocations start after the host data (see
/// `SharedHeap::join`).
fn host_allocate<C: wasmtime::AsContextMut<Data = PluginState>>(
    ctx: &mut C,
    heap: Option<SharedHeap>,
    size: usize,
    allocate: fn(&mut PluginState, usize) -> usize,
) -> usize {
    let mut cx = ctx.as_context_mut();
    if let Some(heap) = heap {
        heap.join(&mut cx);
    }
    let ptr = allocate(cx.data_mut(), size);
    if ptr != 0 {
        if let Some(heap) = heap {
            let end = cx.data().alloc_offset;
            heap.set_top(&mut cx, end);
        }
    }
    ptr
}

/// Open a reclamation scope on the shared heap and return its mark.
/// Everything allocated after the mark, by the host or by the plugin, lies
/// above it.
fn open_heap_scope(caller: &mut Caller<'_, PluginState>) -> usize {
    if let Some(heap) = SharedHeap::of_caller(caller) {
        heap.join(&mut caller.as_context_mut());
    }
    caller.data().alloc_offset
}

/// Close a reclamation scope: rewind the host pointer to `host_mark` and the
/// plugin's `__heap_ptr` to `heap_mark`, reclaiming every allocation made
/// since the matching `open_heap_scope`. Neither pointer is rewound into the
/// stable zone or moved upward.
fn close_heap_scope(caller: &mut Caller<'_, PluginState>, host_mark: usize, heap_mark: usize) {
    let state = caller.data_mut();
    let host_target = host_mark.max(state.stable_zone_end);
    if host_target <= state.alloc_offset {
        state.arena_reset(ArenaMark(host_target));
    }
    let heap_target = heap_mark.max(caller.data().stable_zone_end);
    if let Some(heap) = SharedHeap::of_caller(caller) {
        let mut cx = caller.as_context_mut();
        if heap_target < heap.top(&mut cx) {
            heap.set_top(&mut cx, heap_target);
        }
    }
}
/// Allocate `4 + data.len()` bytes with `allocate`, grow linear memory to
/// cover them and write `[4-byte LE length][data]`. Returns the pointer, or
/// 0 on any failure — never writing over address 0. An allocation or growth
/// failure is recorded in `oom_during_call` so a later trap is reported with
/// its out-of-memory cause (`describe_plugin_trap_with_oom`).
fn write_lp_string(
    caller: &mut Caller<'_, PluginState>,
    data: &[u8],
    allocate: fn(&mut PluginState, usize) -> usize,
) -> i32 {
    let Some((memory, ptr)) = host_allocate_in_memory(caller, 4 + data.len(), allocate) else {
        return 0;
    };

    // Write length header then content.
    let len_bytes = (data.len() as u32).to_le_bytes();
    if memory.write(&mut *caller, ptr, &len_bytes).is_err()
        || memory.write(&mut *caller, ptr + 4, data).is_err()
    {
        return 0;
    }

    ptr as i32
}

/// The calling plugin's exported `memory`. A module without one cannot
/// receive or return strings, so a bridge that needs it traps.
fn caller_memory(caller: &mut Caller<'_, PluginState>) -> wasmtime::Result<Memory> {
    caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| wasmtime::Error::msg("plugin module does not export `memory`"))
}

/// Write `unit` repeated `count` times as an LP string. The result is built
/// directly in plugin memory, so its size is bounded by what the plugin's
/// heap can hold: a result past the allocator's limit is an out-of-memory
/// failure (recorded in `oom_during_call`, 0 returned) like any other
/// oversized bridge result.
fn write_repeated_lp_string(
    caller: &mut Caller<'_, PluginState>,
    unit: &[u8],
    count: usize,
) -> i32 {
    let total = unit
        .len()
        .checked_mul(count)
        .filter(|&total| total <= i32::MAX as usize - 4);
    let Some(total) = total else {
        let state = caller.data_mut();
        if state.oom_during_call.is_none() {
            state.oom_during_call = Some(format!(
                "host bridge result: string_repeat of a {}-byte string {} times exceeds \
                 the plugin address space",
                unit.len(),
                count,
            ));
        }
        return 0;
    };
    let Some((memory, ptr)) = host_allocate_in_memory(caller, 4 + total, PluginState::allocate)
    else {
        return 0;
    };
    let data = memory.data_mut(&mut *caller);
    data[ptr..ptr + 4].copy_from_slice(&(total as u32).to_le_bytes());
    for chunk in data[ptr + 4..ptr + 4 + total].chunks_mut(unit.len().max(1)) {
        chunk.copy_from_slice(&unit[..chunk.len()]);
    }
    ptr as i32
}

/// Allocate `size` bytes on the shared heap with `allocate` and grow the
/// module's linear memory to cover them. Returns the memory and the address,
/// or `None` on failure — recording an allocation or growth failure in
/// `oom_during_call` so a later trap is reported with its cause.
fn host_allocate_in_memory(
    caller: &mut Caller<'_, PluginState>,
    size: usize,
    allocate: fn(&mut PluginState, usize) -> usize,
) -> Option<(Memory, usize)> {
    let heap = SharedHeap::of_caller(caller);
    let ptr = host_allocate(caller, heap, size, allocate);
    if ptr == 0 {
        let state = caller.data_mut();
        if state.oom_during_call.is_none() {
            state.oom_during_call = Some(format!(
                "host bridge result: bump allocator exhausted (>= {} MiB consumed) \
                 requesting {} bytes",
                i32::MAX as usize / (1024 * 1024),
                size,
            ));
        }
        return None;
    }

    let memory = caller.get_export("memory").and_then(|e| e.into_memory())?;

    // Grow the module's linear memory if the allocation extends past the current limit.
    // Use reborrows (&mut *caller) so the borrow-checker knows we re-use the same caller.
    let current_size = memory.data_size(&mut *caller);
    let required_size = ptr + size;
    if required_size > current_size {
        let pages_needed = (required_size - current_size).div_ceil(65536);
        if memory.grow(&mut *caller, pages_needed as u64).is_err() {
            let state = caller.data_mut();
            if state.oom_during_call.is_none() {
                state.oom_during_call = Some(format!(
                    "host bridge result: memory.grow failed at {} MiB current \
                     (plugin host-cap 1 GiB), requested a {} byte result \
                     ({} additional pages)",
                    current_size / (1024 * 1024),
                    size,
                    pages_needed,
                ));
            }
            return None;
        }
    }

    Some((memory, ptr))
}

/// `list.push_f64(list, value) -> list` for plugin code.
///
/// List layout (MEMORY_MODEL.md, `list.allocate`): a 16-byte header
/// `[length:i32][capacity:i32][type_id:i32][flags:i32]` followed by the
/// elements. `capacity` counts 4-byte slots — `list.allocate` reserves
/// `capacity * 4` bytes — and an f64 element takes two of them. When the
/// next element does not fit, the list is copied into a block with double
/// the room and the new pointer is returned, like the runtime's growable
/// lists. Returns 0 (with the cause recorded for the trap report) when the
/// pointer is invalid or memory runs out; never writes past the list.
fn list_push_f64(mut caller: Caller<'_, PluginState>, list_ptr: i32, value: f64) -> i32 {
    const HEADER: usize = 16;
    let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) else {
        return 0;
    };
    let Ok(ptr) = usize::try_from(list_ptr) else {
        return 0;
    };
    let data = memory.data(&caller);
    let Some(header) = data.get(ptr..ptr + HEADER) else {
        return 0;
    };
    let read = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap()) as usize;
    let (length, capacity) = (read(0), read(4));
    let used = length * 8;
    let room = capacity * 4;
    if used > room || ptr + HEADER + room > data.len() {
        // A header that claims more than the list holds or than memory has.
        return 0;
    }

    let (list, room) = if used + 8 <= room {
        (ptr, room)
    } else {
        let new_room = (room * 2).max(used + 8).max(64);
        let Some((_, new_ptr)) =
            host_allocate_in_memory(&mut caller, HEADER + new_room, PluginState::allocate)
        else {
            return 0;
        };
        let data = memory.data_mut(&mut caller);
        data.copy_within(ptr..ptr + HEADER + used, new_ptr);
        data[new_ptr + 4..new_ptr + 8].copy_from_slice(&((new_room / 4) as u32).to_le_bytes());
        (new_ptr, new_room)
    };
    debug_assert!(used + 8 <= room);

    let data = memory.data_mut(&mut caller);
    let at = list + HEADER + used;
    data[at..at + 8].copy_from_slice(&value.to_le_bytes());
    data[list..list + 4].copy_from_slice(&((length + 1) as u32).to_le_bytes());
    list as i32
}

/// Allocator for the JSON bridges (`runtime::json_bridge`): the shared bump
/// heap, the same one every other host result is written to.
fn json_bridge_alloc(caller: &mut Caller<'_, PluginState>, size: i32) -> Result<i32, String> {
    let size = usize::try_from(size).map_err(|_| format!("negative allocation size {size}"))?;
    host_allocate_in_memory(caller, size, PluginState::allocate)
        .and_then(|(_, ptr)| i32::try_from(ptr).ok())
        .ok_or_else(|| format!("plugin host allocator could not provide {size} bytes"))
}

/// Wrap statement-level plugin output in a synthetic `start:` block so the
/// program parser accepts it (`parse_plugin_snippet`).
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

/// One `"key":"value"` (or `"key":true` for a bare flag) JSON member of the
/// attribute payload, with key and value JSON-escaped.
fn attr_json_pair(key: &str, value: Option<&str>) -> String {
    let key = serde_json::Value::from(key);
    match value {
        Some(v) => format!("{}:{}", key, serde_json::Value::from(v)),
        None => format!("{}:true", key),
    }
}

/// Build the JSON attribute object passed to a plugin's expand export from
/// the block's declared attributes plus any inline `key="value"` pairs
/// (already formatted by `extract_inline_attrs`). Returns an empty string
/// when there are no attributes, which is what plugins expect.
fn build_attributes_json(
    attributes: &[crate::plugins::FrameworkAttribute],
    extra_attrs: Vec<String>,
) -> String {
    let mut pairs: Vec<String> = attributes
        .iter()
        .map(|attr| attr_json_pair(&attr.name, attr.value.as_deref()))
        .collect();
    pairs.extend(extra_attrs);
    if pairs.is_empty() {
        String::new()
    } else {
        format!("{{{}}}", pairs.join(","))
    }
}

/// The strings a block's expand export (and its validate export, which
/// plugin-contract.md §5.2 gives the same inputs) is called with.
struct ExpandInputs {
    /// The `<type> <name> =` binding line taken off the body, for a v1
    /// block; the compiler reattaches it to the plugin's output.
    binding_header: Option<String>,
    /// JSON object of the block's attributes (`""` when there are none).
    attributes: String,
    body: String,
}

/// Build the expand inputs for `block` under ABI `version`.
///
/// For a `<type> <name> = Model.<verb>:` block the expander puts the binding
/// on the first line of the content. A v1 (string-emission) plugin returns
/// a bare expression, so the binding is taken off here and reattached to
/// its output. A v3 (typed-emission) plugin receives the binding as part of
/// the body and emits the declaration itself (`emit_binding_stmt` in
/// frame.data), so the body is passed unchanged.
fn expand_inputs(block_name: &str, block: &FrameworkBlock, version: u32) -> ExpandInputs {
    let (binding_header, content) = if version >= 3 {
        (None, block.content.as_str())
    } else {
        split_binding_header(block_name, &block.content)
    };
    let (extra_attrs, body) = extract_inline_attrs(content);
    ExpandInputs {
        binding_header,
        attributes: build_attributes_json(&block.attributes, extra_attrs),
        body,
    }
}

/// Split the variable-binding header the expander prepends to ORM-style
/// dotted blocks (`<type> <name> =` on the first line of `Model.<verb>:`
/// content) from the block body.
///
/// The expander writes the header as `<type> <name> =` alone on the first
/// line, so the first line is a binding header iff it ends with its only
/// `=` and the text before it has at least two whitespace-separated tokens
/// (the type and the identifier — `list<User> rows`). A body line such as
/// `where: x == 5` or a field assignment (`name = "Alice"`) is not one.
/// Non-dotted blocks never carry a header.
fn split_binding_header<'c>(block_name: &str, content: &'c str) -> (Option<String>, &'c str) {
    if !block_name.contains('.') {
        return (None, content);
    }
    let first_line = content.lines().next().unwrap_or("").trim();
    let is_header = match first_line.strip_suffix('=') {
        Some(before) => !before.contains('=') && before.split_whitespace().count() >= 2,
        None => false,
    };
    if !is_header {
        return (None, content);
    }
    match content.find('\n') {
        Some(newline_pos) => (
            Some(content[..newline_pos].to_string()),
            &content[newline_pos + 1..],
        ),
        None => (Some(content.to_string()), ""),
    }
}

/// Extract the inline `key="value"` attributes written on a block's header
/// line (`component: tag="site-header"`, frame-ui.ebnf
/// `component_tag_attr`).
///
/// The parser hands over the block content from the first token after the
/// `:`. Text on the header line therefore arrives unindented as the first
/// line, while every body line keeps its leading tab. So the first line is
/// an attribute line only when it is not indented and consists entirely of
/// `identifier="string"` pairs separated by whitespace or a comma. Values are
/// `string_literal`s (grammar.ebnf §1.6) and their escapes are decoded.
///
/// Returns the attributes as JSON members (`"tag":"site-header"`) and the
/// body that follows; content without an attribute line is returned whole.
fn extract_inline_attrs(content: &str) -> (Vec<String>, String) {
    let (first_line, rest) = match content.find('\n') {
        Some(pos) => (&content[..pos], &content[pos + 1..]),
        None => (content, ""),
    };
    if first_line.starts_with([' ', '\t']) {
        return (Vec::new(), content.to_string());
    }
    match parse_attr_line(first_line.trim_end()) {
        Some(pairs) if !pairs.is_empty() => (
            pairs
                .iter()
                .map(|(k, v)| attr_json_pair(k, Some(v)))
                .collect(),
            rest.to_string(),
        ),
        _ => (Vec::new(), content.to_string()),
    }
}

/// Parse a whole line of `identifier="string"` pairs. `None` when any part
/// of the line is something else.
fn parse_attr_line(line: &str) -> Option<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        let key_len = rest
            .char_indices()
            .find(|&(i, c)| {
                !(c.is_ascii_alphabetic()
                    || c == '_'
                    || (i > 0 && (c.is_ascii_digit() || c == '-')))
            })
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        if key_len == 0 {
            return None;
        }
        let key = &rest[..key_len];
        let mut chars = rest[key_len..].strip_prefix("=\"")?.char_indices();
        let after_open = &rest[key_len + 2..];
        let mut value = String::new();
        let end = loop {
            let (i, c) = chars.next()?;
            match c {
                '"' => break i + 1,
                '\\' => {
                    let (_, escaped) = chars.next()?;
                    value.push(match escaped {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '0' => '\0',
                        '"' | '\\' | '{' | '}' => escaped,
                        _ => return None,
                    });
                }
                c => value.push(c),
            }
        };
        pairs.push((key.to_string(), value));
        // Pairs are separated by whitespace, optionally after a comma
        // (`tag="x", client="off"`).
        let after = &after_open[end..];
        let after_comma = after.strip_prefix(',').unwrap_or(after);
        let trimmed = after_comma.trim_start_matches([' ', '\t']);
        if !trimmed.is_empty() && trimmed.len() == after.len() {
            return None;
        }
        if trimmed.is_empty() && after_comma.len() != after.len() {
            // A trailing comma.
            return None;
        }
        rest = trimmed;
    }
    Some(pairs)
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

    /// Plugin output quoted in an error message is cut at 256 bytes. A
    /// multi-byte character straddling that limit used to panic the
    /// compiler (byte-index slice through a char) instead of reporting the
    /// plugin's malformed response.
    #[test]
    fn diagnostic_prefix_cuts_at_a_char_boundary() {
        let s = format!("{}é tail", "a".repeat(255));
        assert_eq!(diagnostic_prefix(&s), "a".repeat(255));
        assert_eq!(diagnostic_prefix("short"), "short");
        let long = "b".repeat(300);
        assert_eq!(diagnostic_prefix(&long).len(), 256);
    }

    /// Instantiate a one-page WAT module against one host-bridge group and
    /// return the store, instance and memory.
    fn instantiate_with(
        wat: &str,
        register: fn(&mut Linker<PluginState>) -> Result<()>,
    ) -> (Store<PluginState>, wasmtime::Instance, Memory) {
        let engine = Engine::default();
        let module = Module::new(&engine, wat).expect("compile WAT");
        let mut linker: Linker<PluginState> = Linker::new(&engine);
        register(&mut linker).expect("register bridges");
        let mut store = Store::new(&engine, PluginState::new());
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let memory = instance.get_memory(&mut store, "memory").expect("memory");
        (store, instance, memory)
    }

    fn call_and_read(wat: &str, register: fn(&mut Linker<PluginState>) -> Result<()>) -> Vec<u8> {
        let (mut store, instance, memory) = instantiate_with(wat, register);
        let run: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "run").unwrap();
        let ptr = run.call(&mut store, ()).expect("bridge call must not trap");
        read_lp_from_data(memory.data(&store), ptr)
            .expect("bridge result must be a readable LP string")
            .to_vec()
    }

    /// A v1 adapter for `wat` whose manifest is the given `[exports]` lines.
    fn v1_adapter(name: &str, exports: &str, wat: &str) -> WasmPluginAdapter {
        let engine = crate::plugins::wasm_loader::shared_engine().unwrap();
        let module = Module::new(&engine, wat).unwrap();
        let manifest: PluginManifest = toml::from_str(&format!(
            "[plugin]\nname = \"{name}\"\nversion = \"1.0.0\"\n\
             [handles]\nblocks = [\"probe\"]\n[exports]\n{exports}\n"
        ))
        .unwrap();
        WasmPluginAdapter::new(name.to_string(), manifest, module, engine).unwrap()
    }

    fn probe_block() -> FrameworkBlock {
        FrameworkBlock {
            name: "probe".to_string(),
            content: "body".to_string(),
            attributes: Vec::new(),
            location: None,
            body_location: None,
        }
    }

    /// A plugin's `error(message)` call fails the expansion with that
    /// message (the codegen contract is "print and trap"); it used to be a
    /// no-op that let the plugin carry on.
    #[test]
    fn plugin_error_call_fails_expansion_with_its_message() {
        let wat = r#"
            (module
              (import "env" "error" (func $error (param i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 1024) "\0e\00\00\00bad block body")
              (data (i32.const 2048) "\05\00\00\00x = 1")
              (func (export "expand") (param i32 i32 i32) (result i32)
                (drop (call $error (i32.const 1024)))
                i32.const 2048))
        "#;
        let adapter = v1_adapter("err.probe", "expand = \"expand\"", wat);
        let err = adapter.expand_full(&probe_block()).unwrap_err().to_string();
        assert!(err.contains("bad block body"), "{}", err);
        assert!(err.contains("plugin raised an error"), "{}", err);
    }

    /// v1 plugin output that does not parse is reported with the parser's
    /// errors and the output itself; the compiler no longer rewrites it
    /// (reordering sections, re-indenting a `start:` body) to make it parse.
    #[test]
    fn v1_unparseable_output_is_reported_not_rewritten() {
        let wat = r#"
            (module
              (memory (export "memory") 1)
              (data (i32.const 2048) "\02\00\00\00)(")
              (func (export "expand") (param i32 i32 i32) (result i32)
                i32.const 2048))
        "#;
        let adapter = v1_adapter("parse.probe", "expand = \"expand\"", wat);
        let err = adapter.expand_full(&probe_block()).unwrap_err().to_string();
        assert!(err.contains("neither a program"), "{}", err);
        assert!(err.contains("plugin output: )("), "{}", err);
    }

    /// A `validate` export returning pointer 0 is an allocator failure, not
    /// "valid" (the old code read an LP string at address 0).
    #[test]
    fn validate_null_result_is_an_error() {
        let wat = r#"
            (module
              (memory (export "memory") 1)
              (data (i32.const 2048) "\00\00\00\00")
              (func (export "expand") (param i32 i32 i32) (result i32)
                i32.const 2048)
              (func (export "validate") (param i32 i32 i32) (result i32)
                i32.const 0))
        "#;
        let adapter = v1_adapter(
            "validate.probe",
            "expand = \"expand\"\nvalidate = \"validate\"",
            wat,
        );
        let err = adapter.validate(&probe_block()).unwrap_err().to_string();
        assert!(err.contains("null string pointer"), "{}", err);
    }

    /// `[exports].get_keywords` reaches the plugin's export, which returns a
    /// JSON array of keywords.
    #[test]
    fn get_keywords_reads_the_plugin_export() {
        let wat = r#"
            (module
              (memory (export "memory") 1)
              (data (i32.const 2048) "\0d\00\00\00[\22one\22,\22two\22]")
              (func (export "expand") (param i32 i32 i32) (result i32)
                i32.const 2048)
              (func (export "get_keywords") (result i32)
                i32.const 2048))
        "#;
        let adapter = v1_adapter(
            "keywords.probe",
            "expand = \"expand\"\nget_keywords = \"get_keywords\"",
            wat,
        );
        assert_eq!(adapter.get_keywords(), &["one", "two"]);
        let without = v1_adapter("keywords.none", "expand = \"expand\"", wat);
        assert!(without.get_keywords().is_empty());
    }

    /// `string_repeat` builds its result in plugin memory; a result past the
    /// plugin address space is an out-of-memory failure (0 and a recorded
    /// cause), not a multi-gigabyte host allocation.
    #[test]
    fn string_repeat_is_bounded_by_plugin_memory() {
        let wat = r#"
            (module
              (import "env" "string_repeat" (func $repeat (param i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 16) "\04\00\00\00abcd")
              (func (export "small") (result i32)
                (call $repeat (i32.const 16) (i32.const 4) (i32.const 3)))
              (func (export "huge") (result i32)
                (call $repeat (i32.const 16) (i32.const 4) (i32.const 2147483647))))
        "#;
        let (mut store, instance, memory) =
            instantiate_with(wat, WasmPluginAdapter::register_string_dot_functions);
        let small: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "small").unwrap();
        let ptr = small.call(&mut store, ()).unwrap();
        assert_eq!(
            read_lp_from_data(memory.data(&store), ptr).unwrap(),
            b"abcdabcdabcd"
        );
        let huge: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "huge").unwrap();
        assert_eq!(huge.call(&mut store, ()).unwrap(), 0);
        assert!(store.data().oom_during_call.is_some());
    }

    /// A string bridge called from a module without an exported `memory`
    /// traps with an error instead of panicking the compiler.
    #[test]
    fn string_bridge_without_memory_traps() {
        let wat = r#"
            (module
              (import "env" "string.indexOf" (func $index (param i32 i32) (result i32)))
              (func (export "run") (result i32)
                (call $index (i32.const 0) (i32.const 0))))
        "#;
        let engine = Engine::default();
        let module = Module::new(&engine, wat).unwrap();
        let mut linker: Linker<PluginState> = Linker::new(&engine);
        WasmPluginAdapter::register_string_dot_functions(&mut linker).unwrap();
        let mut store = Store::new(&engine, PluginState::new());
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let run: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "run").unwrap();
        let err = run.call(&mut store, ()).unwrap_err();
        assert!(
            format!("{:?}", err).contains("does not export `memory`"),
            "{:?}",
            err
        );
    }

    /// `string.substring` takes byte offsets. An offset inside a multi-byte
    /// character used to panic the compiler (char-boundary slice).
    #[test]
    fn substring_bridge_slices_bytes_without_panicking() {
        let wat = r#"
            (module
              (import "env" "string.substring" (func $sub (param i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 16) "\03\00\00\00a\c3\a9")
              (func (export "run") (result i32)
                i32.const 16
                i32.const 0
                i32.const 2
                call $sub))
        "#;
        let bytes = call_and_read(wat, WasmPluginAdapter::register_string_dot_functions);
        assert_eq!(bytes, vec![b'a', 0xc3]);
    }

    /// Bridge results are written at the host bump pointer (512 KiB up),
    /// beyond a small plugin's initial memory. `float_to_string` wrote there
    /// without growing memory, silently dropped the failed write and
    /// returned a pointer past the end of memory.
    #[test]
    fn conversion_bridge_grows_memory_for_its_result() {
        let wat = r#"
            (module
              (import "env" "float_to_string" (func $f2s (param f64) (result i32)))
              (memory (export "memory") 1)
              (func (export "run") (result i32)
                f64.const 1.5
                call $f2s))
        "#;
        let bytes = call_and_read(wat, WasmPluginAdapter::register_env_core_functions);
        assert_eq!(bytes, b"1.5");
    }

    /// Stub bridges return an empty string. They used to hand back a bare
    /// bump allocation: never zeroed (stale bytes after an arena reset) and
    /// never backed by grown memory.
    #[test]
    fn stub_bridge_returns_a_readable_empty_string() {
        let wat = r#"
            (module
              (import "env" "http_get" (func $get (param i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "run") (result i32)
                i32.const 0
                i32.const 0
                call $get))
        "#;
        let bytes = call_and_read(wat, WasmPluginAdapter::register_http_client_functions);
        assert!(bytes.is_empty());
    }

    /// String-returning stubs hand back a readable empty LP string, never
    /// the caller's raw input pointer or a null pointer.
    #[test]
    fn string_stubs_return_readable_empty_strings() {
        let two_arg = |name: &str| {
            format!(
                r#"
                (module
                  (import "env" "{name}" (func $f (param i32 i32) (result i32)))
                  (memory (export "memory") 1)
                  (data (i32.const 64) "\05\00\00\00a%20b")
                  (func (export "run") (result i32)
                    i32.const 68
                    i32.const 5
                    call $f))
                "#
            )
        };
        for name in ["http_encode_url", "http_decode_url"] {
            let bytes = call_and_read(
                &two_arg(name),
                WasmPluginAdapter::register_http_client_functions,
            );
            assert!(bytes.is_empty(), "{} returned {:?}", name, bytes);
        }
        let no_arg = |name: &str| {
            format!(
                r#"
                (module
                  (import "env" "{name}" (func $f (result i32)))
                  (memory (export "memory") 1)
                  (func (export "run") (result i32) call $f))
                "#
            )
        };
        for name in ["_req_ip", "_req_form", "_req_headers", "_auth_get_session"] {
            let bytes = call_and_read(&no_arg(name), WasmPluginAdapter::register_http_auth_stubs);
            assert!(bytes.is_empty(), "{} returned {:?}", name, bytes);
        }
    }

    /// `list.push_f64` appends within the list's capacity and grows the list
    /// (returning the new pointer) when it is full, instead of writing past
    /// the allocation.
    #[test]
    fn list_push_f64_grows_a_full_list() {
        // A list at 1024 with capacity 2 slots (= one f64), followed by a
        // guard word that must survive.
        let wat = r#"
            (module
              (import "env" "list.push_f64" (func $push (param i32 f64) (result i32)))
              (memory (export "memory") 2)
              (global (export "__heap_ptr") (mut i32) (i32.const 4096))
              (data (i32.const 1024) "\00\00\00\00\02\00\00\00\00\00\00\00\00\00\00\00")
              (data (i32.const 1048) "\ef\be\ad\de")
              (func (export "run") (result i32)
                (local $l i32)
                i32.const 1024
                f64.const 1.5
                call $push
                local.set $l
                local.get $l
                f64.const 2.5
                call $push
                local.set $l
                local.get $l
                f64.const 3.5
                call $push))
        "#;
        let (mut store, instance, memory) =
            instantiate_with(wat, WasmPluginAdapter::register_list_functions);
        let run: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "run").unwrap();
        let list = run.call(&mut store, ()).expect("push must not trap") as usize;
        let data = memory.data(&store);
        let word = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let f64_at = |at: usize| f64::from_le_bytes(data[at..at + 8].try_into().unwrap());
        assert_ne!(list, 1024, "a full list must be copied, not overflowed");
        assert_eq!(word(list), 3, "length");
        assert!(
            word(list + 4) as usize * 4 >= 3 * 8,
            "capacity covers the elements"
        );
        assert_eq!(
            [f64_at(list + 16), f64_at(list + 24), f64_at(list + 32)],
            [1.5, 2.5, 3.5]
        );
        assert_eq!(
            word(1048),
            0xdead_beef,
            "memory after the old list is untouched"
        );
    }

    /// Header-line attributes (`component: tag="x"`) are taken off the
    /// content; an indented body line of the same shape is body, and
    /// escaped quotes in a value are decoded instead of breaking parsing.
    #[test]
    fn inline_attrs_are_parsed_exactly() {
        let (attrs, body) = extract_inline_attrs("tag=\"site-header\" client=\"off\"\n\thtml:\n");
        assert_eq!(attrs, vec!["\"tag\":\"site-header\"", "\"client\":\"off\""]);
        assert_eq!(body, "\thtml:\n");
        let (attrs, _) = extract_inline_attrs("tag=\"site-header\", client=\"off\"\n\thtml:\n");
        assert_eq!(attrs, vec!["\"tag\":\"site-header\"", "\"client\":\"off\""]);

        // An indented first line is the body, even when it looks like an attribute.
        let content = "\tname=\"x\"\n\tage=\"3\"\n";
        assert_eq!(extract_inline_attrs(content), (vec![], content.to_string()));

        // Escapes inside a value.
        let (attrs, _) = extract_inline_attrs("tag=\"a\\\"b\\\\c\"\n\tbody");
        assert_eq!(attrs, vec![r#""tag":"a\"b\\c""#]);

        // Anything that is not wholly `key="value"` pairs is not an attribute line.
        for line in [
            "tag=\"x\" stray",
            "tag=\"x\"other=\"y\"",
            "tag=\"x\",",
            "tag = \"x\"",
            "list<User> rows =",
            "tag=\"unterminated",
            "<div>",
        ] {
            let content = format!("{}\n\tbody", line);
            assert_eq!(
                extract_inline_attrs(&content),
                (vec![], content.clone()),
                "{:?}",
                line
            );
        }
    }

    /// Attribute keys and values are JSON-escaped completely (control
    /// characters included), so the payload a plugin receives always parses.
    #[test]
    fn attribute_json_escapes_control_characters() {
        let attrs = vec![crate::plugins::FrameworkAttribute {
            name: "title".to_string(),
            value: Some("line1\nline2\t\"q\"\\".to_string()),
            location: None,
        }];
        let json = build_attributes_json(&attrs, Vec::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).expect(&json);
        assert_eq!(parsed["title"], "line1\nline2\t\"q\"\\");
    }

    /// A v1 block's binding header is taken off the body (the compiler
    /// reattaches it); a v3 block receives it, and validate sees exactly
    /// what expand sees in both cases.
    #[test]
    fn expand_inputs_keep_binding_header_for_typed_emission() {
        let block = FrameworkBlock {
            name: "User.find".to_string(),
            content: "list<User> rows =\n\twhere: id == 1\n".to_string(),
            attributes: Vec::new(),
            location: None,
            body_location: None,
        };
        let v1 = expand_inputs("User.find", &block, 1);
        assert_eq!(v1.binding_header.as_deref(), Some("list<User> rows ="));
        assert_eq!(v1.body, "\twhere: id == 1\n");
        let v3 = expand_inputs("User.find", &block, 3);
        assert_eq!(v3.binding_header, None);
        assert_eq!(v3.body, block.content);
    }

    /// An ORM body without a binding (`return User.find:`) whose first line
    /// is a comparison keeps that line: it is not a `<type> <name> =` header.
    #[test]
    fn v1_body_line_with_comparison_is_not_a_binding_header() {
        let block = FrameworkBlock {
            name: "User.find".to_string(),
            content: "where: x == 5\n\tlimit: 1\n".to_string(),
            attributes: Vec::new(),
            location: None,
            body_location: None,
        };
        let v1 = expand_inputs("User.find", &block, 1);
        assert_eq!(v1.binding_header, None);
        assert_eq!(v1.body, block.content);
    }

    /// A v1 `assemble` that returns an empty result (the out-of-memory
    /// signature) is an error, not "nothing to add".
    #[test]
    fn v1_assemble_empty_result_is_an_error() {
        let wat = r#"
            (module
              (memory (export "memory") 1)
              (global (export "__heap_ptr") (mut i32) (i32.const 2048))
              (data (i32.const 1024) "\00\00\00\00")
              (func (export "assemble") (param i32) (result i32)
                i32.const 1024))
        "#;
        let engine = crate::plugins::wasm_loader::shared_engine().unwrap();
        let module = Module::new(&engine, wat).unwrap();
        let manifest: PluginManifest = toml::from_str(
            "[plugin]\nname = \"asm.empty\"\nversion = \"1.0.0\"\n\
             [handles]\nblocks = []\n[exports]\nassemble = \"assemble\"\n",
        )
        .unwrap();
        let adapter =
            WasmPluginAdapter::new("asm.empty".to_string(), manifest, module, engine).unwrap();
        let input = crate::plugins::plugin_abi::AssembleInput::default();
        let err = adapter.assemble(&input).unwrap_err().to_string();
        assert!(err.contains("empty result"), "{}", err);
    }

    /// A plugin-like module: exports `memory` and a `__heap_ptr` bump heap
    /// starting at 70 000 (where a real plugin's heap begins, just past its
    /// data section), plus `$malloc`, which bumps that heap and grows memory
    /// like the compiler's native allocator.
    fn plugin_heap_wat(imports: &str, body: &str) -> String {
        format!(
            r#"
            (module
              {imports}
              (memory (export "memory") 2)
              (global $heap (export "__heap_ptr") (mut i32) (i32.const 70000))
              (func $malloc (param $size i32) (result i32)
                (local $ptr i32) (local $end i32) (local $need i32)
                global.get $heap
                local.set $ptr
                local.get $ptr
                local.get $size
                i32.add
                local.set $end
                local.get $end
                i32.const 65535
                i32.add
                i32.const 16
                i32.shr_u
                memory.size
                i32.sub
                local.tee $need
                i32.const 0
                i32.gt_s
                if
                  local.get $need
                  memory.grow
                  drop
                end
                local.get $end
                global.set $heap
                local.get $ptr)
              {body})
            "#
        )
    }

    /// Fill `size` freshly allocated plugin bytes with 0xAA — what a plugin
    /// building a large page does to its heap.
    const FILL_600K: &str =
        "i32.const 600000 call $malloc i32.const 170 i32.const 600000 memory.fill";

    /// A host bridge result must survive the plugin allocating well past
    /// 400 KB afterwards. The host used to write results at 512 KiB in its
    /// own region while the plugin's heap grew up from ~70 KB into it.
    #[test]
    fn bridge_result_survives_plugin_heap_growth() {
        let wat = plugin_heap_wat(
            r#"(import "env" "float_to_string" (func $f2s (param f64) (result i32)))"#,
            &format!(
                r#"(func (export "run") (result i32) (local $s i32)
                     f64.const 1.5
                     call $f2s
                     local.set $s
                     {FILL_600K}
                     local.get $s)"#
            ),
        );
        let bytes = call_and_read(&wat, WasmPluginAdapter::register_env_core_functions);
        assert_eq!(bytes, b"1.5");
    }

    /// The inputs the host writes before invoking a plugin export must
    /// survive the plugin allocating well past 400 KB during the call.
    #[test]
    fn expand_inputs_survive_plugin_heap_growth() {
        let wat = plugin_heap_wat(
            "",
            &format!(
                r#"(func (export "expand") (param $name i32) (param $attrs i32) (param $body i32) (result i32)
                     {FILL_600K}
                     local.get $body)"#
            ),
        );
        let engine = Engine::default();
        let module = Module::new(&engine, wat).expect("compile plugin");
        let manifest: PluginManifest = toml::from_str(
            "[plugin]\nname = \"test.heap\"\nversion = \"1.0.0\"\ndescription = \"\"\n\
             author = \"\"\n\n[handles]\nblocks = [\"heapblock\"]\n",
        )
        .expect("manifest");
        let adapter = WasmPluginAdapter::new("test.heap".to_string(), manifest, module, engine)
            .expect("adapter");
        let block = FrameworkBlock {
            name: "heapblock:".to_string(),
            content: "print(\"hello from the input\")".to_string(),
            attributes: Vec::new(),
            location: None,
            body_location: None,
        };
        let statements = adapter
            .call_expand(&block)
            .expect("the plugin returns its intact input");
        assert!(
            format!("{statements:?}").contains("hello from the input"),
            "input was overwritten by the plugin heap: {statements:?}"
        );
    }

    /// Per-call reclamation spans the shared heap: an arena scope reclaims
    /// both host bridge allocations and the plugin's own allocations, so a
    /// scope run repeatedly leaves the heap where it started.
    #[test]
    fn arena_scope_reclaims_host_and_plugin_allocations() {
        let wat = plugin_heap_wat(
            r#"(import "env" "float_to_string" (func $f2s (param f64) (result i32)))
               (import "env" "_arena_scope_push" (func $push (result i32)))
               (import "env" "_arena_scope_pop" (func $pop (param i32)))"#,
            r#"(func (export "run") (result i32) (local $h i32)
                 call $push
                 local.set $h
                 f64.const 1.5
                 call $f2s
                 drop
                 i32.const 4096
                 call $malloc
                 drop
                 local.get $h
                 call $pop
                 global.get $heap)"#,
        );
        let (mut store, instance, _memory) = instantiate_with(&wat, |linker| {
            WasmPluginAdapter::register_env_core_functions(linker)?;
            WasmPluginAdapter::register_memory_runtime_functions(linker)
        });
        let run: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "run").unwrap();
        let first = run.call(&mut store, ()).expect("first run");
        let second = run.call(&mut store, ()).expect("second run");
        let third = run.call(&mut store, ()).expect("third run");
        assert_eq!(first, second, "a scope run must not grow the heap");
        assert_eq!(second, third, "a scope run must not grow the heap");
    }

    /// A negative LP pointer passed to `string.concat` used to overflow the
    /// bounds check and panic on the slice; it now yields a null result.
    #[test]
    fn concat_bridge_rejects_a_negative_pointer() {
        let wat = r#"
            (module
              (import "env" "string.concat" (func $cat (param i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "run") (result i32)
                i32.const -1
                i32.const -1
                call $cat))
        "#;
        let (mut store, instance, _memory) =
            instantiate_with(wat, WasmPluginAdapter::register_env_core_functions);
        let run: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "run").unwrap();
        assert_eq!(run.call(&mut store, ()).expect("no trap"), 0);
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
        let mut state = PluginState::new();
        state.build_state = std::sync::Arc::clone(&build_state);
        let mut store = Store::new(&engine, state);
        let mut linker: Linker<PluginState> = Linker::new(&engine);

        // Exercise the production code path. If the signature here drifts
        // away from what plugins import, instantiation will fail with the
        // exact "incompatible import type for `env::_build_state_set`" error
        // that bit frame.ui 2.12.36.
        super::register_build_state_bridges(&mut linker).expect("register_build_state_bridges");

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
            body_location: None,
        };

        let stmts = registry
            .expand_full(&block)
            .map(PluginExpansion::into_inline_statements)
            .expect("expand_block must not fail");

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
            body_location: None,
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
        let mut parser = SpecificationParser::new(tokens);
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
        let mut parser = SpecificationParser::new(tokens);
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

// ─────────────────────────────────────────────────────────────────────────────
// Assemble hook signature validation tests
//
// `foundation/spec/plugins/contracts/assemble.md` §5.3 requires the compiler
// to detect the ABI shape of `exports.assemble` from its WASM function
// signature at plugin load time and reject any signature that matches
// neither the v1 nor the v3 shape. These tests exercise
// `validate_assemble_export_signature` directly.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod assemble_signature_validation_tests {
    use super::validate_assemble_export_signature;
    use wasmtime::{Engine, Module};

    fn module(wat: &str) -> Module {
        let engine = Engine::default();
        Module::new(&engine, wat).expect("compile test WAT")
    }

    #[test]
    fn accepts_v1_signature() {
        let m = module(
            r#"(module
                (func (export "assemble") (param i32) (result i32) i32.const 0))"#,
        );
        validate_assemble_export_signature(&m, "test.plugin", "assemble")
            .expect("v1 (i32) -> i32 must be accepted per assemble.md §5.3");
    }

    #[test]
    fn accepts_v3_signature() {
        let m = module(
            r#"(module
                (func (export "assemble_typed") (param i32 i32) (result i32) i32.const 0))"#,
        );
        validate_assemble_export_signature(&m, "test.plugin", "assemble_typed")
            .expect("v3 (i32, i32) -> i32 must be accepted per assemble.md §5.3");
    }

    #[test]
    fn rejects_missing_export() {
        let m = module(r#"(module (func (export "other") (result i32) i32.const 0))"#);
        let err = validate_assemble_export_signature(&m, "test.plugin", "assemble")
            .expect_err("missing export must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("test.plugin"),
            "diagnostic names plugin: {}",
            msg
        );
        assert!(
            msg.contains("assemble.md"),
            "diagnostic links assemble contract: {}",
            msg
        );
    }

    #[test]
    fn rejects_zero_arity() {
        let m = module(r#"(module (func (export "assemble") (result i32) i32.const 0))"#);
        let err = validate_assemble_export_signature(&m, "test.plugin", "assemble")
            .expect_err("0-param signature matches neither v1 nor v3");
        assert!(
            err.to_string().contains("assemble.md"),
            "diagnostic must link the spec: {}",
            err
        );
    }

    #[test]
    fn rejects_three_params() {
        let m = module(
            r#"(module (func (export "assemble") (param i32 i32 i32) (result i32) i32.const 0))"#,
        );
        let err = validate_assemble_export_signature(&m, "test.plugin", "assemble")
            .expect_err("3-param signature matches neither v1 nor v3");
        assert!(err.to_string().contains("assemble.md"));
    }

    #[test]
    fn rejects_no_result() {
        let m = module(r#"(module (func (export "assemble") (param i32)))"#);
        let err = validate_assemble_export_signature(&m, "test.plugin", "assemble")
            .expect_err("no-result signature must be rejected");
        assert!(err.to_string().contains("assemble.md"));
    }

    #[test]
    fn rejects_non_i32_param() {
        let m =
            module(r#"(module (func (export "assemble") (param i64) (result i32) i32.const 0))"#);
        let err = validate_assemble_export_signature(&m, "test.plugin", "assemble")
            .expect_err("i64 param does not match v1 or v3");
        assert!(err.to_string().contains("assemble.md"));
    }

    #[test]
    fn rejects_non_i32_result() {
        let m =
            module(r#"(module (func (export "assemble") (param i32) (result i64) i64.const 0))"#);
        let err = validate_assemble_export_signature(&m, "test.plugin", "assemble")
            .expect_err("i64 result does not match v1 or v3");
        assert!(err.to_string().contains("assemble.md"));
    }

    #[test]
    fn rejects_non_function_export() {
        // A global export named `assemble` is not a function.
        let m = module(r#"(module (global (export "assemble") i32 (i32.const 0)))"#);
        let err = validate_assemble_export_signature(&m, "test.plugin", "assemble")
            .expect_err("global export must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("not a function"),
            "diagnostic mentions non-function: {}",
            msg
        );
        assert!(msg.contains("assemble.md"));
    }
}

/// Cycle 2 tests for Contract 5 lint hook signature validation and the
/// end-to-end WASM call path. See
/// `foundation/spec/framework/contracts/lint-extension.md`.
#[cfg(test)]
mod lint_signature_validation_tests {
    use super::validate_lint_export_signature;
    use wasmtime::{Engine, Module};

    fn module(wat: &str) -> Module {
        let engine = Engine::default();
        Module::new(&engine, wat).expect("compile test WAT")
    }

    #[test]
    fn accepts_i32_i32_signature() {
        let m = module(
            r#"(module
                (func (export "lint_project") (param i32) (result i32) i32.const 0))"#,
        );
        validate_lint_export_signature(&m, "test.plugin", "lint_project")
            .expect("(i32) -> i32 must be accepted per lint-extension.md §2");
    }

    #[test]
    fn rejects_dangling_export() {
        // Manifest says exports.lint = "lint_project" but the module has no
        // such export → PLUGIN001. This is the load-time gate that catches
        // typos in plugin manifests before any `cln lint` invocation.
        let m = module(r#"(module (func (export "other_export") (result i32) i32.const 0))"#);
        let err = validate_lint_export_signature(&m, "test.plugin", "lint_project")
            .expect_err("dangling export must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("has no export by that name"));
        assert!(msg.contains("lint-extension.md"));
    }

    #[test]
    fn rejects_wrong_arity() {
        let m = module(
            r#"(module
                (func (export "lint_project") (param i32 i32) (result i32) i32.const 0))"#,
        );
        let err = validate_lint_export_signature(&m, "test.plugin", "lint_project")
            .expect_err("2-param signature must be rejected");
        assert!(err.to_string().contains("lint-extension.md"));
    }

    #[test]
    fn rejects_non_i32_param() {
        let m = module(
            r#"(module
                (func (export "lint_project") (param i64) (result i32) i32.const 0))"#,
        );
        let err = validate_lint_export_signature(&m, "test.plugin", "lint_project")
            .expect_err("i64 param does not match (i32) -> i32");
        assert!(err.to_string().contains("lint-extension.md"));
    }

    #[test]
    fn rejects_non_i32_result() {
        let m = module(
            r#"(module
                (func (export "lint_project") (param i32) (result i64) i64.const 0))"#,
        );
        let err = validate_lint_export_signature(&m, "test.plugin", "lint_project")
            .expect_err("i64 result does not match (i32) -> i32");
        assert!(err.to_string().contains("lint-extension.md"));
    }

    #[test]
    fn rejects_non_function_export() {
        let m = module(r#"(module (global (export "lint_project") i32 (i32.const 0)))"#);
        let err = validate_lint_export_signature(&m, "test.plugin", "lint_project")
            .expect_err("global export must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("not a function"),
            "diagnostic mentions non-function: {}",
            msg
        );
        assert!(msg.contains("lint-extension.md"));
    }
}

/// Cycle 2 end-to-end integration test for the lint host bridges + the
/// call_lint_project path. Uses a hand-authored WAT plugin that ignores
/// its input pointer and returns a hard-coded LP JSON blob describing
/// two fake diagnostics.
///
/// This bypasses `WasmPluginAdapter::new` (which requires a full
/// PluginManifest) and drives the lint bridges + `LintArena` +
/// a lint-arena store + `read_lp_from_data` directly — the
/// same production code path a real plugin would exercise.
#[cfg(test)]
mod lint_e2e_tests {
    use super::*;
    use crate::ast::Program;
    use wasmtime::{Engine, Linker, Module, Store, TypedFunc};

    /// Minimal plugin module exporting `lint_project(i32) -> i32`.
    ///
    /// The plugin's `lint_project` ignores its parameter and returns a
    /// pointer to a hard-coded LP string embedded as static data. The
    /// LP string encodes a JSON array of two diagnostic entries per
    /// spec §3.2.
    ///
    /// Memory layout inside the plugin:
    ///   [0..1024]      reserved (heap start marker)
    ///   [1024..1028]   LP length prefix (u32 LE)
    ///   [1028..]       LP body (the JSON string)
    ///
    /// The static JSON body is:
    ///   [{"code":"TEST-E001","severity":"error","message":"first",
    ///     "location":{"file":"a.cln","line":1,"column":1}}]
    /// which is exactly 107 bytes (0x6b LE).
    const LINT_PLUGIN_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (global (export "__heap_ptr") (mut i32) (i32.const 1024))
  (data (i32.const 1024) "\6b\00\00\00[{\22code\22:\22TEST-E001\22,\22severity\22:\22error\22,\22message\22:\22first\22,\22location\22:{\22file\22:\22a.cln\22,\22line\22:1,\22column\22:1}}]")
  (func (export "lint_project") (param $input i32) (result i32)
    ;; Return the pointer to the static LP-string, ignoring the input.
    i32.const 1024))
"#;

    fn empty_program() -> Program {
        Program::new(None)
    }

    #[test]
    fn call_lint_project_end_to_end_returns_plugin_json() {
        let engine = Engine::default();
        let module = Module::new(&engine, LINT_PLUGIN_WAT).expect("compile lint plugin WAT");

        // Build the store with a LintArena — same as the production path.
        let handle = 42_i32;
        let arena = crate::plugins::lint::LintArena::new(handle, empty_program());
        let mut state = PluginState::new();
        state.lint_arena = Some(arena);
        let mut store = Store::new(&engine, state);

        // Build a lint linker manually (mirrors WasmPluginAdapter::lint_linker
        // without going through the manifest-bound adapter).
        let mut linker: Linker<PluginState> = Linker::new(&engine);
        crate::plugins::lint::register_lint_bridges(&mut linker).expect("register lint bridges");

        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate lint plugin");
        let memory = instance
            .get_memory(&mut store, "memory")
            .expect("plugin exports memory");

        // Call the plugin's lint_project with a dummy input pointer.
        let lint_fn: TypedFunc<i32, i32> = instance
            .get_typed_func(&mut store, "lint_project")
            .expect("plugin exports lint_project");
        let out_ptr = lint_fn.call(&mut store, 0).expect("lint_project call");

        // Read back the LP string using the same helper the production path uses.
        let data = memory.data(&store);
        let bytes = read_lp_from_data(data, out_ptr).expect("valid LP pointer");
        let json = std::str::from_utf8(bytes).expect("valid UTF-8");
        assert!(
            json.contains("TEST-E001"),
            "plugin JSON must contain the diagnostic code: {}",
            json
        );
        assert!(
            json.contains("\"severity\":\"error\""),
            "plugin JSON must carry severity: {}",
            json
        );
    }

    #[test]
    fn lint_bridges_return_ast_handle_invalid_on_stale_handle() {
        // Same fixture, but call one of the accessor bridges with a handle
        // that doesn't match the arena's active handle. Should return the
        // JSON error payload described in the arena.
        let bridge_probe_wat = r#"
(module
  (import "env" "_ast_list_classes" (func $list_classes (param i32) (result i32)))
  (memory (export "memory") 1)
  (global (export "__heap_ptr") (mut i32) (i32.const 1024))
  (func (export "probe") (param $stale_handle i32) (result i32)
    local.get $stale_handle
    call $list_classes))
"#;

        let engine = Engine::default();
        let module = Module::new(&engine, bridge_probe_wat).expect("compile probe WAT");

        let arena = crate::plugins::lint::LintArena::new(7, empty_program());
        let mut state = PluginState::new();
        state.lint_arena = Some(arena);
        let mut store = Store::new(&engine, state);

        let mut linker: Linker<PluginState> = Linker::new(&engine);
        crate::plugins::lint::register_lint_bridges(&mut linker).expect("register lint bridges");
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let memory = instance.get_memory(&mut store, "memory").expect("memory");

        // Call with a stale handle (99 instead of 7).
        let probe: TypedFunc<i32, i32> = instance
            .get_typed_func(&mut store, "probe")
            .expect("probe export");
        let out_ptr = probe.call(&mut store, 99).expect("probe call");

        let data = memory.data(&store);
        let bytes = read_lp_from_data(data, out_ptr).expect("valid LP from stale handle");
        let json = std::str::from_utf8(bytes).expect("valid UTF-8");
        assert!(
            json.contains("AST-HANDLE-INVALID"),
            "stale handle must produce AST-HANDLE-INVALID: {}",
            json
        );
        assert!(json.contains("\"handle\":99"));
    }
}
