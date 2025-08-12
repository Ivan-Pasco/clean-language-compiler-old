//! WebAssembly Module Generator for LIR
//!
//! Generates WebAssembly modules from Low-level Intermediate Representation (LIR)

use crate::error::CompilerError;
use crate::ir::*;
use std::collections::HashMap;
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    GlobalType, ImportSection, Instruction, MemArg, MemorySection, MemoryType, Module, TypeSection,
    ValType,
};

/// WebAssembly module generator from LIR
pub struct WasmGenerator {
    /// Function type index mapping
    function_types: HashMap<Vec<ValType>, u32>,
    /// Runtime function type mapping with both params and returns
    runtime_function_type_map: HashMap<(Vec<ValType>, Vec<ValType>), u32>,
    /// Function index mapping
    function_indices: HashMap<String, u32>,
    /// Import function count
    import_count: u32,
    /// Next function type index
    next_type_idx: u32,
}

impl WasmGenerator {
    pub fn new() -> Self {
        Self {
            function_types: HashMap::new(),
            runtime_function_type_map: HashMap::new(),
            function_indices: HashMap::new(),
            import_count: 0,
            next_type_idx: 0,
        }
    }

    /// Generate WebAssembly module from LIR program
    pub fn generate_wasm_module(&mut self, program: LIRProgram) -> Result<Vec<u8>, CompilerError> {
        let mut module = Module::new();

        // 1. Generate type section
        let type_section = self.generate_type_section(&program)?;
        module.section(&type_section);

        // 2. Generate import section
        let import_section = self.generate_import_section(&program)?;
        module.section(&import_section);

        // 3. Generate function section
        let function_section = self.generate_function_section(&program)?;
        module.section(&function_section);

        // 4. Generate memory section
        let memory_section = self.generate_memory_section(&program)?;
        module.section(&memory_section);

        // 5. Generate export section
        let export_section = self.generate_export_section(&program)?;
        module.section(&export_section);

        // 6. Generate code section
        let code_section = self.generate_code_section(&program)?;
        module.section(&code_section);

        Ok(module.finish())
    }

    fn generate_type_section(
        &mut self,
        program: &LIRProgram,
    ) -> Result<TypeSection, CompilerError> {
        let mut type_section = TypeSection::new();

        // Pre-populate runtime function types
        self.add_runtime_function_types();

        // Collect all function types without adding them to the section yet
        for function in &program.functions {
            let param_types: Vec<ValType> = function
                .parameters
                .iter()
                .map(|t| self.lir_type_to_wasm_val_type(t))
                .collect();

            let return_types: Vec<ValType> = match &function.return_type {
                Some(ret_type) => vec![self.lir_type_to_wasm_val_type(ret_type)],
                None => vec![],
            };

            // Check if this function type already exists (using params and return as key)
            let type_key = (param_types.clone(), return_types.clone());
            if !self.runtime_function_type_map.contains_key(&type_key) {
                self.runtime_function_type_map.insert(type_key.clone(), self.next_type_idx);
                // Also add to function_types for backward compatibility
                self.function_types.insert(param_types, self.next_type_idx);
                self.next_type_idx += 1;
            }
        }

        // Add import function types
        for import in &program.imports {
            if let LIRImportType::Function(param_types, return_type) = &import.import_type {
                let wasm_param_types: Vec<ValType> = param_types
                    .iter()
                    .map(|t| self.lir_type_to_wasm_val_type(t))
                    .collect();

                let wasm_return_types: Vec<ValType> = match return_type {
                    Some(ret_type) => vec![self.lir_type_to_wasm_val_type(ret_type)],
                    None => vec![],
                };

                // Check if this function type already exists (using params and return as key)
                let type_key = (wasm_param_types.clone(), wasm_return_types.clone());
                if !self.runtime_function_type_map.contains_key(&type_key) {
                    self.runtime_function_type_map.insert(type_key.clone(), self.next_type_idx);
                    // Also add to function_types for backward compatibility
                    self.function_types.insert(wasm_param_types, self.next_type_idx);
                    self.next_type_idx += 1;
                }
            }
        }

        // Now emit all types to the section in order
        self.emit_function_types_to_section(&mut type_section);

        Ok(type_section)
    }

    /// Pre-populate function types for runtime functions
    fn add_runtime_function_types(&mut self) {
        // Do nothing here - types will be added as needed
    }

