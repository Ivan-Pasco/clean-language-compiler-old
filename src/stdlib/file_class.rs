use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::{register_stdlib_function, register_stdlib_function_with_locals};
use crate::types::WasmType;
use wasm_encoder::{BlockType, Instruction, MemArg};

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
    /// Clean Language specification defines only: file.read, file.write, file.append,
    /// file.exists, file.delete, file.lines
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

        // file.lines is built from file.read and string.split, so it needs
        // no host function of its own (no host provides a `file_lines`).
        if codegen.get_file_import_index("file_read").is_some()
            && codegen.get_function_index("file.read").is_some()
            && codegen.get_function_index("string.split").is_some()
        {
            let body = self.generate_lines(codegen)?;
            register_stdlib_function_with_locals(
                codegen,
                "file.lines",
                &[WasmType::I32],
                Some(WasmType::I32),
                // 1: list, 2: line count, 3: index, 4: line, 5: line length
                &[WasmType::I32; 5],
                body,
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

    /// `file.lines(path)`: the file's text split on `\n` into a
    /// `list<string>`. A `\r` before a `\n` is dropped, and so is the empty
    /// piece after the last `\n`, so `"a\nb\n"` is `["a", "b"]` and an empty
    /// or unreadable file is an empty list.
    fn generate_lines(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<Vec<Instruction<'static>>, CompilerError> {
        let missing = |name: &str| {
            CompilerError::codegen_error(
                format!("Function '{name}' must be registered before 'file.lines'"),
                None,
                None,
            )
        };
        let read = codegen
            .get_function_index("file.read")
            .ok_or_else(|| missing("file.read"))?;
        let split = codegen
            .get_function_index("string.split")
            .ok_or_else(|| missing("string.split"))?;
        let newline = codegen.get_or_create_string_offset("\n")?;

        let word = |offset: u64| MemArg {
            offset,
            align: 2,
            memory_index: 0,
        };
        // Address of element `index_local` of the list in local 1.
        let element_address = |index: Vec<Instruction<'static>>| {
            let mut code = vec![Instruction::LocalGet(1)];
            code.extend(index);
            code.extend([
                Instruction::I32Const(2),
                Instruction::I32Shl,
                Instruction::I32Add,
            ]);
            code
        };

        let mut code = vec![
            // list = string.split(file.read(path), "\n")
            Instruction::LocalGet(0),
            Instruction::Call(read),
            Instruction::I32Const(newline as i32),
            Instruction::Call(split),
            Instruction::LocalSet(1),
            // count = list.length
            Instruction::LocalGet(1),
            Instruction::I32Load(word(0)),
            Instruction::LocalSet(2),
            // for each line: drop a trailing '\r'
            Instruction::I32Const(0),
            Instruction::LocalSet(3),
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            Instruction::I32GeU,
            Instruction::BrIf(1),
        ];
        code.extend(element_address(vec![Instruction::LocalGet(3)]));
        code.extend([
            Instruction::I32Load(word(16)),
            Instruction::LocalSet(4),
            Instruction::LocalGet(4),
            Instruction::I32Load(word(0)),
            Instruction::LocalSet(5),
            Instruction::LocalGet(5),
            Instruction::I32Const(0),
            Instruction::I32GtU,
            Instruction::If(BlockType::Empty),
            // last content byte is at line + 4 + length - 1
            Instruction::LocalGet(4),
            Instruction::LocalGet(5),
            Instruction::I32Add,
            Instruction::I32Load8U(MemArg {
                offset: 3,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Const(13),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(4),
            Instruction::LocalGet(5),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::I32Store(word(0)),
            Instruction::End,
            Instruction::End,
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(3),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            // the empty piece after a final '\n' is not a line
            Instruction::LocalGet(2),
            Instruction::I32Const(0),
            Instruction::I32GtU,
            Instruction::If(BlockType::Empty),
        ]);
        let last = vec![
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Sub,
        ];
        code.extend(element_address(last));
        code.extend([
            Instruction::I32Load(word(16)),
            Instruction::I32Load(word(0)),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::I32Store(word(0)),
            Instruction::End,
            Instruction::End,
            Instruction::LocalGet(1),
        ]);
        Ok(code)
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
