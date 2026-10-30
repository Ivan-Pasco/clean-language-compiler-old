//! Module for type management during code generation.

use crate::error::CompilerError;
use crate::types::WasmType;
use wasm_encoder::{TypeSection, ValType};
use wasmparser::FuncType;

/// Manages type information and conversions during code generation
#[derive(Clone)]
pub(crate) struct TypeManager {
    type_section: TypeSection,
    function_types: Vec<FuncType>,
}

impl TypeManager {
    /// Create a new type manager
    pub(crate) fn new() -> Self {
        Self {
            type_section: TypeSection::new(),
            function_types: Vec::new(),
        }
    }

    /// Get a cloned type section for module assembly
    pub(crate) fn clone_type_section(&self) -> TypeSection {
        self.type_section.clone()
    }

    /// Add a function type to the type section (supports multi-value returns)
    /// Returns the type index, reusing existing types if they match
    pub(crate) fn add_function_type(
        &mut self,
        params: &[WasmType],
        return_types: &[WasmType],
    ) -> Result<u32, CompilerError> {
        let param_val_types: Vec<ValType> = params.iter().map(|t| (*t).into()).collect();
        let return_val_types: Vec<ValType> = return_types.iter().map(|t| (*t).into()).collect();

        // Convert to parser types for comparison
        let parser_param_types: Vec<wasmparser::ValType> = param_val_types
            .iter()
            .map(|vt| WasmType::from(*vt).to_parser_val_type())
            .collect();
        let parser_result_types: Vec<wasmparser::ValType> = return_val_types
            .iter()
            .map(|vt| WasmType::from(*vt).to_parser_val_type())
            .collect();

        // Check if this function type already exists
        for (index, existing_type) in self.function_types.iter().enumerate() {
            if existing_type.params() == parser_param_types.as_slice()
                && existing_type.results() == parser_result_types.as_slice()
            {
                // Type already exists, return its index
                return Ok(index as u32);
            }
        }

        // Type doesn't exist, add it
        self.type_section
            .function(param_val_types.clone(), return_val_types.clone());
        let type_index = self.function_types.len() as u32;

        self.function_types
            .push(FuncType::new(parser_param_types, parser_result_types));

        Ok(type_index)
    }

    /// Add a function type with single or no return value (convenience method)
    pub(crate) fn add_function_type_single(
        &mut self,
        params: &[WasmType],
        return_type: Option<WasmType>,
    ) -> Result<u32, CompilerError> {
        let return_types: Vec<WasmType> = return_type.map(|t| vec![t]).unwrap_or_default();
        self.add_function_type(params, &return_types)
    }
}