    /// Emit all collected function types to the type section  
    fn emit_function_types_to_section(&self, type_section: &mut TypeSection) {
        // Collect all unique type indices and their signatures
        let mut all_types: Vec<(u32, Vec<ValType>, Vec<ValType>)> = Vec::new();

        // Use only runtime_function_type_map as the source of truth
        for ((param_types, return_types), &type_idx) in &self.runtime_function_type_map {
            all_types.push((type_idx, param_types.clone(), return_types.clone()));
        }

        // Sort by type index and emit
        all_types.sort_by_key(|(type_idx, _, _)| *type_idx);
        for (_, param_types, return_types) in all_types {
            type_section.function(param_types, return_types);
        }
    }

    /// Get return types for a function type pattern
    fn get_return_types_for_function_type(&self, param_types: Vec<ValType>) -> Vec<ValType> {
        // Map specific parameter patterns to return types
        match param_types.as_slice() {
            [ValType::I32, ValType::I32] => vec![ValType::I32], // mem_alloc
            [ValType::I32] if param_types.len() == 1 => {
                // Could be mem_retain/release (no return) or mem_get_ref_count (returns i32)
                // Default to no return for single i32 param
                vec![]
            }
            [] => vec![ValType::I32], // mem_collect
            _ => vec![],              // Default: no return
        }
    }

    fn generate_import_section(
        &mut self,
        program: &LIRProgram,
    ) -> Result<ImportSection, CompilerError> {
        let mut import_section = ImportSection::new();

        // Add memory management runtime function imports
        self.add_memory_runtime_imports(&mut import_section)?;

        for import in &program.imports {
            match &import.import_type {
                LIRImportType::Function(param_types, return_type) => {
                    let wasm_param_types: Vec<ValType> = param_types
                        .iter()
                        .map(|t| self.lir_type_to_wasm_val_type(t))
                        .collect();

                    let wasm_return_types: Vec<ValType> = match return_type {
                        Some(ret_type) => vec![self.lir_type_to_wasm_val_type(ret_type)],
                        None => vec![],
                    };

                    // Use the combined key to find the correct type index
                    let type_key = (wasm_param_types, wasm_return_types);
                    let type_idx = *self.runtime_function_type_map.get(&type_key).ok_or_else(|| {
                        CompilerError::codegen_error(
                            format!("Function type not found for import: {} with signature {:?}", import.name, type_key),
                            None,
                            None,
                        )
                    })?;

                    import_section.import(
                        &import.module,
                        &import.name,
                        EntityType::Function(type_idx),
                    );

                    self.function_indices
                        .insert(import.name.clone(), self.import_count);
                    self.import_count += 1;
                }
                LIRImportType::Memory(initial, max) => {
                    let memory_type = MemoryType {
                        minimum: *initial as u64,
                        maximum: max.map(|m| m as u64),
                        memory64: false,
                        shared: false,
                    };
                    import_section.import(
                        &import.module,
                        &import.name,
                        EntityType::Memory(memory_type),
                    );
                }
                LIRImportType::Global(global_type, mutable) => {
                    let val_type = self.lir_type_to_wasm_val_type(global_type);
                    let global_type = GlobalType {
                        val_type,
                        mutable: *mutable,
                    };
                    import_section.import(
                        &import.module,
                        &import.name,
                        EntityType::Global(global_type),
                    );
                }
            }
        }

        Ok(import_section)
    }

    fn generate_function_section(
        &mut self,
        program: &LIRProgram,
    ) -> Result<FunctionSection, CompilerError> {
        let mut function_section = FunctionSection::new();

        for function in &program.functions {
            let param_types: Vec<ValType> = function
                .parameters
                .iter()
                .map(|t| self.lir_type_to_wasm_val_type(t))
                .collect();

            let return_types: Vec<ValType> = match &function.return_type {
                Some(ret_type) => vec![self.lir_type_to_wasm_val_type(ret_type)],
                None => vec![],
            };

            // Use the combined key to find the correct type index
            let type_key = (param_types.clone(), return_types);
            let type_idx = *self.runtime_function_type_map.get(&type_key).ok_or_else(|| {
                CompilerError::codegen_error(
                    format!("Function type not found for function: {} with signature {:?}", function.name, type_key),
                    None,
                    None,
                )
            })?;

            function_section.function(type_idx);
            self.function_indices
                .insert(function.name.clone(), self.import_count);
            self.import_count += 1;
        }

        Ok(function_section)
    }

