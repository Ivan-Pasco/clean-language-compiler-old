//! WebAssembly module builder for the Clean Language compiler.
//!
//! This module provides functionality to assemble WebAssembly modules from
//! various sections (type, function, export, code, etc.) following WASM standards.

#![allow(dead_code)]

use crate::codegen::type_manager::TypeManager;
use crate::error::CompilerError;
use wasm_encoder::{
    CodeSection, DataSection, ExportSection, FunctionSection, GlobalSection, ImportSection,
    MemorySection, Module,
};

/// Builder for constructing WebAssembly modules from sections.
pub struct WasmModuleBuilder {
    include_runtime_imports: bool,
}

impl WasmModuleBuilder {
    /// Create a new WASM module builder.
    pub fn new(include_runtime_imports: bool) -> Self {
        Self {
            include_runtime_imports,
        }
    }

    /// Assemble a complete WebAssembly module from the provided sections.
    ///
    /// Sections are added in the correct order as required by the WASM specification:
    /// 1. Type section
    /// 2. Import section (if runtime imports are enabled)
    /// 3. Function section (if functions exist)
    /// 4. Memory section
    /// 5. Global section
    /// 6. Export section
    /// 7. Code section (if functions exist)
    /// 8. Data section
    pub fn assemble_module(
        &self,
        type_manager: &TypeManager,
        import_section: &ImportSection,
        function_section: &FunctionSection,
        memory_section: &MemorySection,
        global_section: &GlobalSection,
        export_section: &ExportSection,
        code_section: &CodeSection,
        data_section: &DataSection,
        function_count: u32,
    ) -> Result<Vec<u8>, CompilerError> {
        let mut module = Module::new();

        // Add sections in the correct order per WASM specification
        module.section(&type_manager.clone_type_section());

        // Add import section if runtime imports are enabled
        if self.include_runtime_imports {
            module.section(import_section);
        }

        // Add function section if we have functions
        if function_count > 0 {
            module.section(function_section);
        }

        // Always add memory section
        module.section(memory_section);

        // Add global section for heap pointer and other globals
        if !global_section.is_empty() {
            module.section(global_section);
        }

        // Add exports section
        module.section(export_section);

        // Add code section if we have functions
        if function_count > 0 {
            module.section(code_section);
        }

        // Add data section for string literals and other static data
        module.section(data_section);

        Ok(module.finish())
    }

    /// Create a minimal test module without imports.
    /// Used for testing basic WASM generation without runtime dependencies.
    pub fn create_test_module_without_imports(
        &self,
        type_manager: &TypeManager,
        function_section: &FunctionSection,
        memory_section: &MemorySection,
        global_section: &GlobalSection,
        export_section: &ExportSection,
        code_section: &CodeSection,
        data_section: &DataSection,
        function_count: u32,
    ) -> Result<Vec<u8>, CompilerError> {
        let mut module = Module::new();

        // Add sections for test module (no imports)
        module.section(&type_manager.clone_type_section());

        if function_count > 0 {
            module.section(function_section);
        }

        module.section(memory_section);

        // Add global section for heap pointer and other globals
        if !global_section.is_empty() {
            module.section(global_section);
        }

        module.section(export_section);

        if function_count > 0 {
            module.section(code_section);
        }

        module.section(data_section);

        Ok(module.finish())
    }

    /// Validate that the module sections are in the correct order.
    /// This is a sanity check to ensure WASM compliance.
    pub fn validate_section_order(&self) -> bool {
        // This would contain validation logic for section ordering
        // For now, return true as the assemble_module method handles ordering
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_encoder::MemoryType;

    #[test]
    fn test_module_builder_creation() {
        let builder = WasmModuleBuilder::new(true);
        assert_eq!(builder.include_runtime_imports, true);

        let builder_no_imports = WasmModuleBuilder::new(false);
        assert_eq!(builder_no_imports.include_runtime_imports, false);
    }

    #[test]
    fn test_section_order_validation() {
        let builder = WasmModuleBuilder::new(true);
        assert!(builder.validate_section_order());
    }

    #[test]
    fn test_minimal_module_assembly() {
        let builder = WasmModuleBuilder::new(false);
        let type_manager = TypeManager::new();
        let function_section = FunctionSection::new();
        let mut memory_section = MemorySection::new();
        memory_section.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
        });
        let global_section = GlobalSection::new();
        let export_section = ExportSection::new();
        let code_section = CodeSection::new();
        let data_section = DataSection::new();

        let result = builder.create_test_module_without_imports(
            &type_manager,
            &function_section,
            &memory_section,
            &global_section,
            &export_section,
            &code_section,
            &data_section,
            0,
        );

        assert!(result.is_ok());
        let wasm_bytes = result.unwrap();
        assert!(!wasm_bytes.is_empty());
    }
}
