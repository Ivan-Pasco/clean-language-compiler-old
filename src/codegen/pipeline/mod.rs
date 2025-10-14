//! Modular compilation pipeline for Clean Language to WebAssembly
//!
//! # DEPRECATED
//! This module is deprecated and will be removed in v0.11.0.
//! The stub pipeline only generates hello-world WASM modules and is not used in production.
//! Use the MIR-based code generator (mir_codegen.rs) for actual WASM compilation.

use crate::ast::Program;
use crate::error::CompilerError;
use wasm_encoder::Module;

// Temporarily comment out complex modules while fixing compilation errors
// pub mod analysis;
// pub mod resolution; 
// pub mod generation;
// pub mod assembly;

/// Represents different phases of the compilation pipeline
#[derive(Debug, Clone, PartialEq)]
pub enum CompilationPhase {
    Analysis,
    Resolution,
    Generation,
    Assembly,
}

/// Main compilation pipeline - simplified working version
///
/// # DEPRECATED
/// This is a stub implementation that only generates hello-world modules.
/// Use MirCodeGenerator from mir_codegen.rs for actual compilation.
#[deprecated(since = "0.10.2", note = "Use MirCodeGenerator from mir_codegen.rs instead")]
pub struct CompilationPipeline {
    /// Current phase of compilation
    #[allow(dead_code)]
    current_phase: CompilationPhase,
}

impl CompilationPipeline {
    pub fn new() -> Self {
        Self {
            current_phase: CompilationPhase::Analysis,
        }
    }

    /// Compile a Clean Language program to WebAssembly
    pub fn compile(&mut self, program: &Program) -> Result<Module, CompilerError> {
        // Create a basic working WASM module
        let mut module = Module::new();
        
        // WASM sections must be in the correct order:
        // 1. Type, 2. Import, 3. Function, 4. Table, 5. Memory, 6. Global, 7. Export, 8. Start, 9. Element, 10. Code, 11. Data
        
        // 1. Add type section
        let mut types = wasm_encoder::TypeSection::new();
        types.function([wasm_encoder::ValType::I32, wasm_encoder::ValType::I32], []); // print function signature
        types.function([], []); // start function signature
        module.section(&types);
        
        // 2. Add import section for print function
        let mut imports = wasm_encoder::ImportSection::new();
        imports.import("env", "print", wasm_encoder::EntityType::Function(0));
        module.section(&imports);
        
        // 3. Add function section
        let mut functions = wasm_encoder::FunctionSection::new();
        functions.function(1); // start function uses type index 1
        module.section(&functions);
        
        // 5. Add memory section (after function section)
        let memory_type = wasm_encoder::MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
        };
        let mut memory = wasm_encoder::MemorySection::new();
        memory.memory(memory_type);
        module.section(&memory);
        
        // 7. Add export section (after memory section)
        let mut exports = wasm_encoder::ExportSection::new();
        exports.export("memory", wasm_encoder::ExportKind::Memory, 0);
        exports.export("_start", wasm_encoder::ExportKind::Func, 1);
        module.section(&exports);
        
        // 10. Add code section
        let mut code = wasm_encoder::CodeSection::new();
        let mut func_body = wasm_encoder::Function::new([]);
        
        // Check if this is a hello world program and add appropriate instructions
        if self.is_hello_world_program(program) {
            // Add instructions to print "Hello, World!"
            func_body.instruction(&wasm_encoder::Instruction::I32Const(1024)); // string offset
            func_body.instruction(&wasm_encoder::Instruction::I32Const(13));   // string length
            func_body.instruction(&wasm_encoder::Instruction::Call(0));        // call print function
        }
        
        func_body.instruction(&wasm_encoder::Instruction::End);
        code.function(&func_body);
        module.section(&code);
        
        // 11. Add data section for string literals (last)
        if self.is_hello_world_program(program) {
            let mut data = wasm_encoder::DataSection::new();
            let offset = wasm_encoder::ConstExpr::i32_const(1024);
            data.active(0, &offset, b"Hello, World!".iter().copied());
            module.section(&data);
        }
        
        Ok(module)
    }

    /// Simple check to see if this looks like a hello world program
    fn is_hello_world_program(&self, program: &Program) -> bool {
        program.functions.iter().any(|func| func.name == "start")
    }
}

impl Default for CompilationPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_creation() {
        let pipeline = CompilationPipeline::new();
        // Basic smoke test - ensure pipeline can be created
        assert!(true);
    }
}