    fn generate_memory_section(
        &self,
        program: &LIRProgram,
    ) -> Result<MemorySection, CompilerError> {
        let mut memory_section = MemorySection::new();

        let memory_type = MemoryType {
            minimum: program.memory_layout.initial_pages as u64,
            maximum: program.memory_layout.max_pages.map(|p| p as u64),
            memory64: false,
            shared: false,
        };
        memory_section.memory(memory_type);

        Ok(memory_section)
    }

    fn generate_export_section(
        &self,
        program: &LIRProgram,
    ) -> Result<ExportSection, CompilerError> {
        let mut export_section = ExportSection::new();

        for export in &program.exports {
            match &export.export_type {
                LIRExportType::Function(idx) => {
                    export_section.export(&export.name, ExportKind::Func, *idx as u32);
                }
                LIRExportType::Memory(idx) => {
                    export_section.export(&export.name, ExportKind::Memory, *idx as u32);
                }
                LIRExportType::Global(idx) => {
                    export_section.export(&export.name, ExportKind::Global, *idx as u32);
                }
            }
        }

        Ok(export_section)
    }

    fn generate_code_section(&self, program: &LIRProgram) -> Result<CodeSection, CompilerError> {
        let mut code_section = CodeSection::new();

        for function in &program.functions {
            let locals: Vec<(u32, ValType)> = function
                .locals
                .iter()
                .map(|t| (1, self.lir_type_to_wasm_val_type(t)))
                .collect();

            let mut func = Function::new(locals);

            // Convert LIR instructions to WebAssembly instructions
            for instruction in &function.instructions {
                self.emit_wasm_instruction(&mut func, instruction)?;
            }

            code_section.function(&func);
        }

        Ok(code_section)
    }

