use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::register_stdlib_function;
use crate::types::WasmType;
use wasm_encoder::Instruction;

/// File class implementation for Clean Language
/// Provides file I/O operations as static methods
pub struct FileClass;

impl Default for FileClass {
    fn default() -> Self {
        Self::new()
    }
}

impl FileClass {
    pub fn new() -> Self {
        Self
    }

    /// Register only specification-compliant file functions
    /// Clean Language specification defines only: file.read, file.write, file.append, file.exists, file.delete
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_basic_operations(codegen)?;
        self.register_info_operations(codegen)?;
        Ok(())
    }

    fn register_basic_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        register_stdlib_function(
            codegen,
            "file.read",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_read(codegen)?,
        )?;

        register_stdlib_function(
            codegen,
            "file.write",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_write(codegen)?,
        )?;

        register_stdlib_function(
            codegen,
            "file.append",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_append(codegen)?,
        )?;

        register_stdlib_function(
            codegen,
            "file.delete",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_delete(codegen)?,
        )?;

        Ok(())
    }

    fn register_info_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        register_stdlib_function(
            codegen,
            "file.exists",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_exists(codegen)?,
        )?;

        Ok(())
    }

    fn generate_read(&self, codegen: &CodeGenerator) -> Result<Vec<Instruction>, CompilerError> {
        let import_index = codegen.get_file_import_index("file_read").ok_or_else(|| {
            CompilerError::codegen_error(
                "File import function 'file_read' not found",
                Some("Make sure file imports are properly registered".to_string()),
                None,
            )
        })?;

        Ok(vec![
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(8192),
            Instruction::Call(import_index),
        ])
    }

    fn generate_write(&self, codegen: &CodeGenerator) -> Result<Vec<Instruction>, CompilerError> {
        let import_index = codegen.get_file_import_index("file_write").ok_or_else(|| {
            CompilerError::codegen_error(
                "File import function 'file_write' not found",
                Some("Make sure file imports are properly registered".to_string()),
                None,
            )
        })?;

        Ok(vec![
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Call(import_index),
        ])
    }

    fn generate_append(&self, codegen: &CodeGenerator) -> Result<Vec<Instruction>, CompilerError> {
        let import_index = codegen
            .get_file_import_index("file_append")
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    "File import function 'file_append' not found",
                    Some("Make sure file imports are properly registered".to_string()),
                    None,
                )
            })?;

        Ok(vec![
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Call(import_index),
        ])
    }

    fn generate_delete(&self, codegen: &CodeGenerator) -> Result<Vec<Instruction>, CompilerError> {
        let import_index = codegen
            .get_file_import_index("file_delete")
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    "File import function 'file_delete' not found",
                    Some("Make sure file imports are properly registered".to_string()),
                    None,
                )
            })?;

        Ok(vec![
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Call(import_index),
        ])
    }

    fn generate_exists(&self, codegen: &CodeGenerator) -> Result<Vec<Instruction>, CompilerError> {
        let import_index = codegen
            .get_file_import_index("file_exists")
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    "File import function 'file_exists' not found",
                    Some("Make sure file imports are properly registered".to_string()),
                    None,
                )
            })?;

        Ok(vec![
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Call(import_index),
        ])
    }
}
