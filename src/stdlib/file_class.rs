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
        // Only register basic file operations (specification-compliant)
        self.register_basic_operations(codegen)?;

        // Only register file.exists from info operations (specification-compliant)
        self.register_info_operations(codegen)?;

        // DISABLED - NOT IN SPECIFICATION:
        // Directory and path operations are not defined in Clean Language specification
        /* DISABLED:
        // Directory operations
        self.register_directory_operations(codegen)?;

        // Path operations
        self.register_path_operations(codegen)?;
        */

        Ok(())
    }

    fn register_basic_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // file.read(string path) -> string - SPECIFICATION COMPLIANT
        register_stdlib_function(
            codegen,
            "file.read",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_read(codegen)?,
        )?;

        // NON-SPECIFICATION FUNCTIONS DISABLED
        // The following functions are not in the Clean Language specification
        // and have been disabled to maintain specification compliance.
        // Only file.read, file.write, file.append, file.exists, file.delete are specified.

        /* DISABLED - NOT IN SPECIFICATION:
        // File.readBytes(string path) -> array<integer>
        register_stdlib_function(
            codegen,
            "File.readBytes",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_read_bytes()
        )?;

        // File.readLines(string path) -> array<string>
        register_stdlib_function(
            codegen,
            "File.readLines",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_read_lines()
        )?;
        */

        // file.write(string path, string content) -> boolean - SPECIFICATION COMPLIANT
        register_stdlib_function(
            codegen,
            "file.write",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_write(codegen)?,
        )?;

        /* DISABLED - NOT IN SPECIFICATION:
        // File.writeBytes(string path, array<integer> data) -> boolean
        register_stdlib_function(
            codegen,
            "File.writeBytes",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_write_bytes()
        )?;
        */

        // file.append(string path, string content) -> boolean - SPECIFICATION COMPLIANT
        register_stdlib_function(
            codegen,
            "file.append",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_append(codegen)?,
        )?;

        /* DISABLED - NOT IN SPECIFICATION:
        // File.copy(string source, string destination) -> boolean
        register_stdlib_function(
            codegen,
            "File.copy",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_copy()
        )?;

        // File.move(string source, string destination) -> boolean
        register_stdlib_function(
            codegen,
            "File.move",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_move()
        )?;
        */

        // file.delete(string path) -> boolean - SPECIFICATION COMPLIANT
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
        // file.exists(string path) -> boolean - SPECIFICATION COMPLIANT
        register_stdlib_function(
            codegen,
            "file.exists",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_exists(codegen)?,
        )?;

        // DISABLED - NOT IN SPECIFICATION:
        /* All other file info functions are not in Clean Language specification
        // File.isFile(string path) -> boolean
        register_stdlib_function(
            codegen,
            "File.isFile",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_is_file()
        )?;

        // File.isDirectory(string path) -> boolean
        register_stdlib_function(
            codegen,
            "File.isDirectory",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_is_directory()
        )?;

        // File.size(string path) -> integer
        register_stdlib_function(
            codegen,
            "File.size",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_size()
        )?;

        // File.lastModified(string path) -> integer
        register_stdlib_function(
            codegen,
            "File.lastModified",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_last_modified()
        )?;

        // File.permissions(string path) -> string
        register_stdlib_function(
            codegen,
            "File.permissions",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_permissions()
        )?;
        */

        Ok(())
    }

    #[allow(dead_code)]
    fn register_directory_operations(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // File.listFiles(string path) -> array<string>
        register_stdlib_function(
            codegen,
            "File.listFiles",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_files(),
        )?;

        // File.listDirectories(string path) -> array<string>
        register_stdlib_function(
            codegen,
            "File.listDirectories",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_list_directories(),
        )?;

        // File.createDirectory(string path) -> boolean
        register_stdlib_function(
            codegen,
            "File.createDirectory",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_create_directory(),
        )?;

        // File.createDirectories(string path) -> boolean
        register_stdlib_function(
            codegen,
            "File.createDirectories",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_create_directories(),
        )?;

        // File.deleteDirectory(string path) -> boolean
        register_stdlib_function(
            codegen,
            "File.deleteDirectory",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_delete_directory(),
        )?;

        Ok(())
    }

    #[allow(dead_code)]
    fn register_path_operations(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // File.getFileName(string path) -> string
        register_stdlib_function(
            codegen,
            "File.getFileName",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_get_file_name(),
        )?;

        // File.getFileExtension(string path) -> string
        register_stdlib_function(
            codegen,
            "File.getFileExtension",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_get_file_extension(),
        )?;

        // File.getDirectory(string path) -> string
        register_stdlib_function(
            codegen,
            "File.getDirectory",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_get_directory(),
        )?;

        // File.getAbsolutePath(string path) -> string
        register_stdlib_function(
            codegen,
            "File.getAbsolutePath",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_get_absolute_path(),
        )?;

        // File.joinPath(string path1, string path2) -> string
        register_stdlib_function(
            codegen,
            "File.joinPath",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_join_path(),
        )?;

        // File.normalizePath(string path) -> string
        register_stdlib_function(
            codegen,
            "File.normalizePath",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_normalize_path(),
        )?;

        Ok(())
    }

    // Implementation methods for file operations

    fn generate_read(&self, codegen: &CodeGenerator) -> Result<Vec<Instruction>, CompilerError> {
        let import_index = codegen.get_file_import_index("file_read").ok_or_else(|| {
            CompilerError::codegen_error(
                "File import function 'file_read' not found",
                Some("Make sure file imports are properly registered".to_string()),
                None,
            )
        })?;

        Ok(vec![
            // Load path string data pointer and length
            Instruction::LocalGet(0), // path string pointer
            Instruction::I32Const(4),
            Instruction::I32Add, // path data ptr (skip 4-byte length prefix)
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // path length
            // Allocate memory for result string - use simple allocation scheme
            // For now, use fixed memory location for file read buffer
            Instruction::I32Const(8192), // resultPtr - fixed buffer at 8KB mark
            // Call the file_read import function (expects 3 params: pathPtr, pathLen, resultPtr)
            Instruction::Call(import_index),
        ])
    }

    #[allow(dead_code)]
    fn generate_read_bytes(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.readBytes() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return error message at runtime");
        eprintln!("         WebAssembly host import 'file_read_bytes' needed for implementation");

        // Return pointer to a pre-allocated error message
        // This should be replaced with proper error string allocation when runtime supports it
        vec![
            Instruction::I32Const(0), // Empty string for now - better than crashing
        ]
    }

    #[allow(dead_code)]
    fn generate_read_lines(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.readLines() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return empty result at runtime");
        eprintln!("         WebAssembly host import 'file_read_lines' needed for implementation");

        // Return pointer to empty result instead of crashing
        vec![
            Instruction::I32Const(0), // Empty array for now
        ]
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
            // Load path string data pointer and length
            Instruction::LocalGet(0), // path string pointer
            Instruction::I32Const(4),
            Instruction::I32Add, // path data ptr
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // path length
            // Load content string data pointer and length
            Instruction::LocalGet(1), // content string pointer
            Instruction::I32Const(4),
            Instruction::I32Add, // content data ptr
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // content length
            // Call the file_write import function
            Instruction::Call(import_index),
        ])
    }

    #[allow(dead_code)]
    fn generate_write_bytes(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.writeBytes() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return false (failure) at runtime");
        eprintln!("         WebAssembly host import 'file_write_bytes' needed for implementation");

        vec![
            Instruction::I32Const(0), // Return false (operation failed)
        ]
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
            // Load path string data pointer and length
            Instruction::LocalGet(0), // path string pointer
            Instruction::I32Const(4),
            Instruction::I32Add, // path data ptr
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // path length
            // Load content string data pointer and length
            Instruction::LocalGet(1), // content string pointer
            Instruction::I32Const(4),
            Instruction::I32Add, // content data ptr
            Instruction::LocalGet(1),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // content length
            // Call the file_append import function
            Instruction::Call(import_index),
        ])
    }

    #[allow(dead_code)]
    fn generate_copy(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.copy() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return false (failure) at runtime");
        eprintln!("         WebAssembly host import 'file_copy' needed for implementation");

        vec![
            Instruction::I32Const(0), // Return false (operation failed)
        ]
    }

    #[allow(dead_code)]
    fn generate_move(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.move() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return false (failure) at runtime");
        eprintln!("         WebAssembly host import 'file_move' needed for implementation");

        vec![
            Instruction::I32Const(0), // Return false (operation failed)
        ]
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
            // Load path string data pointer and length
            Instruction::LocalGet(0), // path string pointer
            Instruction::I32Const(4),
            Instruction::I32Add, // path data ptr
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // path length
            // Call the file_delete import function
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
            // Load path string data pointer and length
            Instruction::LocalGet(0), // path string pointer
            Instruction::I32Const(4),
            Instruction::I32Add, // path data ptr
            Instruction::LocalGet(0),
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // path length
            // Call the file_exists import function
            Instruction::Call(import_index),
        ])
    }

    #[allow(dead_code)]
    fn generate_is_file(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.isFile() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return false at runtime");
        eprintln!("         WebAssembly host import 'file_is_file' needed for implementation");

        vec![
            Instruction::I32Const(0), // Return false (cannot determine)
        ]
    }

    #[allow(dead_code)]
    fn generate_is_directory(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.isDirectory() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return false at runtime");
        eprintln!("         WebAssembly host import 'file_is_directory' needed for implementation");

        vec![
            Instruction::I32Const(0), // Return false (cannot determine)
        ]
    }

    #[allow(dead_code)]
    fn generate_size(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.size() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return 0 at runtime");
        eprintln!("         WebAssembly host import 'file_size' needed for implementation");

        vec![
            Instruction::I32Const(0), // Return 0 (unknown size)
        ]
    }

    #[allow(dead_code)]
    fn generate_last_modified(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.lastModified() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return 0 (timestamp) at runtime");
        eprintln!(
            "         WebAssembly host import 'file_last_modified' needed for implementation"
        );

        vec![
            Instruction::I32Const(0), // Return 0 (no timestamp available)
        ]
    }

    #[allow(dead_code)]
    fn generate_permissions(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.permissions() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return empty string at runtime");
        eprintln!("         WebAssembly host import 'file_permissions' needed for implementation");

        vec![
            Instruction::I32Const(0), // Return empty string (no permissions available)
        ]
    }

    #[allow(dead_code)]
    fn generate_list_files(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.listFiles() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return empty array at runtime");
        eprintln!("         WebAssembly host import 'file_list_files' needed for implementation");

        vec![
            Instruction::I32Const(0), // Return empty array (no files listed)
        ]
    }

    #[allow(dead_code)]
    fn generate_list_directories(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.listDirectories() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return empty array at runtime");
        eprintln!(
            "         WebAssembly host import 'file_list_directories' needed for implementation"
        );

        vec![
            Instruction::I32Const(0), // Return empty array (no directories listed)
        ]
    }

    #[allow(dead_code)]
    fn generate_create_directory(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.createDirectory() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return false (failure) at runtime");
        eprintln!(
            "         WebAssembly host import 'file_create_directory' needed for implementation"
        );

        vec![
            Instruction::I32Const(0), // Return false (operation failed)
        ]
    }

    #[allow(dead_code)]
    fn generate_create_directories(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.createDirectories() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return false (failure) at runtime");
        eprintln!(
            "         WebAssembly host import 'file_create_directories' needed for implementation"
        );

        vec![
            Instruction::I32Const(0), // Return false (operation failed)
        ]
    }

    #[allow(dead_code)]
    fn generate_delete_directory(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.deleteDirectory() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return false (failure) at runtime");
        eprintln!(
            "         WebAssembly host import 'file_delete_directory' needed for implementation"
        );

        vec![
            Instruction::I32Const(0), // Return false (operation failed)
        ]
    }

    #[allow(dead_code)]
    fn generate_get_file_name(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.getFileName() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return original path at runtime (not just filename)");
        eprintln!(
            "         WebAssembly host import 'file_get_name' needed for proper implementation"
        );

        vec![
            Instruction::LocalGet(0), // Return original path as fallback
        ]
    }

    #[allow(dead_code)]
    fn generate_get_file_extension(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.getFileExtension() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return empty string at runtime");
        eprintln!(
            "         WebAssembly host import 'file_get_extension' needed for implementation"
        );

        vec![
            Instruction::I32Const(0), // Return empty string (no extension detected)
        ]
    }

    #[allow(dead_code)]
    fn generate_get_directory(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.getDirectory() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return empty string at runtime");
        eprintln!(
            "         WebAssembly host import 'file_get_directory' needed for implementation"
        );

        vec![
            Instruction::I32Const(0), // Return empty string (no directory path)
        ]
    }

    #[allow(dead_code)]
    fn generate_get_absolute_path(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.getAbsolutePath() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return original path at runtime (may not be absolute)");
        eprintln!(
            "         WebAssembly host import 'file_get_absolute_path' needed for implementation"
        );

        vec![
            Instruction::LocalGet(0), // Return original path as fallback
        ]
    }

    #[allow(dead_code)]
    fn generate_join_path(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.joinPath() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return first path at runtime (not properly joined)");
        eprintln!("         WebAssembly host import 'file_join_path' needed for implementation");

        vec![
            Instruction::LocalGet(0), // Return first path as fallback
        ]
    }

    #[allow(dead_code)]
    fn generate_normalize_path(&self) -> Vec<Instruction> {
        // Emit compile-time warning
        eprintln!("WARNING: File.normalizePath() is not yet supported by WebAssembly runtime");
        eprintln!("         Function will return original path at runtime (not normalized)");
        eprintln!(
            "         WebAssembly host import 'file_normalize_path' needed for implementation"
        );

        vec![
            Instruction::LocalGet(0), // Return original path as fallback
        ]
    }
}