    fn emit_wasm_instruction(
        &self,
        func: &mut Function,
        instruction: &LIRInstruction,
    ) -> Result<(), CompilerError> {
        match instruction {
            // Control flow
            LIRInstruction::Block(block_type) => {
                let wasm_block_type = match block_type {
                    LIRType::I32 => BlockType::Result(ValType::I32),
                    LIRType::I64 => BlockType::Result(ValType::I64),
                    LIRType::F32 => BlockType::Result(ValType::F32),
                    LIRType::F64 => BlockType::Result(ValType::F64),
                };
                func.instruction(&Instruction::Block(wasm_block_type));
            }
            LIRInstruction::Loop(block_type) => {
                let wasm_block_type = match block_type {
                    LIRType::I32 => BlockType::Result(ValType::I32),
                    LIRType::I64 => BlockType::Result(ValType::I64),
                    LIRType::F32 => BlockType::Result(ValType::F32),
                    LIRType::F64 => BlockType::Result(ValType::F64),
                };
                func.instruction(&Instruction::Loop(wasm_block_type));
            }
            LIRInstruction::If(block_type) => {
                let wasm_block_type = match block_type {
                    LIRType::I32 => BlockType::Result(ValType::I32),
                    LIRType::I64 => BlockType::Result(ValType::I64),
                    LIRType::F32 => BlockType::Result(ValType::F32),
                    LIRType::F64 => BlockType::Result(ValType::F64),
                };
                func.instruction(&Instruction::If(wasm_block_type));
            }
            LIRInstruction::Else => {
                func.instruction(&Instruction::Else);
            }
            LIRInstruction::End => {
                func.instruction(&Instruction::End);
            }
            LIRInstruction::Br(label) => {
                func.instruction(&Instruction::Br(*label));
            }
            LIRInstruction::BrIf(label) => {
                func.instruction(&Instruction::BrIf(*label));
            }
            LIRInstruction::Return => {
                func.instruction(&Instruction::Return);
            }

            // Constants
            LIRInstruction::I32Const(value) => {
                func.instruction(&Instruction::I32Const(*value));
            }
            LIRInstruction::I64Const(value) => {
                func.instruction(&Instruction::I64Const(*value));
            }
            LIRInstruction::F32Const(value) => {
                func.instruction(&Instruction::F32Const(*value));
            }
            LIRInstruction::F64Const(value) => {
                func.instruction(&Instruction::F64Const(*value));
            }

            // Arithmetic (i32)
            LIRInstruction::I32Add => {
                func.instruction(&Instruction::I32Add);
            }
            LIRInstruction::I32Sub => {
                func.instruction(&Instruction::I32Sub);
            }
            LIRInstruction::I32Mul => {
                func.instruction(&Instruction::I32Mul);
            }
            LIRInstruction::I32DivS => {
                func.instruction(&Instruction::I32DivS);
            }
            LIRInstruction::I32DivU => {
                func.instruction(&Instruction::I32DivU);
            }
            LIRInstruction::I32RemS => {
                func.instruction(&Instruction::I32RemS);
            }
            LIRInstruction::I32RemU => {
                func.instruction(&Instruction::I32RemU);
            }

            // Arithmetic (i64)
            LIRInstruction::I64Add => {
                func.instruction(&Instruction::I64Add);
            }
            LIRInstruction::I64Sub => {
                func.instruction(&Instruction::I64Sub);
            }
            LIRInstruction::I64Mul => {
                func.instruction(&Instruction::I64Mul);
            }
            LIRInstruction::I64DivS => {
                func.instruction(&Instruction::I64DivS);
            }
            LIRInstruction::I64DivU => {
                func.instruction(&Instruction::I64DivU);
            }
            LIRInstruction::I64RemS => {
                func.instruction(&Instruction::I64RemS);
            }
            LIRInstruction::I64RemU => {
                func.instruction(&Instruction::I64RemU);
            }

            // Arithmetic (f32)
            LIRInstruction::F32Add => {
                func.instruction(&Instruction::F32Add);
            }
            LIRInstruction::F32Sub => {
                func.instruction(&Instruction::F32Sub);
            }
            LIRInstruction::F32Mul => {
                func.instruction(&Instruction::F32Mul);
            }
            LIRInstruction::F32Div => {
                func.instruction(&Instruction::F32Div);
            }

            // Arithmetic (f64)
            LIRInstruction::F64Add => {
                func.instruction(&Instruction::F64Add);
            }
            LIRInstruction::F64Sub => {
                func.instruction(&Instruction::F64Sub);
            }
            LIRInstruction::F64Mul => {
                func.instruction(&Instruction::F64Mul);
            }
            LIRInstruction::F64Div => {
                func.instruction(&Instruction::F64Div);
            }

            // Comparison (i32)
            LIRInstruction::I32Eq => {
                func.instruction(&Instruction::I32Eq);
            }
            LIRInstruction::I32Ne => {
                func.instruction(&Instruction::I32Ne);
            }
            LIRInstruction::I32LtS => {
                func.instruction(&Instruction::I32LtS);
            }
            LIRInstruction::I32LtU => {
                func.instruction(&Instruction::I32LtU);
            }
            LIRInstruction::I32GtS => {
                func.instruction(&Instruction::I32GtS);
            }
            LIRInstruction::I32GtU => {
                func.instruction(&Instruction::I32GtU);
            }
            LIRInstruction::I32LeS => {
                func.instruction(&Instruction::I32LeS);
            }
            LIRInstruction::I32LeU => {
                func.instruction(&Instruction::I32LeU);
            }
            LIRInstruction::I32GeS => {
                func.instruction(&Instruction::I32GeS);
            }
            LIRInstruction::I32GeU => {
                func.instruction(&Instruction::I32GeU);
            }

            // Comparison (f32)
            LIRInstruction::F32Eq => {
                func.instruction(&Instruction::F32Eq);
            }
            LIRInstruction::F32Ne => {
                func.instruction(&Instruction::F32Ne);
            }
            LIRInstruction::F32Lt => {
                func.instruction(&Instruction::F32Lt);
            }
            LIRInstruction::F32Gt => {
                func.instruction(&Instruction::F32Gt);
            }
            LIRInstruction::F32Le => {
                func.instruction(&Instruction::F32Le);
            }
            LIRInstruction::F32Ge => {
                func.instruction(&Instruction::F32Ge);
            }

            // Comparison (f64)
            LIRInstruction::F64Eq => {
                func.instruction(&Instruction::F64Eq);
            }
            LIRInstruction::F64Ne => {
                func.instruction(&Instruction::F64Ne);
            }
            LIRInstruction::F64Lt => {
                func.instruction(&Instruction::F64Lt);
            }
            LIRInstruction::F64Gt => {
                func.instruction(&Instruction::F64Gt);
            }
            LIRInstruction::F64Le => {
                func.instruction(&Instruction::F64Le);
            }
            LIRInstruction::F64Ge => {
                func.instruction(&Instruction::F64Ge);
            }

            // Memory
            LIRInstruction::I32Load(align, offset) => {
                func.instruction(&Instruction::I32Load(MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: 0,
                }));
            }
            LIRInstruction::I64Load(align, offset) => {
                func.instruction(&Instruction::I64Load(MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: 0,
                }));
            }
            LIRInstruction::F32Load(align, offset) => {
                func.instruction(&Instruction::F32Load(MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: 0,
                }));
            }
            LIRInstruction::F64Load(align, offset) => {
                func.instruction(&Instruction::F64Load(MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: 0,
                }));
            }
            LIRInstruction::I32Store(align, offset) => {
                func.instruction(&Instruction::I32Store(MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: 0,
                }));
            }
            LIRInstruction::I64Store(align, offset) => {
                func.instruction(&Instruction::I64Store(MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: 0,
                }));
            }
            LIRInstruction::F32Store(align, offset) => {
                func.instruction(&Instruction::F32Store(MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: 0,
                }));
            }
            LIRInstruction::F64Store(align, offset) => {
                func.instruction(&Instruction::F64Store(MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: 0,
                }));
            }
            LIRInstruction::MemorySize => {
                func.instruction(&Instruction::MemorySize(0));
            }
            LIRInstruction::MemoryGrow => {
                func.instruction(&Instruction::MemoryGrow(0));
            }

            // Variables
            LIRInstruction::LocalGet(idx) => {
                func.instruction(&Instruction::LocalGet(*idx));
            }
            LIRInstruction::LocalSet(idx) => {
                func.instruction(&Instruction::LocalSet(*idx));
            }
            LIRInstruction::LocalTee(idx) => {
                func.instruction(&Instruction::LocalTee(*idx));
            }
            LIRInstruction::GlobalGet(idx) => {
                func.instruction(&Instruction::GlobalGet(*idx));
            }
            LIRInstruction::GlobalSet(idx) => {
                func.instruction(&Instruction::GlobalSet(*idx));
            }

            // Function calls
            LIRInstruction::Call(idx) => {
                func.instruction(&Instruction::Call(*idx));
            }
            LIRInstruction::CallIndirect(type_idx) => {
                func.instruction(&Instruction::CallIndirect {
                    ty: *type_idx,
                    table: 0,
                });
            }

            // Stack manipulation
            LIRInstruction::Drop => {
                func.instruction(&Instruction::Drop);
            }
            LIRInstruction::Select => {
                func.instruction(&Instruction::Select);
            }

            // Type conversions
            LIRInstruction::I32WrapI64 => {
                func.instruction(&Instruction::I32WrapI64);
            }
            LIRInstruction::I64ExtendI32S => {
                func.instruction(&Instruction::I64ExtendI32S);
            }
            LIRInstruction::I64ExtendI32U => {
                func.instruction(&Instruction::I64ExtendI32U);
            }
            LIRInstruction::I32TruncF32S => {
                func.instruction(&Instruction::I32TruncF32S);
            }
            LIRInstruction::I32TruncF32U => {
                func.instruction(&Instruction::I32TruncF32U);
            }
            LIRInstruction::I32TruncF64S => {
                func.instruction(&Instruction::I32TruncF64S);
            }
            LIRInstruction::I32TruncF64U => {
                func.instruction(&Instruction::I32TruncF64U);
            }
            LIRInstruction::I64TruncF32S => {
                func.instruction(&Instruction::I64TruncF32S);
            }
            LIRInstruction::I64TruncF32U => {
                func.instruction(&Instruction::I64TruncF32U);
            }
            LIRInstruction::I64TruncF64S => {
                func.instruction(&Instruction::I64TruncF64S);
            }
            LIRInstruction::I64TruncF64U => {
                func.instruction(&Instruction::I64TruncF64U);
            }
            LIRInstruction::F32ConvertI32S => {
                func.instruction(&Instruction::F32ConvertI32S);
            }
            LIRInstruction::F32ConvertI32U => {
                func.instruction(&Instruction::F32ConvertI32U);
            }
            LIRInstruction::F32ConvertI64S => {
                func.instruction(&Instruction::F32ConvertI64S);
            }
            LIRInstruction::F32ConvertI64U => {
                func.instruction(&Instruction::F32ConvertI64U);
            }
            LIRInstruction::F32DemoteF64 => {
                func.instruction(&Instruction::F32DemoteF64);
            }
            LIRInstruction::F64ConvertI32S => {
                func.instruction(&Instruction::F64ConvertI32S);
            }
            LIRInstruction::F64ConvertI32U => {
                func.instruction(&Instruction::F64ConvertI32U);
            }
            LIRInstruction::F64ConvertI64S => {
                func.instruction(&Instruction::F64ConvertI64S);
            }
            LIRInstruction::F64ConvertI64U => {
                func.instruction(&Instruction::F64ConvertI64U);
            }
            LIRInstruction::F64PromoteF32 => {
                func.instruction(&Instruction::F64PromoteF32);
            }

            // Memory management instructions (implemented as runtime function calls)
            LIRInstruction::MemAlloc => {
                // Call imported memory allocator function
                // Assumes function index for mem_alloc is known
                func.instruction(&Instruction::Call(
                    self.get_runtime_function_index("mem_alloc")?,
                ));
            }
            LIRInstruction::MemRetain => {
                // Call imported retain function
                func.instruction(&Instruction::Call(
                    self.get_runtime_function_index("mem_retain")?,
                ));
            }
            LIRInstruction::MemRelease => {
                // Call imported release function
                func.instruction(&Instruction::Call(
                    self.get_runtime_function_index("mem_release")?,
                ));
            }
            LIRInstruction::MemCollect => {
                // Call imported garbage collection function
                func.instruction(&Instruction::Call(
                    self.get_runtime_function_index("mem_collect")?,
                ));
            }
            LIRInstruction::MemGetRefCount => {
                // Call imported get reference count function
                func.instruction(&Instruction::Call(
                    self.get_runtime_function_index("mem_get_ref_count")?,
                ));
            }
        }
        Ok(())
    }

    /// Get the function index for a runtime memory management function
    fn get_runtime_function_index(&self, function_name: &str) -> Result<u32, CompilerError> {
        self.function_indices
            .get(function_name)
            .copied()
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    format!("Runtime function not found: {}", function_name),
                    None,
                    None,
                )
            })
    }

    fn lir_type_to_wasm_val_type(&self, lir_type: &LIRType) -> ValType {
        match lir_type {
            LIRType::I32 => ValType::I32,
            LIRType::I64 => ValType::I64,
            LIRType::F32 => ValType::F32,
            LIRType::F64 => ValType::F64,
        }
    }

    /// Add memory management runtime imports
    fn add_memory_runtime_imports(
        &mut self,
        import_section: &mut ImportSection,
    ) -> Result<(), CompilerError> {
        // Define memory management runtime function signatures with unique signatures
        let runtime_functions = vec![
            (
                "mem_alloc",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ), // (type_id, size) -> address
            ("mem_retain", vec![ValType::I32], vec![]), // (address) -> ()
            ("mem_release", vec![ValType::I32], vec![]), // (address) -> ()
            ("mem_collect", vec![], vec![ValType::I32]), // () -> freed_count
            ("mem_get_ref_count", vec![ValType::I32], vec![ValType::I32]), // (address) -> ref_count
        ];

        for (name, param_types, return_types) in runtime_functions {
            // Create unique key that includes both params and returns
            let unique_key = (param_types.clone(), return_types.clone());

            // Get or create function type
            let type_idx = if let Some(&idx) = self.runtime_function_type_map.get(&unique_key) {
                idx
            } else {
                let idx = self.next_type_idx;
                self.runtime_function_type_map.insert(unique_key, idx);

                // Also add to function_types for backward compatibility
                self.function_types.insert(param_types.clone(), idx);

                self.next_type_idx += 1;
                idx
            };

            // Add import
            import_section.import("memory_runtime", name, EntityType::Function(type_idx));

            // Track function index
            self.function_indices
                .insert(name.to_string(), self.import_count);
            self.import_count += 1;
        }

        Ok(())
    }
}

impl Default for WasmGenerator {
    fn default() -> Self {
        Self::new()
    }
}
