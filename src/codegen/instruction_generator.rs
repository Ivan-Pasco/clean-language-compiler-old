//! WebAssembly instruction and function-type tracking for CodeGenerator.

use crate::types::WasmType;
use wasm_encoder::ValType;

use super::type_manager::TypeManager;

/// Tracks function signatures and indices for WASM code generation.
pub(crate) struct InstructionGenerator {
    #[allow(dead_code)] // Stored at construction; type queries not yet delegated here
    type_manager: TypeManager,
    function_map: std::collections::HashMap<String, u32>,
    function_signatures: std::collections::HashMap<String, u32>,
}

impl InstructionGenerator {
    pub(crate) fn new(type_manager: TypeManager) -> Self {
        Self {
            type_manager,
            function_map: std::collections::HashMap::new(),
            function_signatures: std::collections::HashMap::new(),
        }
    }

    fn create_function_signature(&self, name: &str, param_types: &[WasmType]) -> String {
        let param_str = param_types
            .iter()
            .map(|t| format!("{t:?}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{name}({param_str})")
    }

    /// No-op: type info was previously stored here but is never retrieved.
    /// Kept for API compatibility with callers in codegen/mod.rs.
    pub(crate) fn add_function_type(
        &mut self,
        _index: u32,
        _params: Vec<ValType>,
        _results: Vec<ValType>,
    ) {
    }

    /// Register a function, deduplicating by exact signature.
    pub(crate) fn register_function(
        &mut self,
        name: &str,
        params: &[WasmType],
        _return_type: Option<WasmType>,
        _instructions: &[wasm_encoder::Instruction],
        function_index: u32,
    ) -> Result<u32, crate::error::CompilerError> {
        let signature = self.create_function_signature(name, params);

        if let Some(&existing_index) = self.function_signatures.get(&signature) {
            return Ok(existing_index);
        }

        self.function_signatures.insert(signature, function_index);

        if !self.function_map.contains_key(name) {
            self.function_map.insert(name.to_string(), function_index);
        }

        Ok(function_index)
    }
}
