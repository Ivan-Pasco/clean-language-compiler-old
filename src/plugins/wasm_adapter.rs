//! WASM Plugin Adapter
//!
//! Wraps a WASM module to implement the FrameworkPlugin trait
//! Provides full Clean Language runtime environment for plugin execution

use anyhow::{anyhow, Result};
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
    /// Plugin manifest (kept for future version compatibility checks)
    #[allow(dead_code)]
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

        Ok(Self {
            name,
            manifest,
            module,
            engine,
            handles_cache,
            name_cache,
            version_cache,
            description_cache,
        })
    }

    /// Create a new store with host functions
    fn create_store(&self) -> Store<PluginState> {
        let state = PluginState::new();
        Store::new(&self.engine, state)
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

        // env.string_concat - Concatenate two strings (CRITICAL for plugins)
        linker.func_wrap(
            "env",
            "string_concat",
            |mut caller: Caller<'_, PluginState>,
             str1_ptr: i32,
             str1_len: i32,
             str2_ptr: i32,
             str2_len: i32|
             -> i32 {
                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return 0,
                };

                let data = memory.data(&caller);

                // Read first string
                let s1_start = str1_ptr as usize;
                let s1_end = s1_start + str1_len as usize;
                let s1 = if s1_end <= data.len() {
                    data[s1_start..s1_end].to_vec()
                } else {
                    Vec::new()
                };

                // Read second string
                let s2_start = str2_ptr as usize;
                let s2_end = s2_start + str2_len as usize;
                let s2 = if s2_end <= data.len() {
                    data[s2_start..s2_end].to_vec()
                } else {
                    Vec::new()
                };

                // Concatenate
                let mut result = s1;
                result.extend(s2);
                let result_len = result.len();

                // Allocate and write result
                let state = caller.data_mut();
                let ptr = state.allocate(4 + result_len);

                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();
                let len_bytes = (result_len as u32).to_le_bytes();
                let _ = memory.write(&mut caller, ptr, &len_bytes);
                let _ = memory.write(&mut caller, ptr + 4, &result);

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
        // Legacy host functions (for compatibility)
        // =========================================

        // env.debug_print (legacy)
        linker.func_wrap(
            "env",
            "debug_print",
            |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| {
                if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let data = memory.data(&caller);
                    if let Some(slice) = data.get(ptr as usize..(ptr + len) as usize) {
                        if let Ok(s) = std::str::from_utf8(slice) {
                            eprintln!("[Plugin Debug] {}", s);
                        }
                    }
                }
            },
        )?;

        // env.alloc (legacy)
        linker.func_wrap(
            "env",
            "alloc",
            |mut caller: Caller<'_, PluginState>, size: i32| -> i32 {
                let state = caller.data_mut();
                state.allocate(size as usize) as i32
            },
        )?;

        // env.report_error (legacy)
        linker.func_wrap(
            "env",
            "report_error",
            |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| {
                if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let data = memory.data(&caller);
                    if let Some(slice) = data.get(ptr as usize..(ptr + len) as usize) {
                        if let Ok(s) = std::str::from_utf8(slice) {
                            caller.data_mut().last_error = Some(s.to_string());
                        }
                    }
                }
            },
        )?;

        Ok(linker)
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
        let linker = self.setup_linker()?;

        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|e| anyhow!("Failed to instantiate plugin module: {}", e))?;

        // Get memory
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("Plugin does not export memory"))?;

        // Strip trailing colon from block name (e.g., "server:" -> "server")
        let block_name = block.name.trim_end_matches(':');

        // Try to find an existing string pointer in the plugin's memory that matches
        // Clean Language uses pointer equality for string comparison, so we need
        // to return the same pointer the plugin uses for its string literals
        let block_name_ptr = self.find_or_write_string(&mut store, &memory, block_name)?;

        // Format attributes as a simple string (name=value pairs)
        let attributes_str = block
            .attributes
            .iter()
            .map(|attr| {
                if let Some(ref val) = attr.value {
                    format!("{}={}", attr.name, val)
                } else {
                    attr.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let attributes_ptr = self.find_or_write_string(&mut store, &memory, &attributes_str)?;

        // Use the raw content string as the body (it's already in Clean syntax)
        let body_ptr = self.find_or_write_string(&mut store, &memory, &block.content)?;

        // Call the expand function with signature: (i32, i32, i32) -> i32
        // Three string pointers, returns a string pointer
        let expand: TypedFunc<(i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, "expand")
            .map_err(|e| anyhow!("Plugin does not export 'expand' function: {}", e))?;

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

        // Handle empty result
        if generated_code.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Plugin output is typically a "start:" block like:
        //   start:
        //       _http_listen(3000)
        //
        // We need to extract the statements from this format.
        // First, try parsing as a full program (in case plugin returns complete code)
        if let Ok(program) = crate::parser::CleanParser::parse_program(generated_code) {
            if let Some(start_fn) = program.start_function {
                return Ok(start_fn.body);
            }
            return Ok(Vec::new());
        }

        // If full program parsing fails, try extracting statements from start() or start: block
        // Strip the start prefix and parse individual statements
        let code_without_start = if generated_code.trim().starts_with("start()")
            || generated_code.trim().starts_with("start:")
        {
            generated_code
                .lines()
                .skip(1) // Skip "start()" or "start:" line
                .filter(|line| !line.trim().is_empty()) // Skip empty lines
                .map(|line| {
                    // Remove one level of indentation (tab or spaces)
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

        // Wrap the statements in a minimal program structure to parse
        // Use start() syntax which is valid Clean Language
        let wrapper = format!(
            "start()\n\t{}",
            code_without_start.trim().replace('\n', "\n\t")
        );
        let program = crate::parser::CleanParser::parse_program(&wrapper).map_err(|e| {
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
        let linker = self.setup_linker()?;

        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|e| anyhow!("Failed to instantiate plugin module: {}", e))?;

        // Get memory
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("Plugin does not export memory"))?;

        // Strip trailing colon from block name
        let block_name = block.name.trim_end_matches(':');
        let block_name_ptr = self.find_or_write_string(&mut store, &memory, block_name)?;

        // Format attributes
        let attributes_str = block
            .attributes
            .iter()
            .map(|attr| {
                if let Some(ref val) = attr.value {
                    format!("{}={}", attr.name, val)
                } else {
                    attr.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let attributes_ptr = self.find_or_write_string(&mut store, &memory, &attributes_str)?;

        // Body
        let body_ptr = self.find_or_write_string(&mut store, &memory, &block.content)?;

        // Call expand function
        let expand: TypedFunc<(i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, "expand")
            .map_err(|e| anyhow!("Plugin does not export 'expand' function: {}", e))?;

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

        // Try parsing as a full program - this preserves the start function
        if let Ok(program) = crate::parser::CleanParser::parse_program(generated_code) {
            return Ok(PluginExpansion {
                statements: program.statements,
                start_function: program.start_function,
                functions: program.functions,
            });
        }

        // If full program parsing fails, try wrapping and parsing
        let code_without_start = if generated_code.trim().starts_with("start()")
            || generated_code.trim().starts_with("start:")
        {
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
            "start()\n\t{}",
            code_without_start.trim().replace('\n', "\n\t")
        );
        let program = crate::parser::CleanParser::parse_program(&wrapper)
            .map_err(|e| anyhow!("Failed to parse plugin output: {}", e))?;

        Ok(PluginExpansion {
            statements: Vec::new(),
            start_function: program.start_function,
            functions: program.functions,
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
        // Data section typically starts around 4096 and extends to ~8192
        let data = memory.data(&*store);
        let scan_start = 4096usize;
        let scan_end = std::cmp::min(8192usize, data.len().saturating_sub(4 + len));

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
    /// - Offset 8: type_id (u32) - must be 3 for STRING_TYPE_ID
    ///
    /// The string.length() function checks type_id at offset 8 to verify
    /// it's a string before reading the length at offset 0.
    fn write_clean_string(
        &self,
        store: &mut Store<PluginState>,
        memory: &Memory,
        s: &str,
    ) -> Result<i32> {
        let bytes = s.as_bytes();
        let len = bytes.len();

        // We need space for: length (4) + data (len) + padding to offset 8 + type_id (4)
        // Layout: [length:4][data:len][padding:?][type_id:4]
        // The type_id must be at offset 8 from the start
        // So if len < 4, we need padding; if len >= 4, type_id goes at offset 4 + len (aligned to 8)
        let type_id_offset = 8; // type_id is always at offset 8
        let total_size = std::cmp::max(4 + len, type_id_offset) + 4; // At least offset 8 + 4 bytes for type_id

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

        // Write type_id (STRING_TYPE_ID = 3) at offset 8
        let type_id_bytes = 3u32.to_le_bytes();
        memory.write(&mut *store, ptr + type_id_offset, &type_id_bytes)?;

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
            alloc_offset: 1024, // Start after initial memory region
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_state_allocation() {
        let mut state = PluginState::new();

        let ptr1 = state.allocate(100);
        assert_eq!(ptr1, 1024);

        let ptr2 = state.allocate(200);
        // 1024 + 100 = 1124, aligned to 8 = 1128
        assert_eq!(ptr2, 1128);

        let ptr3 = state.allocate(50);
        // 1128 + 200 = 1328, aligned to 8 = 1328
        assert_eq!(ptr3, 1328);
    }
}
