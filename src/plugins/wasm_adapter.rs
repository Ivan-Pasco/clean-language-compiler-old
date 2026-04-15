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

        let mut adapter = Self {
            name,
            manifest,
            module,
            engine,
            handles_cache,
            name_cache,
            version_cache,
            description_cache,
            cached_linker: None,
        };

        // Pre-build the linker once — this sets up ~50+ host function stubs
        // and is expensive to do per-block call.
        let linker = adapter.setup_linker()?;
        adapter.cached_linker = Some(linker);

        Ok(adapter)
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

        // =========================================
        // ENV NAMESPACE - Core I/O and conversions
        // =========================================

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
                    let pages_needed = ((required_size - current_size) + 65535) / 65536;
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

        // env.string.split - Split string (stub)
        linker.func_wrap(
            "env",
            "string.split",
            |mut caller: Caller<'_, PluginState>, _str_ptr: i32, _delim_ptr: i32| -> i32 {
                // Return empty array
                let state = caller.data_mut();
                let ptr = state.allocate(4);
                if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let _ = memory.write(&mut caller, ptr, &[0u8; 4]);
                }
                ptr as i32
            },
        )?;

        // env.string_trim - Remove leading and trailing whitespace
        // String format: [4-byte length][data]
        linker.func_wrap(
            "env",
            "string_trim",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                // Use the read_clean_string helper pattern
                let original = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let len_start = str_ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
                };
                let trimmed = original.trim();

                // Allocate and write result
                let result = trimmed.as_bytes();
                let result_len = result.len();
                let state = caller.data_mut();
                let ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, ptr, &len_bytes);
                let _ = memory.write(&mut caller, ptr + 4, result);
                ptr as i32
            },
        )?;

        // env.string_trim_start - Remove leading whitespace
        linker.func_wrap(
            "env",
            "string_trim_start",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let len_start = str_ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
                };
                let trimmed = original.trim_start();

                let result = trimmed.as_bytes();
                let result_len = result.len();
                let state = caller.data_mut();
                let ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, ptr, &len_bytes);
                let _ = memory.write(&mut caller, ptr + 4, result);
                ptr as i32
            },
        )?;

        // env.string_trim_end - Remove trailing whitespace
        linker.func_wrap(
            "env",
            "string_trim_end",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = {
                    let memory = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .unwrap();
                    let data = memory.data(&caller);
                    let len_start = str_ptr as usize;
                    let len_bytes: [u8; 4] = data[len_start..len_start + 4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    let data_start = len_start + 4;
                    String::from_utf8_lossy(&data[data_start..data_start + len]).to_string()
                };
                let trimmed = original.trim_end();

                let result = trimmed.as_bytes();
                let result_len = result.len();
                let state = caller.data_mut();
                let ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, ptr, &len_bytes);
                let _ = memory.write(&mut caller, ptr + 4, result);
                ptr as i32
            },
        )?;

        // =========================================
        // DOT-NOTATION TRIM ALIASES
        // For compatibility with WASM modules using dot notation
        // =========================================

        // env.string.trim - Alias for string_trim (dot notation)
        linker.func_wrap(
            "env",
            "string.trim",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = {
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
                let trimmed = original.trim();

                let result = trimmed.as_bytes();
                let result_len = result.len();
                let state = caller.data_mut();
                let ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, ptr, &len_bytes);
                let _ = memory.write(&mut caller, ptr + 4, result);
                ptr as i32
            },
        )?;

        // env.string.trimStart - Alias for string_trim_start (dot notation)
        linker.func_wrap(
            "env",
            "string.trimStart",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = {
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
                let trimmed = original.trim_start();

                let result = trimmed.as_bytes();
                let result_len = result.len();
                let state = caller.data_mut();
                let ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, ptr, &len_bytes);
                let _ = memory.write(&mut caller, ptr + 4, result);
                ptr as i32
            },
        )?;

        // env.string.trimEnd - Alias for string_trim_end (dot notation)
        linker.func_wrap(
            "env",
            "string.trimEnd",
            |mut caller: Caller<'_, PluginState>, str_ptr: i32| -> i32 {
                let original = {
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
                let trimmed = original.trim_end();

                let result = trimmed.as_bytes();
                let result_len = result.len();
                let state = caller.data_mut();
                let ptr = state.allocate(result_len + 4);
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, ptr, &len_bytes);
                let _ = memory.write(&mut caller, ptr + 4, result);
                ptr as i32
            },
        )?;

        // env.string_compare - Compare two strings
        // Returns 1 if equal, 0 if not equal
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
                    1
                } else {
                    0
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
                    let pages_needed = ((required_size - current_size) + 65535) / 65536;
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

        // env.string_from_char_code - Create string from character code
        // string_from_char_code(char_code: i32) -> i32
        linker.func_wrap(
            "env",
            "string_from_char_code",
            |mut caller: Caller<'_, PluginState>, char_code: i32| -> i32 {
                // Create a single-character string from the char code
                let ch = if char_code >= 0 && char_code <= 127 {
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
                    let pages_needed = ((required_size - current_size) + 65535) / 65536;
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

        // =========================================
        // UNDERSCORE-STYLE STRING ALIASES
        // For compatibility with different WASM naming conventions
        // =========================================

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

        // =========================================
        // LIST NAMESPACE - List/array operations
        // =========================================

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

        // =========================================
        // MEMORY_RUNTIME NAMESPACE - Memory management
        // =========================================

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

        // =========================================
        // HTTP NAMESPACE - Network operations (stubs)
        // =========================================

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

        // =========================================
        // HTTP SERVER NAMESPACE - Server functions (stubs)
        // These are for Frame runtime, stubbed for plugin execution
        // =========================================

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

        // =========================================
        // REQUEST CONTEXT ACCESS - Stubs for plugins
        // =========================================

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

        // =========================================
        // FILE NAMESPACE - File operations (stubs)
        // =========================================

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

        // =========================================
        // MATH NAMESPACE - Math operations
        // Required by compiled WASM modules for stdlib functions
        // =========================================

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

        // =========================================
        // ADDITIONAL HTTP/AUTH STUBS - These are functions that plugins generate
        // as OUTPUT but don't actually call. Only add ones not already defined above.
        // =========================================

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
        // _http_set_cookie: (cookie_ptr: i32, cookie_len: i32) -> i32
        linker.func_wrap(
            "env",
            "_http_set_cookie",
            |_: Caller<'_, PluginState>, _: i32, _: i32| -> i32 { 0 },
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

        Ok(linker)
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

        // Pre-strip common indentation from block body before passing to plugin.
        let stripped_body = strip_common_indent(&actual_body);
        let body_ptr = self.find_or_write_string(&mut store, &memory, &stripped_body)?;

        // For html: blocks, bypass expand_block and call html_block_to_code directly.
        // The plugin's strip_block_indent returns empty due to a WASM codegen bug,
        // so we pre-strip indentation in Rust (strip_common_indent) and pass the
        // cleaned body directly to html_block_to_code.
        let result_ptr = if block_name == "html" {
            if let Ok(html_fn) =
                instance.get_typed_func::<i32, i32>(&mut store, "html_block_to_code")
            {
                let code_ptr = html_fn.call(&mut store, body_ptr)?;
                let code_bytes = self.read_result(&store, &memory, code_ptr)?;
                let code_str = std::str::from_utf8(&code_bytes).unwrap_or("");

                // Check for var="name" attribute to determine mode
                let var_name = if attributes_str.contains("\"var\"") {
                    attributes_str
                        .split("\"var\":\"")
                        .nth(1)
                        .and_then(|s| s.split('"').next())
                        .unwrap_or("")
                } else {
                    ""
                };

                let generated = if var_name.is_empty() {
                    format!("string __html = \"\"\n{}return __html\n", code_str)
                } else {
                    let remapped = code_str.replace("__html", var_name);
                    format!("string {} = \"\"\n{}", var_name, remapped)
                };

                self.write_clean_string(&mut store, &memory, &generated)?
            } else {
                // Fallback: call expand_block normally
                let expand_fn_name = &self.manifest.exports.expand;
                let expand: TypedFunc<(i32, i32, i32), i32> =
                    instance.get_typed_func(&mut store, expand_fn_name)?;
                expand.call(&mut store, (block_name_ptr, attributes_ptr, body_ptr))?
            }
        } else {
            // For all other block types, call expand_block normally
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
            expand.call(&mut store, (block_name_ptr, attributes_ptr, body_ptr))?
        };

        // Check for errors
        if let Some(error) = store.data().last_error.clone() {
            return Err(anyhow!("Plugin error: {}", error));
        }

        // Read the result (Clean string format)
        let result_bytes = self.read_result(&store, &memory, result_ptr)?;

        // The plugin returns Clean Language source code, which we parse
        let generated_code = std::str::from_utf8(&result_bytes)
            .map_err(|e| anyhow!("Invalid UTF-8 in plugin response: {}", e))?;

        // Log expansion result at trace level for debugging
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
        match self.parse_plugin_code(generated_code) {
            Ok(program) => {
                if let Some(start_fn) = program.start_function {
                    return Ok(start_fn.body);
                }
                // Plugin returned classes/functions but no start block —
                // there are no imperative statements to return.
                return Ok(Vec::new());
            }
            Err(_) => {}
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
                    if line.starts_with('\t') {
                        &line[1..]
                    } else if line.starts_with("    ") {
                        &line[4..]
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
        // Pre-strip common indentation from block body
        let stripped_body = strip_common_indent(&actual_body);
        let body_ptr = self.find_or_write_string(&mut store, &memory, &stripped_body)?;

        // Same html: block handling as call_expand — bypass expand_block,
        // call html_block_to_code directly with pre-stripped body.
        let result_ptr = if block_name == "html" {
            if let Ok(html_fn) =
                instance.get_typed_func::<i32, i32>(&mut store, "html_block_to_code")
            {
                let code_ptr = html_fn.call(&mut store, body_ptr)?;
                let code_bytes = self.read_result(&store, &memory, code_ptr)?;
                let code_str = std::str::from_utf8(&code_bytes).unwrap_or("");

                let var_name = if attributes_str.contains("\"var\"") {
                    attributes_str
                        .split("\"var\":\"")
                        .nth(1)
                        .and_then(|s| s.split('"').next())
                        .unwrap_or("")
                } else {
                    ""
                };

                let generated = if var_name.is_empty() {
                    format!("string __html = \"\"\n{}return __html\n", code_str)
                } else {
                    let remapped = code_str.replace("__html", var_name);
                    format!("string {} = \"\"\n{}", var_name, remapped)
                };

                self.write_clean_string(&mut store, &memory, &generated)?
            } else {
                let expand_fn_name = &self.manifest.exports.expand;
                let expand: TypedFunc<(i32, i32, i32), i32> =
                    instance.get_typed_func(&mut store, expand_fn_name)?;
                expand.call(&mut store, (block_name_ptr, attributes_ptr, body_ptr))?
            }
        } else {
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
            expand.call(&mut store, (block_name_ptr, attributes_ptr, body_ptr))?
        };

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
        let code_without_start = if generated_code.trim().starts_with("start:") {
            generated_code
                .lines()
                .skip(1)
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    if line.starts_with('\t') {
                        &line[1..]
                    } else if line.starts_with("    ") {
                        &line[4..]
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
        let total_size = ((4 + len + 7) / 8) * 8;

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

/// Helper to read a Clean string from WASM memory
/// Clean strings are stored as [4-byte length][data]
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

            if after_eq.starts_with('"') {
                // Find closing quote
                if let Some(close_pos) = after_eq[1..].find('"') {
                    let value = &after_eq[1..close_pos + 1];
                    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                    pairs.push(format!("\"{}\":\"{}\"", key, escaped));
                    remaining = after_eq[close_pos + 2..].trim_start();
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

/// Strip common leading indentation from block body content.
///
/// This replicates the logic that plugins perform in `strip_block_indent`,
/// working around a WASM codegen bug where the plugin's implementation
/// returns empty string due to local variable scoping issues in compiled WASM.
///
/// The function:
/// 1. Finds the minimum tab indentation across all non-empty lines
/// 2. Strips that many leading tabs from each line
/// 3. Joins non-empty lines with spaces (matching plugin behavior)
fn strip_common_indent(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();

    // Find minimum indent (number of leading tabs) across non-empty lines
    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|c| *c == '\t').count())
        .min()
        .unwrap_or(0);

    // Rebuild with indent stripped, joining non-empty lines with spaces
    lines
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else if line.len() > min_indent {
                Some(line.chars().skip(min_indent).collect::<String>())
            } else {
                Some(trimmed.to_string())
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
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
}
