//! WASM Plugin Adapter
//!
//! Wraps a WASM module to implement the FrameworkPlugin trait
//! Provides full Clean Language runtime environment for plugin execution

use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;
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

    /// Create a new store with host functions
    fn create_store(&self) -> Store<PluginState> {
        let state = PluginState::new();
        Store::new(&self.engine, state)
    }

    /// Get a reference to the cached linker, or create one if not yet cached.
    fn get_linker(&self) -> Result<&Linker<PluginState>> {
        self.cached_linker
            .as_ref()
            .ok_or_else(|| anyhow!("Linker not initialized"))
    }

    /// Set up the linker with host functions
    /// Provides the full Clean Language runtime environment
    fn setup_linker(&self) -> Result<Linker<PluginState>> {
        let mut linker = Linker::new(&self.engine);
        self.register_env_core_functions(&mut linker)?;
        self.register_string_dot_functions(&mut linker)?;
        self.register_string_underscore_functions(&mut linker)?;
        self.register_list_functions(&mut linker)?;
        self.register_memory_runtime_functions(&mut linker)?;
        self.register_http_client_functions(&mut linker)?;
        self.register_http_server_functions(&mut linker)?;
        self.register_request_context_functions(&mut linker)?;
        self.register_file_functions(&mut linker)?;
        self.register_math_functions(&mut linker)?;
        self.register_http_auth_stubs(&mut linker)?;
        self.register_build_state_bridges(&mut linker)?;
        Ok(linker)
    }

    /// Plugin Contracts v2 — register the `_build_state_set` /
    /// `_build_state_get` bridges that plugins use to communicate state
    /// across calls within one build. See `lifecycle.md` §2.5.
    ///
    /// **Calling convention:** both bridges take **LP-pointers** (single i32
    /// per string arg pointing to a `[length: 4 bytes LE][data]` allocation
    /// — `HOST_BRIDGE.md` §"LP-pointer convention"). This matches what Clean
    /// emits for `string` parameters declared in an `external:` block, so
    /// plugins can call these bridges directly without raw (ptr, len) glue.
    fn register_build_state_bridges(&self, linker: &mut Linker<PluginState>) -> Result<()> {
        // `_build_state_set(key_lp_ptr, value_lp_ptr) -> i32` — stores a value
        // under a key in the per-build state. Each argument is a single LP
        // pointer (length prefix at ptr, data at ptr+4).
        //
        // The `i32` return is a Clean convention quirk: every `external:` block
        // declaration emits a `(result i32)` import regardless of whether the
        // Clean-side return type is `void`. The host adapter MUST match the
        // imported signature, so void-style bridges return `0` here. Same
        // pattern as clean-server's `_http_respond` and every other host-side
        // void-return bridge.
        let state_for_set = std::sync::Arc::clone(&self.build_state);
        linker.func_wrap(
            "env",
            "_build_state_set",
            move |mut caller: Caller<'_, PluginState>, key_ptr: i32, value_ptr: i32| -> i32 {
                let Some(key) = read_clean_string(&mut caller, key_ptr) else {
                    return 0;
                };
                let Some(value) = read_clean_string(&mut caller, value_ptr) else {
                    return 0;
                };
                if key.is_empty() {
                    return 0;
                }
                if let Ok(mut guard) = state_for_set.lock() {
                    guard.insert(key, value);
                }
                0
            },
        )?;

        // `_build_state_get(key_lp_ptr) -> string_lp_ptr` — returns an LP
        // pointer to the value, or an empty LP string if the key is absent.
        let state_for_get = std::sync::Arc::clone(&self.build_state);
        linker.func_wrap(
            "env",
            "_build_state_get",
            move |mut caller: Caller<'_, PluginState>, key_ptr: i32| -> i32 {
                let Some(key) = read_clean_string(&mut caller, key_ptr) else {
                    return 0;
                };
                let value = state_for_get
                    .lock()
                    .ok()
                    .and_then(|g| g.get(&key).cloned())
                    .unwrap_or_default();
                let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) else {
                    return 0;
                };
                write_lp_string(&mut caller, &memory, &value).unwrap_or(0)
            },
        )?;

        Ok(())
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
        linker.func_wrap(
            "env",
            "string_compare",
            |mut caller: Caller<'_, PluginState>, ptr1: i32, ptr2: i32| -> i32 {
                let (string1, string2) = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);

                    // Read first string
                    let len_start1 = ptr1 as usize;
                    let len_bytes1: [u8; 4] = data[len_start1..len_start1 + 4].try_into().unwrap();
                    let len1 = u32::from_le_bytes(len_bytes1) as usize;
                    let data_start1 = len_start1 + 4;
                    let s1 =
                        String::from_utf8_lossy(&data[data_start1..data_start1 + len1]).to_string();

                    // Read second string
                    let len_start2 = ptr2 as usize;
                    let len_bytes2: [u8; 4] = data[len_start2..len_start2 + 4].try_into().unwrap();
                    let len2 = u32::from_le_bytes(len_bytes2) as usize;
                    let data_start2 = len_start2 + 4;
                    let s2 =
                        String::from_utf8_lossy(&data[data_start2..data_start2 + len2]).to_string();

                    (s1, s2)
                };
                if string1 == string2 {
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

                    // Read source string
                    let len_start1 = string_ptr as usize;
                    let len_bytes1: [u8; 4] = data[len_start1..len_start1 + 4].try_into().unwrap();
                    let len1 = u32::from_le_bytes(len_bytes1) as usize;
                    let data_start1 = len_start1 + 4;
                    let s1 =
                        String::from_utf8_lossy(&data[data_start1..data_start1 + len1]).to_string();

                    // Read search string
                    let len_start2 = search_ptr as usize;
                    let len_bytes2: [u8; 4] = data[len_start2..len_start2 + 4].try_into().unwrap();
                    let len2 = u32::from_le_bytes(len_bytes2) as usize;
                    let data_start2 = len_start2 + 4;
                    let s2 =
                        String::from_utf8_lossy(&data[data_start2..data_start2 + len2]).to_string();

                    // Read replace string
                    let len_start3 = replace_ptr as usize;
                    let len_bytes3: [u8; 4] = data[len_start3..len_start3 + 4].try_into().unwrap();
                    let len3 = u32::from_le_bytes(len_bytes3) as usize;
                    let data_start3 = len_start3 + 4;
                    let s3 =
                        String::from_utf8_lossy(&data[data_start3..data_start3 + len3]).to_string();

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

                    // Read source string
                    let len_start1 = string_ptr as usize;
                    let len_bytes1: [u8; 4] = data[len_start1..len_start1 + 4].try_into().unwrap();
                    let len1 = u32::from_le_bytes(len_bytes1) as usize;
                    let data_start1 = len_start1 + 4;
                    let s1 =
                        String::from_utf8_lossy(&data[data_start1..data_start1 + len1]).to_string();

                    // Read search string
                    let len_start2 = search_ptr as usize;
                    let len_bytes2: [u8; 4] = data[len_start2..len_start2 + 4].try_into().unwrap();
                    let len2 = u32::from_le_bytes(len_bytes2) as usize;
                    let data_start2 = len_start2 + 4;
                    let s2 =
                        String::from_utf8_lossy(&data[data_start2..data_start2 + len2]).to_string();

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

                    let len_start1 = string_ptr as usize;
                    let len_bytes1: [u8; 4] = data[len_start1..len_start1 + 4].try_into().unwrap();
                    let len1 = u32::from_le_bytes(len_bytes1) as usize;
                    let data_start1 = len_start1 + 4;
                    let s1 =
                        String::from_utf8_lossy(&data[data_start1..data_start1 + len1]).to_string();

                    let len_start2 = search_ptr as usize;
                    let len_bytes2: [u8; 4] = data[len_start2..len_start2 + 4].try_into().unwrap();
                    let len2 = u32::from_le_bytes(len_bytes2) as usize;
                    let data_start2 = len_start2 + 4;
                    let s2 =
                        String::from_utf8_lossy(&data[data_start2..data_start2 + len2]).to_string();

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
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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

                    let len_start1 = string_ptr as usize;
                    let len_bytes1: [u8; 4] = data[len_start1..len_start1 + 4].try_into().unwrap();
                    let len1 = u32::from_le_bytes(len_bytes1) as usize;
                    let data_start1 = len_start1 + 4;
                    let s1 =
                        String::from_utf8_lossy(&data[data_start1..data_start1 + len1]).to_string();

                    let len_start2 = prefix_ptr as usize;
                    let len_bytes2: [u8; 4] = data[len_start2..len_start2 + 4].try_into().unwrap();
                    let len2 = u32::from_le_bytes(len_bytes2) as usize;
                    let data_start2 = len_start2 + 4;
                    let s2 =
                        String::from_utf8_lossy(&data[data_start2..data_start2 + len2]).to_string();

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

                    let len_start1 = string_ptr as usize;
                    let len_bytes1: [u8; 4] = data[len_start1..len_start1 + 4].try_into().unwrap();
                    let len1 = u32::from_le_bytes(len_bytes1) as usize;
                    let data_start1 = len_start1 + 4;
                    let s1 =
                        String::from_utf8_lossy(&data[data_start1..data_start1 + len1]).to_string();

                    let len_start2 = suffix_ptr as usize;
                    let len_bytes2: [u8; 4] = data[len_start2..len_start2 + 4].try_into().unwrap();
                    let len2 = u32::from_le_bytes(len_bytes2) as usize;
                    let data_start2 = len_start2 + 4;
                    let s2 =
                        String::from_utf8_lossy(&data[data_start2..data_start2 + len2]).to_string();

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

                    let len_start1 = string_ptr as usize;
                    let len_bytes1: [u8; 4] = data[len_start1..len_start1 + 4].try_into().unwrap();
                    let len1 = u32::from_le_bytes(len_bytes1) as usize;
                    let data_start1 = len_start1 + 4;
                    let s1 =
                        String::from_utf8_lossy(&data[data_start1..data_start1 + len1]).to_string();

                    let len_start2 = search_ptr as usize;
                    let len_bytes2: [u8; 4] = data[len_start2..len_start2 + 4].try_into().unwrap();
                    let len2 = u32::from_le_bytes(len_bytes2) as usize;
                    let data_start2 = len_start2 + 4;
                    let s2 =
                        String::from_utf8_lossy(&data[data_start2..data_start2 + len2]).to_string();

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
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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

                    let len_start1 = string_ptr as usize;
                    let len_bytes1: [u8; 4] = data[len_start1..len_start1 + 4].try_into().unwrap();
                    let len1 = u32::from_le_bytes(len_bytes1) as usize;
                    let data_start1 = len_start1 + 4;
                    let s1 =
                        String::from_utf8_lossy(&data[data_start1..data_start1 + len1]).to_string();

                    let len_start2 = search_ptr as usize;
                    let len_bytes2: [u8; 4] = data[len_start2..len_start2 + 4].try_into().unwrap();
                    let len2 = u32::from_le_bytes(len_bytes2) as usize;
                    let data_start2 = len_start2 + 4;
                    let s2 =
                        String::from_utf8_lossy(&data[data_start2..data_start2 + len2]).to_string();

                    let len_start3 = replace_ptr as usize;
                    let len_bytes3: [u8; 4] = data[len_start3..len_start3 + 4].try_into().unwrap();
                    let len3 = u32::from_le_bytes(len_bytes3) as usize;
                    let data_start3 = len_start3 + 4;
                    let s3 =
                        String::from_utf8_lossy(&data[data_start3..data_start3 + len3]).to_string();

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
        linker.func_wrap(
            "env",
            "string.repeat",
            |mut caller: Caller<'_, PluginState>, ptr: i32, count: i32| -> i32 {
                let string_val = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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

                    let len_start1 = string_ptr as usize;
                    let len_bytes1: [u8; 4] = data[len_start1..len_start1 + 4].try_into().unwrap();
                    let len1 = u32::from_le_bytes(len_bytes1) as usize;
                    let data_start1 = len_start1 + 4;
                    let s1 =
                        String::from_utf8_lossy(&data[data_start1..data_start1 + len1]).to_string();

                    let len_start2 = search_ptr as usize;
                    let len_bytes2: [u8; 4] = data[len_start2..len_start2 + 4].try_into().unwrap();
                    let len2 = u32::from_le_bytes(len_bytes2) as usize;
                    let data_start2 = len_start2 + 4;
                    let s2 =
                        String::from_utf8_lossy(&data[data_start2..data_start2 + len2]).to_string();

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
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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
                    let len_start = ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
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
        // memory_runtime.mem_alloc - Allocate memory
        linker.func_wrap(
            "memory_runtime",
            "mem_alloc",
            |mut caller: Caller<'_, PluginState>, size: i32, _align: i32| -> i32 {
                let state = caller.data_mut();
                state.allocate(size as usize) as i32
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

        // ORM verb direct dispatch: Model.verb blocks (e.g. "User.find") are handled by
        // calling the plugin's specific export (expand_find, expand_first, etc.) directly
        // rather than routing through expand_block.  The frame.data expand_block dispatcher
        // uses an inverted string_compare convention incompatible with the host's current
        // implementation; the verb-specific exports do not have this issue.
        //
        // expand_find(model: string, body: string) -> string
        //   model — the model class name (e.g. "User"), converted to snake_case inside the plugin
        //   body  — the query sub-clauses (where:, order:, limit:, etc.)
        //   returns the RHS query expression (e.g. _db_query("SELECT ...", "[]"))
        //
        // The block content carries the variable binding as a header line ("type name =")
        // followed by the actual sub-clauses.  We strip the header, call the verb export,
        // then reassemble the complete assignment statement for parsing.
        if let Some(dot_pos) = block_name.rfind('.') {
            let verb = &block_name[dot_pos + 1..];
            let model_name = &block_name[..dot_pos];
            if matches!(
                verb,
                "find"
                    | "first"
                    | "count"
                    | "exists"
                    | "insert"
                    | "update"
                    | "delete"
                    | "paginate"
                    | "cursor"
            ) {
                let direct_fn = format!("expand_{}", verb);
                if let Ok(expand_verb) =
                    instance.get_typed_func::<(i32, i32), i32>(&mut store, &direct_fn)
                {
                    // Split block content into header ("list<User> rows =") and body subclauses.
                    //
                    // Not all ORM verbs bind their result to a variable.  `insert:`, `update:`,
                    // and `delete:` blocks typically have no binding — their content is entirely
                    // field assignments (e.g. `name = value`).  `find:`, `first:`, and `count:`
                    // blocks start with a binding header of the form `<type> <identifier> =`.
                    //
                    // We distinguish the two cases by checking how many whitespace-separated
                    // tokens appear before the first `=` on the first line:
                    //   • Two tokens (e.g. `list<User> rows =`) → binding header is present
                    //   • One token  (e.g. `page_id =`)         → no binding, all content is body
                    let content = &block.content;
                    let has_binding_header = {
                        let first_line = content.lines().next().unwrap_or("").trim();
                        if let Some(eq_pos) = first_line.find('=') {
                            let before_eq = first_line[..eq_pos].trim();
                            // Count whitespace-separated tokens (type may contain '<' / '>'
                            // so we split on ASCII whitespace only).
                            let token_count = before_eq.split_whitespace().count();
                            token_count >= 2
                        } else {
                            false
                        }
                    };

                    let (header_line, sub_body) = if has_binding_header {
                        if let Some(newline_pos) = content.find('\n') {
                            (&content[..newline_pos], &content[newline_pos + 1..])
                        } else {
                            (content.as_str(), "")
                        }
                    } else {
                        // No binding header — treat the entire content as the body.
                        ("", content.as_str())
                    };

                    let model_ptr = self.find_or_write_string(&mut store, &memory, model_name)?;
                    let body_ptr = self.find_or_write_string(&mut store, &memory, sub_body)?;

                    let result_ptr = expand_verb.call(&mut store, (model_ptr, body_ptr))?;

                    if let Some(error) = store.data().last_error.clone() {
                        return Err(anyhow!("Plugin error: {}", error));
                    }

                    let result_bytes = self.read_result(&store, &memory, result_ptr)?;
                    let query_expr = std::str::from_utf8(&result_bytes)
                        .map_err(|e| anyhow!("Invalid UTF-8 in plugin response: {}", e))?
                        .trim();

                    tracing::trace!(
                        verb = verb,
                        model = model_name,
                        has_binding_header = has_binding_header,
                        query_expr_len = query_expr.len(),
                        "ORM verb direct dispatch result"
                    );

                    if query_expr.is_empty() {
                        return Ok(Vec::new());
                    }

                    // Reassemble the statement.  When a binding header is present:
                    //   list<User> rows = _db_query(...)
                    // When there is no binding (insert/update/delete used as a statement):
                    //   _db_exec(...)
                    let full_stmt = if has_binding_header {
                        format!("{} {}", header_line, query_expr)
                    } else {
                        query_expr.to_string()
                    };
                    let wrapper = format!("start:\n\t{}", full_stmt.trim().replace('\n', "\n\t"));
                    let program = self.parse_plugin_code(&wrapper).map_err(|e| {
                        anyhow!(
                            "Failed to parse ORM verb plugin output '{}': {}",
                            full_stmt.chars().take(120).collect::<String>(),
                            e
                        )
                    })?;
                    let statements = program.start_function.map(|f| f.body).unwrap_or_default();
                    return Ok(statements);
                }
            }
        }

        // Try to find an existing string pointer in the plugin's memory that matches
        // Clean Language uses pointer equality for string comparison, so we need
        // to return the same pointer the plugin uses for its string literals
        let block_name_ptr = self.find_or_write_string(&mut store, &memory, block_name)?;

        // Extract inline key="value" pairs from the first line of content
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

        let expand_fn_name = &self.manifest.exports.expand;
        let expand: TypedFunc<(i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, expand_fn_name)
            .map_err(|e| {
                anyhow!(
                    "Plugin does not export '{}' function: {}",
                    expand_fn_name,
                    e
                )
            })?;
        let result_ptr = expand.call(&mut store, (block_name_ptr, attributes_ptr, body_ptr))?;

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

        Ok(statements)
    }

    /// Call the expand function and return full expansion result
    ///
    /// This version preserves the start function if the plugin generates one
    fn call_expand_full(&self, block: &FrameworkBlock) -> Result<PluginExpansion> {
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

        let expand_fn_name = &self.manifest.exports.expand;
        let expand: TypedFunc<(i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, expand_fn_name)
            .map_err(|e| {
                anyhow!(
                    "Plugin does not export '{}' function: {}",
                    expand_fn_name,
                    e
                )
            })?;
        let result_ptr = expand.call(&mut store, (block_name_ptr, attributes_ptr, body_ptr))?;

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

        Ok(PluginExpansion {
            statements: Vec::new(),
            start_function: program.start_function,
            functions: program.functions,
            classes: program.classes,
            externals: program.externals,
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

        let result_ptr = slot.call(&mut store, context_ptr)?;

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
        let parsed = self.parse_plugin_code(&response.statements)?;
        // The slot is conceptually contributing statement-level code. Most
        // plugins will return just statements; for plugins that include a
        // start function (e.g. legacy compatibility shims), merge its body
        // into the statements list.
        let mut statements = parsed.statements;
        if let Some(start_fn) = parsed.start_function {
            statements.extend(start_fn.body);
        }
        Ok(PluginExpansion {
            statements,
            start_function: None,
            functions: parsed.functions,
            classes: parsed.classes,
            externals: parsed.externals,
        })
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

        let result_ptr = hook.call(&mut store, ())?;

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

/// State held by the WASM store
struct PluginState {
    /// Simple bump allocator offset
    alloc_offset: usize,
    /// Last error reported by plugin
    last_error: Option<String>,
}

impl PluginState {
    fn new() -> Self {
        Self {
            // Start allocations at 512KB to avoid collision with WASM data section
            // and the module's internal heap which can grow from lower addresses
            // The WASM module's static data starts at 4KB and heap follows data section
            // For large plugins with many string operations, 512KB should be safe
            alloc_offset: 524288,
            last_error: None,
        }
    }

    fn allocate(&mut self, size: usize) -> usize {
        let ptr = self.alloc_offset;
        // Align to 8 bytes
        self.alloc_offset = (self.alloc_offset + size + 7) & !7;
        ptr
    }
}

/// Helper to write a UTF-8 string as a Clean length-prefixed allocation
/// and return its pointer. Wraps `write_clean_string` with a `Result`-style
/// signature that yields `None` on memory failure rather than a sentinel 0.
fn write_lp_string(caller: &mut Caller<'_, PluginState>, _memory: &Memory, s: &str) -> Option<i32> {
    let ptr = write_clean_string(caller, s.as_bytes());
    if ptr == 0 {
        None
    } else {
        Some(ptr)
    }
}

fn read_clean_string(caller: &mut Caller<'_, PluginState>, ptr: i32) -> Option<String> {
    let memory = caller.get_export("memory").and_then(|e| e.into_memory())?;
    let data = memory.data(&*caller);

    let len_start = ptr as usize;
    if len_start + 4 > data.len() {
        return None;
    }

    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().ok()?;
    let len = u32::from_le_bytes(len_bytes) as usize;

    let data_start = len_start + 4;
    let data_end = data_start + len;

    if data_end > data.len() {
        return None;
    }

    std::str::from_utf8(&data[data_start..data_end])
        .ok()
        .map(|s| s.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

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
