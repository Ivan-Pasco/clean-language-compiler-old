//! WebAssembly assembly phase of the compilation pipeline
//! 
//! This phase takes the generated intermediate representation and assembles
//! it into a final WebAssembly module using the wasm-encoder crate.

use crate::error::CompilerError;
use super::{
    CompilationPhase,
    generation::{GenerationContext, GeneratedFunction, FunctionType, ImportDeclaration, ExportDeclaration, MemoryLayout}
};
use wasm_encoder::{
    Module, TypeSection, ImportSection, FunctionSection, MemorySection, ExportSection,
    CodeSection, DataSection, Function, MemoryType, EntityType, ValType
};
use std::collections::HashMap;

/// Result of the assembly phase containing the final WASM module
#[derive(Debug)]
pub struct AssemblyResult {
    pub module: Module,
    pub metadata: AssemblyMetadata,
}

/// Metadata about the assembled module
#[derive(Debug)]
pub struct AssemblyMetadata {
    pub function_count: u32,
    pub import_count: u32,
    pub export_count: u32,
    pub memory_pages: u32,
    pub code_size: usize,
}

impl AssemblyResult {
    pub fn into_module(self) -> Module {
        self.module
    }
    
    pub fn finish(self) -> Vec<u8> {
        self.module.finish()
    }
}

/// WASM module builder implementing the fourth phase of compilation
pub struct WasmBuilder {
    type_section: TypeSection,
    import_section: ImportSection,
    function_section: FunctionSection,
    memory_section: MemorySection,
    export_section: ExportSection,
    code_section: CodeSection,
    data_section: DataSection,
    
    // State tracking
    type_indices: HashMap<String, u32>,
    import_count: u32,
    function_count: u32,
}

impl WasmBuilder {
    pub fn new() -> Self {
        Self {
            type_section: TypeSection::new(),
            import_section: ImportSection::new(),
            function_section: FunctionSection::new(),
            memory_section: MemorySection::new(),
            export_section: ExportSection::new(),
            code_section: CodeSection::new(),
            data_section: DataSection::new(),
            
            type_indices: HashMap::new(),
            import_count: 0,
            function_count: 0,
        }
    }
    
    fn build_type_section(&mut self, type_signatures: &[FunctionType]) -> Result<(), CompilerError> {
        for (i, signature) in type_signatures.iter().enumerate() {
            self.type_section.function(
                signature.parameters.clone(),
                signature.results.clone(),
            );
            
            // Track type index for function signature matching
            let signature_key = self.function_type_key(signature);
            self.type_indices.insert(signature_key, i as u32);
        }
        
        Ok(())
    }
    
    fn build_import_section(&mut self, imports: &[ImportDeclaration]) -> Result<(), CompilerError> {
        for import in imports {
            let type_index = self.get_or_create_type_index(&import.function_type)?;
            
            self.import_section.import(
                &import.module,
                &import.name,
                EntityType::Function(type_index),
            );
            
            self.import_count += 1;
        }
        
        Ok(())
    }
    
    fn build_function_section(&mut self, functions: &[GeneratedFunction]) -> Result<(), CompilerError> {
        for function in functions {
            self.function_section.function(function.type_index);
            self.function_count += 1;
        }
        
        Ok(())
    }
    
    fn build_memory_section(&mut self, memory_layout: &MemoryLayout) -> Result<(), CompilerError> {
        self.memory_section.memory(MemoryType {
            minimum: memory_layout.initial_pages,
            maximum: memory_layout.max_pages,
            memory64: false,
            shared: false,
        });
        
        Ok(())
    }
    
    fn build_export_section(&mut self, exports: &[ExportDeclaration]) -> Result<(), CompilerError> {
        for (i, export) in exports.iter().enumerate() {
            // Function index is import_count + function_index
            let function_index = self.import_count + i as u32;
            self.export_section.export(&export.name, EntityType::Function(function_index));
        }
        
        // Always export memory
        self.export_section.export("memory", EntityType::Memory(0));
        
        Ok(())
    }
    
