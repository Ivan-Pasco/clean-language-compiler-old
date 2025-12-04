//! WASM Plugin Adapter
//!
//! Wraps a WASM module to implement the FrameworkPlugin trait

use anyhow::{anyhow, Result};
use wasmtime::{Caller, Engine, Linker, Memory, Module, Store, TypedFunc};

use crate::ast::{FrameworkBlock, Statement};
use crate::plugins::{
    FrameworkPlugin, PluginCompletionItem, PluginDiagnostic, PluginError, PluginHoverInfo,
    PluginLspContext, PluginResult,
};

use super::plugin_abi::{PluginFrameworkBlock, PluginManifest};

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
    fn setup_linker(&self) -> Result<Linker<PluginState>> {
        let mut linker = Linker::new(&self.engine);

        // Host function: debug_print
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

        // Host function: allocate memory (for returning data)
        linker.func_wrap(
            "env",
            "alloc",
            |mut caller: Caller<'_, PluginState>, size: i32| -> i32 {
                let state = caller.data_mut();
                state.allocate(size as usize) as i32
            },
        )?;

        // Host function: report error
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
    fn call_expand(&self, block: &FrameworkBlock) -> Result<Vec<Statement>> {
        let mut store = self.create_store();
        let linker = self.setup_linker()?;

        let instance = linker.instantiate(&mut store, &self.module)?;

        // Convert to plugin ABI format and serialize to JSON
        let plugin_block = PluginFrameworkBlock::from(block);
        let block_json = serde_json::to_string(&plugin_block)
            .map_err(|e| anyhow!("Failed to serialize FrameworkBlock: {}", e))?;

        // Get memory
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("Plugin does not export memory"))?;

        // Write the JSON to WASM memory
        let json_bytes = block_json.as_bytes();
        let json_ptr = self.write_to_memory(&mut store, &memory, json_bytes)?;

        // Call the expand function
        let expand: TypedFunc<(i32, i32), i32> = instance
            .get_typed_func(&mut store, "expand")
            .map_err(|e| anyhow!("Plugin does not export 'expand' function: {}", e))?;

        let result_ptr = expand.call(&mut store, (json_ptr, json_bytes.len() as i32))?;

        // Check for errors
        if let Some(error) = store.data().last_error.clone() {
            return Err(anyhow!("Plugin error: {}", error));
        }

        // Read the result
        let result_bytes = self.read_result(&store, &memory, result_ptr)?;

        // The plugin returns Clean Language source code, which we parse
        let generated_code = std::str::from_utf8(&result_bytes)
            .map_err(|e| anyhow!("Invalid UTF-8 in plugin response: {}", e))?;

        // Parse the generated code into a program
        // Plugins output code in a "start:" block format
        let program = crate::parser::CleanParser::parse_program(generated_code)
            .map_err(|e| anyhow!("Failed to parse plugin output: {}", e))?;

        // Extract statements from the start function (if present)
        let statements = program.start_function.map(|f| f.body).unwrap_or_default();

        Ok(statements)
    }

    /// Write bytes to WASM memory
    fn write_to_memory(
        &self,
        store: &mut Store<PluginState>,
        memory: &Memory,
        bytes: &[u8],
    ) -> Result<i32> {
        // Find a free region in memory (simple bump allocator)
        let ptr = store.data_mut().allocate(bytes.len());

        // Ensure memory is large enough
        let required_pages = ((ptr + bytes.len()) / 65536) + 1;
        let current_pages = memory.size(&mut *store) as usize;
        if required_pages > current_pages {
            memory
                .grow(&mut *store, (required_pages - current_pages) as u64)
                .map_err(|e| anyhow!("Failed to grow memory: {}", e))?;
        }

        // Write the bytes
        memory.write(&mut *store, ptr, bytes)?;

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
