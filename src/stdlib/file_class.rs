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
    ///
    /// Skipped when no `file_*` import is reachable — see HttpClass for the
    /// same pattern; both classes generate wrappers over Layer 2 host bridge
    /// imports that get tree-shaken by the Import Minimality Rule.
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // fs.write_bytes may be reachable independently of file_* (the errors
        // dashboard's tarball-upload chain uses fs.write_bytes without ever
        // touching file.write). Register it whenever _fs_ imports exist,
        // even if file_* is entirely tree-shaken.
        let has_file = codegen.has_reachable_prefix("file_");
        let has_fs = codegen.has_reachable_prefix("_fs_");
        if !has_file && !has_fs {
            tracing::debug!(
                "FileClass: no file_* or _fs_* imports reachable, skipping wrapper registration"
            );
            return Ok(());
        }
        if has_file {
            self.register_basic_operations(codegen)?;
            self.register_info_operations(codegen)?;
        }
        if has_fs {
            self.register_fs_bytes_operations(codegen)?;
        }
        Ok(())
    }

    /// Register `fs.*` wrappers over the bytes-safe host bridges. Opaque
    /// handle convention — see spec/type-system.md §9b.
    fn register_fs_bytes_operations(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        if codegen.get_file_import_index("_fs_write_bytes").is_some() {
            register_stdlib_function(
                codegen,
                "fs.write_bytes",
                // (path_str_ptr, handle: i32) — path expands to (ptr+4, len);
                // handle passes through unchanged.
                &[WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
                self.generate_fs_write_bytes(codegen)?,
            )?;
        }
        Ok(())
    }

    fn generate_fs_write_bytes(
        &self,
        codegen: &CodeGenerator,
    ) -> Result<Vec<Instruction>, CompilerError> {
        let import_index = codegen
            .get_file_import_index("_fs_write_bytes")
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    "File import function '_fs_write_bytes' not found",
                    Some("Make sure fs imports are properly registered".to_string()),
                    None,
                )
            })?;

        // Local 0: path string pointer (points at [4-byte length][utf8]).
        // Local 1: opaque byte handle (already a valid ptr to [len][bytes]).
        // Expand path to (content_ptr, length); pass handle unchanged.
        Ok(vec![
            // path_ptr := local0 + 4
            Instruction::LocalGet(0),
            Instruction::I32Const(4),
            Instruction::I32Add,
            // path_len := i32.load(local0)
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // handle passes through
            Instruction::LocalGet(1),
            Instruction::Call(import_index),
        ])
    }

    fn register_basic_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Each wrapper is gated on whether its specific host import was emitted.
        // The Import Minimality Rule tree-shakes unused `file_*` imports, so a
        // program that only calls `file.read` will not have `file_delete`
        // registered. Generating the `file.delete` wrapper in that case would
        // reference a missing import (E007, mirrors HttpClass fix in 0.30.71).
        if codegen.get_file_import_index("file_read").is_some() {
            register_stdlib_function(
                codegen,
                "file.read",
                &[WasmType::I32],
                Some(WasmType::I32),
                self.generate_read(codegen)?,
            )?;
        }

        if codegen.get_file_import_index("file_write").is_some() {
            register_stdlib_function(
                codegen,
                "file.write",
                &[WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
                self.generate_write(codegen)?,
            )?;
        }

        if codegen.get_file_import_index("file_append").is_some() {
            register_stdlib_function(
                codegen,
                "file.append",
                &[WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
                self.generate_append(codegen)?,
            )?;
        }

        if codegen.get_file_import_index("file_delete").is_some() {
            register_stdlib_function(
                codegen,
                "file.delete",
                &[WasmType::I32],
                Some(WasmType::I32),
                self.generate_delete(codegen)?,
            )?;
        }

        Ok(())
    }

    fn register_info_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        if codegen.get_file_import_index("file_exists").is_some() {
            register_stdlib_function(
                codegen,
                "file.exists",
                &[WasmType::I32],
                Some(WasmType::I32),
                self.generate_exists(codegen)?,
            )?;
        }

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