    fn build_code_section(&mut self, functions: &[GeneratedFunction]) -> Result<(), CompilerError> {
        for function in functions {
            let mut wasm_function = Function::new(
                function.locals.iter().map(|local| (1, local.wasm_type)).collect()
            );
            
            // Add all instructions
            for instruction in &function.instructions {
                wasm_function.instruction(instruction);
            }
            
            // Ensure proper function termination
            if function.instructions.is_empty() || !self.ends_with_terminator(&function.instructions) {
                wasm_function.instruction(&wasm_encoder::Instruction::End);
            }
            
            self.code_section.function(&wasm_function);
        }
        
        Ok(())
    }
    
    fn build_data_section(&mut self, memory_layout: &MemoryLayout, string_constants: &[String]) -> Result<(), CompilerError> {
        if !string_constants.is_empty() {
            // Create data segment for string constants
            let mut data = Vec::new();
            let mut current_offset = 1024u32; // Start after reserved memory
            
            for string in string_constants {
                if let Some(&offset) = memory_layout.string_pool.get(string) {
                    // Add string length (4 bytes)
                    let length = string.len() as u32;
                    data.extend_from_slice(&length.to_le_bytes());
                    
                    // Add string data
                    data.extend_from_slice(string.as_bytes());
                    
                    // Align to 4-byte boundary
                    while data.len() % 4 != 0 {
                        data.push(0);
                    }
                }
            }
            
            if !data.is_empty() {
                self.data_section.active(
                    0, // Memory index
                    &wasm_encoder::Instruction::I32Const(1024), // Offset
                    data,
                );
            }
        }
        
        Ok(())
    }
    
    fn get_or_create_type_index(&mut self, function_type: &FunctionType) -> Result<u32, CompilerError> {
        let key = self.function_type_key(function_type);
        
        if let Some(&index) = self.type_indices.get(&key) {
            Ok(index)
        } else {
            let index = self.type_indices.len() as u32;
            self.type_section.function(
                function_type.parameters.clone(),
                function_type.results.clone(),
            );
            self.type_indices.insert(key, index);
            Ok(index)
        }
    }
    
    fn function_type_key(&self, function_type: &FunctionType) -> String {
        format!(
            "({}) -> ({})",
            function_type.parameters.iter()
                .map(|t| format!("{:?}", t))
                .collect::<Vec<_>>()
                .join(", "),
            function_type.results.iter()
                .map(|t| format!("{:?}", t))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
    
    fn ends_with_terminator(&self, instructions: &[wasm_encoder::Instruction]) -> bool {
        if let Some(last_instruction) = instructions.last() {
            matches!(last_instruction,
                wasm_encoder::Instruction::Return |
                wasm_encoder::Instruction::Unreachable |
                wasm_encoder::Instruction::End
            )
        } else {
            false
        }
    }
    
    fn build_module(&mut self, context: GenerationContext) -> Result<AssemblyResult, CompilerError> {
        // Build all sections
        self.build_type_section(&context.type_signatures)?;
        self.build_import_section(&context.imports)?;
        self.build_function_section(&context.functions)?;
        self.build_memory_section(&context.memory_layout)?;
        self.build_export_section(&context.exports)?;
        self.build_code_section(&context.functions)?;
        self.build_data_section(&context.memory_layout, &context.string_constants)?;
        
        // Assemble final module
        let mut module = Module::new();
        
        // Add sections in the correct order
        if !self.type_section.is_empty() {
            module.section(&self.type_section);
        }
        
        if !self.import_section.is_empty() {
            module.section(&self.import_section);
        }
        
        if !self.function_section.is_empty() {
            module.section(&self.function_section);
        }
        
        if !self.memory_section.is_empty() {
            module.section(&self.memory_section);
        }
        
        if !self.export_section.is_empty() {
            module.section(&self.export_section);
        }
        
        if !self.code_section.is_empty() {
            module.section(&self.code_section);
        }
        
        if !self.data_section.is_empty() {
            module.section(&self.data_section);
        }
        
        // Calculate code size
        let code_size = module.finish().len();
        
        // Reconstruct module (since finish() consumes it)
        let mut final_module = Module::new();
        
        if !self.type_section.is_empty() {
            final_module.section(&self.type_section);
        }
        
        if !self.import_section.is_empty() {
            final_module.section(&self.import_section);
        }
        
        if !self.function_section.is_empty() {
            final_module.section(&self.function_section);
        }
        
        if !self.memory_section.is_empty() {
            final_module.section(&self.memory_section);
        }
        
        if !self.export_section.is_empty() {
            final_module.section(&self.export_section);
        }
        
        if !self.code_section.is_empty() {
            final_module.section(&self.code_section);
        }
        
        if !self.data_section.is_empty() {
            final_module.section(&self.data_section);
        }
        
        let metadata = AssemblyMetadata {
            function_count: self.function_count,
            import_count: self.import_count,
            export_count: context.exports.len() as u32,
            memory_pages: context.memory_layout.initial_pages,
            code_size,
        };
        
        Ok(AssemblyResult {
            module: final_module,
            metadata,
        })
    }
}

impl CompilationPhase<GenerationContext, AssemblyResult> for WasmBuilder {
    type Error = CompilerError;
    
    fn execute(&mut self, context: GenerationContext) -> Result<AssemblyResult, Self::Error> {
        self.build_module(context)
    }
}

// Extension trait to check if sections are empty
trait SectionEmpty {
    fn is_empty(&self) -> bool;
}

impl SectionEmpty for TypeSection {
    fn is_empty(&self) -> bool {
        // TypeSection doesn't expose its internal state, so we assume it's not empty
        // if we've added any types. This is a simplification.
        false
    }
}

impl SectionEmpty for ImportSection {
    fn is_empty(&self) -> bool {
        // Same as TypeSection - assume not empty if we've added imports
        false
    }
}

impl SectionEmpty for FunctionSection {
    fn is_empty(&self) -> bool {
        false // Assume not empty if we've added functions
    }
}

impl SectionEmpty for MemorySection {
    fn is_empty(&self) -> bool {
        false // Assume we always have memory
    }
}

impl SectionEmpty for ExportSection {
    fn is_empty(&self) -> bool {
        false // Assume we always have exports
    }
}

impl SectionEmpty for CodeSection {
    fn is_empty(&self) -> bool {
        false // Assume not empty if we have functions
    }
}

impl SectionEmpty for DataSection {
    fn is_empty(&self) -> bool {
        false // For now, assume we might have data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_encoder::{Instruction, ValType};

    #[test]
    fn test_wasm_builder_creation() {
        let builder = WasmBuilder::new();
        assert_eq!(builder.import_count, 0);
        assert_eq!(builder.function_count, 0);
    }

    #[test]
    fn test_function_type_key() {
        let builder = WasmBuilder::new();
        let function_type = FunctionType {
            parameters: vec![ValType::I32, ValType::F64],
            results: vec![ValType::I32],
        };
        
        let key = builder.function_type_key(&function_type);
        assert!(key.contains("I32"));
        assert!(key.contains("F64"));
    }

    #[test]
    fn test_ends_with_terminator() {
        let builder = WasmBuilder::new();
        
        let instructions_with_return = vec![
            Instruction::I32Const(42),
            Instruction::Return,
        ];
        assert!(builder.ends_with_terminator(&instructions_with_return));
        
        let instructions_without_terminator = vec![
            Instruction::I32Const(42),
        ];
        assert!(!builder.ends_with_terminator(&instructions_without_terminator));
    }

    #[test]
    fn test_assembly_metadata() {
        let metadata = AssemblyMetadata {
            function_count: 5,
            import_count: 2,
            export_count: 3,
            memory_pages: 1,
            code_size: 1024,
        };
        
        assert_eq!(metadata.function_count, 5);
        assert_eq!(metadata.code_size, 1024);
    }
}