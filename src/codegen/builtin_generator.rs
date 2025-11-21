//! Built-in function generation for Clean Language WebAssembly compiler.

use wasm_encoder::{Instruction, ValType};
use crate::error::CompilerError;
use crate::types::WasmType;
use super::CodeGenerator;

impl CodeGenerator {
    /// Register all built-in standard library functions
    pub fn register_stdlib_functions(&mut self) -> Result<(), CompilerError> {
        // COMPLETE STANDARD LIBRARY: Enable all functions for full Clean Language support
        self.register_math_operations()?; // Re-enabled for power operator support
        // REMOVED: self.register_console_operations() creates obsolete wrapper functions
        // Console imports (print, printl, input, input_integer, etc.) are registered separately in new()
        self.register_type_conversion_operations()?; // Needed for toString() calls
        self.register_string_operations()?; // Critical for string methods like length()
        self.register_list_operations()?; // Critical for list methods like size()
        self.register_file_operations()?; // Needed for file I/O operations
        self.register_http_operations()?; // Needed for HTTP operations
        self.register_conditional_operations()?; // Needed for conditional operations
        
        // Register StandardLibrary functions including matrix operations
        // self.register_standard_library_functions()?;
        
        Ok(())
    }
    
    /// Register all StandardLibrary functions including matrix operations
    fn register_standard_library_functions(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::StandardLibrary;
        
        // Initialize StandardLibrary and register its functions
        let stdlib = StandardLibrary::new();
        
        // Register all StandardLibrary functions with the code generator
        // If this fails, continue anyway - we'll fall back to direct matrix function registration
        match stdlib.register_functions(self) {
            Ok(_) => {
                // StandardLibrary functions registered successfully
            },
            Err(_) => {
                // StandardLibrary registration failed, use fallback matrix function registration
                self.register_matrix_functions_directly()?;
            }
        }
        
        Ok(())
    }
    
    /// Fallback: Register matrix functions directly if StandardLibrary registration fails
    fn register_matrix_functions_directly(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::MatrixLiteralsManager;
        use crate::stdlib::MemoryManager;
        use std::cell::RefCell;
        use std::rc::Rc;
        
        // Create a shared memory manager and matrix literals manager
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024 * 1024))));
        let matrix_manager = MatrixLiteralsManager::new(memory_manager);
        
        // Register matrix functions
        matrix_manager.register_functions(self)?;
        
        Ok(())
    }

    fn register_math_operations(&mut self) -> Result<(), CompilerError> {
        // Math.abs - native implementation (F64 version)
        let abs_instructions = vec![
            Instruction::LocalGet(0), // get input value
            Instruction::F64Abs,      // apply absolute value
        ];
        self.register_function("math_abs", vec![ValType::F64], vec![ValType::F64], &abs_instructions)?;
        
        // Math.pow - critical for power operator (^) - needs host implementation
        self.register_import_function("env", "math_pow", vec![ValType::F64, ValType::F64], vec![ValType::F64])?;
        
        // Math.sqrt - commonly used mathematical function
        self.register_import_function("env", "math_sqrt", vec![ValType::F64], vec![ValType::F64])?;
        
        // Math.sin - trigonometric function
        self.register_import_function("env", "math_sin", vec![ValType::F64], vec![ValType::F64])?;
        
        // Math.cos - trigonometric function
        self.register_import_function("env", "math_cos", vec![ValType::F64], vec![ValType::F64])?;
        
        // Math.tan - trigonometric function
        self.register_import_function("env", "math_tan", vec![ValType::F64], vec![ValType::F64])?;
        
        // Math.max - native implementation
        let max_instructions = vec![
            Instruction::LocalGet(0), // first value
            Instruction::LocalGet(1), // second value
            Instruction::F64Max,      // get maximum
        ];
        self.register_function("math_max", vec![ValType::F64, ValType::F64], vec![ValType::F64], &max_instructions)?;
        
        // Math.min - native implementation
        let min_instructions = vec![
            Instruction::LocalGet(0), // first value
            Instruction::LocalGet(1), // second value
            Instruction::F64Min,      // get minimum
        ];
        self.register_function("math_min", vec![ValType::F64, ValType::F64], vec![ValType::F64], &min_instructions)?;
        
        // Math.floor - native implementation  
        let floor_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::F64Floor,
        ];
        self.register_function("math_floor", vec![ValType::F64], vec![ValType::F64], &floor_instructions)?;

        // Math.ceil - native implementation
        let ceil_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::F64Ceil,
        ];
        self.register_function("math_ceil", vec![ValType::F64], vec![ValType::F64], &ceil_instructions)?;

        // Math.round - native implementation
        let round_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::F64Nearest,
        ];
        self.register_function("math_round", vec![ValType::F64], vec![ValType::F64], &round_instructions)?;
        
        // Additional math operations for integer absolute value
        let abs_int_instructions = vec![
            Instruction::LocalGet(0), // get input value (i32)
            Instruction::LocalGet(0), // duplicate it
            Instruction::I32Const(31), // 31 (for sign extension)
            Instruction::I32ShrS,     // arithmetic right shift to get sign mask
            Instruction::LocalGet(0), // get input again
            Instruction::I32Xor,      // XOR with sign mask
            Instruction::LocalGet(0), // get input again
            Instruction::I32Const(31),
            Instruction::I32ShrS,     // sign mask again
            Instruction::I32Sub,      // subtract sign mask
        ];
        self.register_function("math_abs_int", vec![ValType::I32], vec![ValType::I32], &abs_int_instructions)?;
        
        Ok(())
    }

    fn register_string_operations(&mut self) -> Result<(), CompilerError> {
        // String operations - require host functions due to memory management complexity
        self.register_import_function("env", "string_length", vec![ValType::I32], vec![ValType::I32])?;
        // CRITICAL FIX: string_concat takes 4 args (ptr1, len1, ptr2, len2) matching host function
        self.register_import_function("env", "string_concat", vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_substring", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_index_of", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_to_upper", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_to_lower", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_trim", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_replace", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_split", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_starts_with", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_ends_with", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_contains", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_compare", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        
        // String namespace functions (static method style)
        self.register_import_function("env", "string.length", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.contains", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.indexOf", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.lastIndexOf", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.startsWith", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.endsWith", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.trim", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.trimStart", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.trimEnd", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.replace", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.substring", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.toUpperCase", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.toLowerCase", vec![ValType::I32], vec![ValType::I32])?;

        // Also register the underscore versions for method calls
        self.register_import_function("env", "string_toUpperCase", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_toLowerCase", vec![ValType::I32], vec![ValType::I32])?;

        Ok(())
    }

    fn register_list_operations(&mut self) -> Result<(), CompilerError> {
        // Basic array operations - these require host functions for memory management
        self.register_import_function("env", "array_get", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_set", vec![ValType::I32, ValType::I32, ValType::I32], vec![])?;
        self.register_import_function("env", "array_length", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_push", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_pop", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_slice", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_concat", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_reverse", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_sort", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_filter", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_map", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_reduce", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_find", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_contains", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;

        // List namespace functions (used in static method calls like list.contains())
        self.register_import_function("env", "list.allocate", vec![ValType::I32], vec![ValType::I32])?; // CRITICAL: needed for array literals
        self.register_import_function("env", "list.push", vec![ValType::I32, ValType::I32], vec![ValType::I32])?; // CRITICAL: needed for integer array literals
        self.register_import_function("env", "list.push_f64", vec![ValType::I32, ValType::F64], vec![ValType::I32])?; // CRITICAL: needed for float array literals
        self.register_import_function("env", "list.length", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "list.add", vec![ValType::I32, ValType::I32], vec![])?;
        self.register_import_function("env", "list.remove", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "list.contains", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "list.get", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "list.set", vec![ValType::I32, ValType::I32, ValType::I32], vec![])?;
        self.register_import_function("env", "list.clear", vec![ValType::I32], vec![])?;
        self.register_import_function("env", "list.isEmpty", vec![ValType::I32], vec![ValType::I32])?;

        Ok(())
    }

    fn register_console_operations(&mut self) -> Result<(), CompilerError> {
        // Core print/println functions - these match the host function signatures
        self.register_import_function("env", "print", vec![ValType::I32, ValType::I32], vec![])?;
        self.register_import_function("env", "printl", vec![ValType::I32, ValType::I32], vec![])?;

        // Console operations - these need host environment support for actual I/O
        self.register_import_function("env", "print_string", vec![ValType::I32], vec![])?;
        self.register_import_function("env", "print_integer", vec![ValType::I32], vec![])?;
        self.register_import_function("env", "print_float", vec![ValType::F64], vec![])?;
        self.register_import_function("env", "print_boolean", vec![ValType::I32], vec![])?;
        self.register_import_function("env", "console_log", vec![ValType::I32], vec![])?;
        self.register_import_function("env", "console_error", vec![ValType::I32], vec![])?;
        self.register_import_function("env", "console_warn", vec![ValType::I32], vec![])?;
        self.register_import_function("env", "console_input", vec![ValType::I32], vec![ValType::I32])?;

        // Direct input function (alias for console_input)
        self.register_import_function("env", "input", vec![ValType::I32], vec![ValType::I32])?;

        // Input namespace methods - require host support
        self.register_import_function("env", "input_integer", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "input_float", vec![ValType::I32], vec![ValType::F64])?;
        self.register_import_function("env", "input_yesno", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "input_range", vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;

        Ok(())
    }

    fn register_type_conversion_operations(&mut self) -> Result<(), CompilerError> {
        // Native type conversions - basic numeric conversions
        
        // int_to_float (i32 -> f64)
        let int_to_float_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::F64ConvertI32S, // signed i32 to f64
        ];
        self.register_function("int_to_float", vec![ValType::I32], vec![ValType::F64], &int_to_float_instructions)?;
        
        // float_to_int (f64 -> i32)
        let float_to_int_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::I32TruncF64S, // signed f64 to i32 (truncate)
        ];
        self.register_function("float_to_int", vec![ValType::F64], vec![ValType::I32], &float_to_int_instructions)?;
        
        // Method-style conversions - identity functions for same types
        
        // integer.toInteger (identity)
        let int_to_int_instructions = vec![
            Instruction::LocalGet(0),
        ];
        self.register_function("integer.toInteger", vec![ValType::I32], vec![ValType::I32], &int_to_int_instructions)?;
        
        // number.toNumber (identity)
        let num_to_num_instructions = vec![
            Instruction::LocalGet(0),
        ];
        self.register_function("number.toNumber", vec![ValType::F64], vec![ValType::F64], &num_to_num_instructions)?;
        
        // boolean.toBoolean (identity)
        let bool_to_bool_instructions = vec![
            Instruction::LocalGet(0),
        ];
        self.register_function("boolean.toBoolean", vec![ValType::I32], vec![ValType::I32], &bool_to_bool_instructions)?;
        
        // string.toString (identity)
        let str_to_str_instructions = vec![
            Instruction::LocalGet(0),
        ];
        self.register_function("string.toString", vec![ValType::I32], vec![ValType::I32], &str_to_str_instructions)?;
        
        // Cross-type conversions
        
        // integer.toNumber (i32 -> f64)
        self.register_function("integer.toNumber", vec![ValType::I32], vec![ValType::F64], &int_to_float_instructions)?;
        
        // number.toInteger (f64 -> i32)
        self.register_function("number.toInteger", vec![ValType::F64], vec![ValType::I32], &float_to_int_instructions)?;
        
        // boolean.toInteger (i32 -> i32, already boolean is stored as i32)
        self.register_function("boolean.toInteger", vec![ValType::I32], vec![ValType::I32], &int_to_int_instructions)?;
        
        // integer.toBoolean (i32 -> i32, non-zero is true)
        let int_to_bool_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::I32Const(0),
            Instruction::I32Ne, // not equal to 0
        ];
        self.register_function("integer.toBoolean", vec![ValType::I32], vec![ValType::I32], &int_to_bool_instructions)?;
        
        // boolean.toNumber (i32 -> f64)
        let bool_to_num_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::F64ConvertI32S,
        ];
        self.register_function("boolean.toNumber", vec![ValType::I32], vec![ValType::F64], &bool_to_num_instructions)?;
        
        // number.toBoolean (f64 -> i32, non-zero is true)
        let num_to_bool_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::F64Const(0.0),
            Instruction::F64Ne, // not equal to 0.0
        ];
        self.register_function("number.toBoolean", vec![ValType::F64], vec![ValType::I32], &num_to_bool_instructions)?;
        
        // String conversions require host functions for now
        self.register_import_function("env", "int_to_string", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "float_to_string", vec![ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "bool_to_string", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_to_int", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_to_float", vec![ValType::I32], vec![ValType::F64])?;
        self.register_import_function("env", "string_to_bool", vec![ValType::I32], vec![ValType::I32])?;
        
        // Method-style string conversions
        self.register_import_function("env", "integer.toString", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "number.toString", vec![ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "boolean.toString", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.toInteger", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.toNumber", vec![ValType::I32], vec![ValType::F64])?;
        self.register_import_function("env", "string.toBoolean", vec![ValType::I32], vec![ValType::I32])?;
        
        // String operations (require host functions)
        self.register_import_function("env", "string.toUpperCase", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.toLowerCase", vec![ValType::I32], vec![ValType::I32])?;
        
        Ok(())
    }

    fn register_file_operations(&mut self) -> Result<(), CompilerError> {
        if !self.include_runtime_imports {
            return Ok(());
        }

        // File operations
        let file_functions = [
            ("file_read", vec![ValType::I32], vec![ValType::I32]),
            ("file_write", vec![ValType::I32, ValType::I32], vec![ValType::I32]),
            ("file_append", vec![ValType::I32, ValType::I32], vec![ValType::I32]),
            ("file_exists", vec![ValType::I32], vec![ValType::I32]),
            ("file_delete", vec![ValType::I32], vec![ValType::I32]),
            ("file_size", vec![ValType::I32], vec![ValType::I32]),
            ("file_copy", vec![ValType::I32, ValType::I32], vec![ValType::I32]),
            ("file_move", vec![ValType::I32, ValType::I32], vec![ValType::I32]),
            ("dir_create", vec![ValType::I32], vec![ValType::I32]),
            ("dir_list", vec![ValType::I32], vec![ValType::I32]),
        ];

        for (name, params, returns) in &file_functions {
            let index = self.register_import_function("env", name, params.clone(), returns.clone())?;
            self.file_import_indices.insert(name.to_string(), index);
        }
        
        // File namespace operations (static method style)
        let file_read_index = self.register_import_function("env", "file.read", vec![ValType::I32], vec![ValType::I32])?;
        self.file_import_indices.insert("file.read".to_string(), file_read_index);

        let file_write_index = self.register_import_function("env", "file.write", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.file_import_indices.insert("file.write".to_string(), file_write_index);

        let file_append_index = self.register_import_function("env", "file.append", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.file_import_indices.insert("file.append".to_string(), file_append_index);

        let file_exists_index = self.register_import_function("env", "file.exists", vec![ValType::I32], vec![ValType::I32])?;
        self.file_import_indices.insert("file.exists".to_string(), file_exists_index);

        let file_delete_index = self.register_import_function("env", "file.delete", vec![ValType::I32], vec![ValType::I32])?;
        self.file_import_indices.insert("file.delete".to_string(), file_delete_index);
        
        Ok(())
    }

    fn register_http_operations(&mut self) -> Result<(), CompilerError> {
        if !self.include_runtime_imports {
            return Ok(());
        }

        // HTTP operations
        let http_functions = [
            ("http_get", vec![ValType::I32], vec![ValType::I32]),
            ("http_post", vec![ValType::I32, ValType::I32], vec![ValType::I32]),
            ("http_put", vec![ValType::I32, ValType::I32], vec![ValType::I32]),
            ("http_delete", vec![ValType::I32], vec![ValType::I32]),
            ("http_request", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32]),
            ("http_set_header", vec![ValType::I32, ValType::I32, ValType::I32], vec![]),
            ("http_get_status", vec![ValType::I32], vec![ValType::I32]),
            ("http_get_body", vec![ValType::I32], vec![ValType::I32]),
            ("http_get_headers", vec![ValType::I32], vec![ValType::I32]),
        ];

        for (name, params, returns) in &http_functions {
            let index = self.register_import_function("env", name, params.clone(), returns.clone())?;
            self.http_import_indices.insert(name.to_string(), index);
        }
        
        Ok(())
    }

    fn register_conditional_operations(&mut self) -> Result<(), CompilerError> {
        // CRITICAL FIX: Register comparison functions with BOTH naming schemes:
        // 1. Underscored names for backwards compatibility (compare_integer_equal)
        // 2. Dotted namespace names for modern calls (compare.integer.equal)

        // Comparison functions for integers - underscored names
        self.register_import_function("env", "compare_integer_equal", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare_integer_notEqual", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare_integer_lessThan", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare_integer_greaterThan", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare_integer_lessEqual", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare_integer_greaterEqual", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;

        // Comparison functions for integers - dotted namespace names
        self.register_import_function("env", "compare.integer.equal", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare.integer.notEqual", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare.integer.lessThan", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare.integer.greaterThan", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare.integer.lessEqual", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "compare.integer.greaterEqual", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;

        // Comparison functions for numbers - underscored names
        self.register_import_function("env", "compare_number_equal", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare_number_notEqual", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare_number_lessThan", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare_number_greaterThan", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare_number_lessEqual", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare_number_greaterEqual", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;

        // Comparison functions for numbers - dotted namespace names
        self.register_import_function("env", "compare.number.equal", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare.number.notEqual", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare.number.lessThan", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare.number.greaterThan", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare.number.lessEqual", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "compare.number.greaterEqual", vec![ValType::F64, ValType::F64], vec![ValType::I32])?;

        // Conditional functions - underscored names
        self.register_import_function("env", "conditional_integer", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "conditional_number", vec![ValType::I32, ValType::F64, ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "conditional_string", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "conditional_boolean", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;

        // Conditional functions - dotted namespace names
        self.register_import_function("env", "conditional.integer", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "conditional.number", vec![ValType::I32, ValType::F64, ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "conditional.string", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "conditional.boolean", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;

        // Logical functions - underscored names
        self.register_import_function("env", "logical_and", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "logical_or", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "logical_not", vec![ValType::I32], vec![ValType::I32])?;

        // Logical functions - dotted namespace names
        self.register_import_function("env", "logical.and", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "logical.or", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "logical.not", vec![ValType::I32], vec![ValType::I32])?;

        Ok(())
    }

    /// Generate built-in static method call
    pub fn generate_builtin_static_method_call(
        &mut self,
        class_name: &str,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        match class_name {
            "Math" => self.generate_math_static_method(method_name, arguments, instructions),
            "String" => self.generate_string_static_method(method_name, arguments, instructions),
            "Array" | "List" => self.generate_array_static_method(method_name, arguments, instructions),
            "Console" => self.generate_console_static_method(method_name, arguments, instructions),
            "Input" => self.generate_input_static_method(method_name, arguments, instructions),
            "File" => self.generate_file_static_method(method_name, arguments, instructions),
            "Http" => self.generate_http_static_method(method_name, arguments, instructions),
            
            // Namespace functions for conditionals and comparisons
            "compare.integer" => self.generate_compare_integer_method(method_name, arguments, instructions),
            "compare.number" => self.generate_compare_number_method(method_name, arguments, instructions),
            "conditional" => self.generate_conditional_method(method_name, arguments, instructions),
            "logical" => self.generate_logical_method(method_name, arguments, instructions),
            "input" => self.generate_input_static_method(method_name, arguments, instructions),
            
            _ => Err(CompilerError::codegen_error(
                format!("Unknown static method: {}::{}", class_name, method_name),
                None,
                None
            ))
        }
    }

    fn generate_math_static_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        // Call appropriate math function
        match method_name {
            "abs" => {
                let func_index = self.get_function_index_or_error("math_abs")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "sqrt" => {
                let func_index = self.get_function_index_or_error("math_sqrt")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "sin" => {
                let func_index = self.get_function_index_or_error("math_sin")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "cos" => {
                let func_index = self.get_function_index_or_error("math_cos")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "tan" => {
                let func_index = self.get_function_index_or_error("math_tan")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "pow" => {
                let func_index = self.get_function_index_or_error("math_pow")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "floor" => {
                let func_index = self.get_function_index_or_error("math_floor")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "ceil" => {
                let func_index = self.get_function_index_or_error("math_ceil")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "round" => {
                let func_index = self.get_function_index_or_error("math_round")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "max" => {
                let func_index = self.get_function_index_or_error("math_max")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "min" => {
                let func_index = self.get_function_index_or_error("math_min")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            // Math constants (no arguments needed)
            "pi" => {
                if !arguments.is_empty() {
                    return Err(CompilerError::codegen_error(
                        "Math.pi() takes no arguments".to_string(),
                        None,
                        None
                    ));
                }
                instructions.push(Instruction::F64Const(std::f64::consts::PI));
                Ok(WasmType::F64)
            },
            "e" => {
                if !arguments.is_empty() {
                    return Err(CompilerError::codegen_error(
                        "Math.e() takes no arguments".to_string(),
                        None,
                        None
                    ));
                }
                instructions.push(Instruction::F64Const(std::f64::consts::E));
                Ok(WasmType::F64)
            },
            "tau" => {
                if !arguments.is_empty() {
                    return Err(CompilerError::codegen_error(
                        "Math.tau() takes no arguments".to_string(),
                        None,
                        None
                    ));
                }
                instructions.push(Instruction::F64Const(std::f64::consts::TAU));
                Ok(WasmType::F64)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Unknown Math method: {}", method_name),
                None,
                None
            ))
        }
    }

    fn generate_string_static_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        match method_name {
            "concat" => {
                let func_index = self.get_function_index_or_error("string_concat")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "compare" => {
                let func_index = self.get_function_index_or_error("string_compare")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "length" => {
                let func_index = self.get_function_index_or_error("string_length")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "substring" => {
                let func_index = self.get_function_index_or_error("string_substring")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "indexOf" => {
                let func_index = self.get_function_index_or_error("string_index_of")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "contains" => {
                let func_index = self.get_function_index_or_error("string_contains")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "startsWith" => {
                let func_index = self.get_function_index_or_error("string_starts_with")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "endsWith" => {
                let func_index = self.get_function_index_or_error("string_ends_with")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "toUpperCase" => {
                let func_index = self.get_function_index_or_error("string_to_upper")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "toLowerCase" => {
                let func_index = self.get_function_index_or_error("string_to_lower")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "trim" => {
                let func_index = self.get_function_index_or_error("string_trim")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "replace" => {
                let func_index = self.get_function_index_or_error("string_replace")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "replaceAll" => {
                // For now, use the same replace function (implementation detail)
                let func_index = self.get_function_index_or_error("string_replace")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "split" => {
                let func_index = self.get_function_index_or_error("string_split")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "trimStart" | "leftTrim" => {
                // Use the regular trim function for now (implementation detail)
                let func_index = self.get_function_index_or_error("string_trim")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "trimEnd" | "rightTrim" => {
                // Use the regular trim function for now (implementation detail)
                let func_index = self.get_function_index_or_error("string_trim")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "isEmpty" => {
                // Check if string length is 0
                let func_index = self.get_function_index_or_error("string_length")?;
                instructions.push(Instruction::Call(func_index));
                instructions.push(Instruction::I32Const(0));
                instructions.push(Instruction::I32Eq);
                Ok(WasmType::I32)
            },
            "isNotEmpty" => {
                // Check if string length is not 0
                let func_index = self.get_function_index_or_error("string_length")?;
                instructions.push(Instruction::Call(func_index));
                instructions.push(Instruction::I32Const(0));
                instructions.push(Instruction::I32Ne);
                Ok(WasmType::I32)
            },
            "join" => {
                // For now, use a simple concatenation approach
                let func_index = self.get_function_index_or_error("string_concat")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "format" => {
                // For now, just return the input string as-is (identity function)
                // This is a placeholder implementation
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Unknown String static method: {}", method_name),
                None,
                None
            ))
        }
    }

    fn generate_array_static_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        match method_name {
            "create" => {
                // Create new array with specified size
                // For now, return a null pointer
                instructions.push(Instruction::I32Const(0));
                Ok(WasmType::I32)
            },
            "size" | "length" => {
                // Get array/list length
                let func_index = self.get_function_index_or_error("array_length")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "isEmpty" => {
                // Check if array is empty (length == 0)
                let func_index = self.get_function_index_or_error("array_length")?;
                instructions.push(Instruction::Call(func_index));
                instructions.push(Instruction::I32Const(0));
                instructions.push(Instruction::I32Eq);
                Ok(WasmType::I32)
            },
            "isNotEmpty" => {
                // Check if array is not empty (length != 0)
                let func_index = self.get_function_index_or_error("array_length")?;
                instructions.push(Instruction::Call(func_index));
                instructions.push(Instruction::I32Const(0));
                instructions.push(Instruction::I32Ne);
                Ok(WasmType::I32)
            },
            "get" => {
                // Get element at index
                let func_index = self.get_function_index_or_error("array_get")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "set" => {
                // Set element at index
                let func_index = self.get_function_index_or_error("array_set")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "push" => {
                // Add element to end of array
                let func_index = self.get_function_index_or_error("array_push")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "pop" => {
                // Remove and return last element
                let func_index = self.get_function_index_or_error("array_pop")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "contains" => {
                // Check if array contains element
                let func_index = self.get_function_index_or_error("array_contains")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "remove" => {
                // Remove element from array (supports both index and no-args versions)
                if arguments.len() == 1 {
                    // Only array argument - remove last element (like pop)
                    let func_index = self.get_function_index_or_error("array_pop")?;
                    instructions.push(Instruction::Call(func_index));
                } else {
                    // Array + index argument - remove at index
                    let func_index = self.get_function_index_or_error("array_pop")?; // Use pop for now
                    instructions.push(Instruction::Call(func_index));
                }
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Unknown Array static method: {}", method_name),
                None,
                None
            ))
        }
    }

    fn generate_console_static_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        match method_name {
            "log" => {
                let func_index = self.get_function_index_or_error("console_log")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32) // Returns void, but we need a type
            },
            "error" => {
                let func_index = self.get_function_index_or_error("console_error")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "warn" => {
                let func_index = self.get_function_index_or_error("console_warn")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "input" => {
                let func_index = self.get_function_index_or_error("console_input")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Unknown Console static method: {}", method_name),
                None,
                None
            ))
        }
    }

    // OLD CODE PATH DELETED - Input static methods now handled by MIR codegen
    // with single-parameter (ptr only) signature per system specification
    fn generate_input_static_method(
        &mut self,
        method_name: &str,
        _arguments: &[crate::ast::Expression],
        _instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        Err(CompilerError::codegen_error(
            format!("Input static method '{}' should be handled by MIR codegen, not legacy AST codegen", method_name),
            None,
            None
        ))
    }

    fn generate_file_static_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        if !self.include_runtime_imports {
            return Err(CompilerError::codegen_error(
                "File operations not available in minimal mode".to_string(),
                None,
                None
            ));
        }

        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        match method_name {
            "read" => {
                let func_index = *self.file_import_indices.get("file_read")
                    .ok_or_else(|| CompilerError::codegen_error("File.read not registered".to_string(), None, None))?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "write" => {
                let func_index = *self.file_import_indices.get("file_write")
                    .ok_or_else(|| CompilerError::codegen_error("File.write not registered".to_string(), None, None))?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "exists" => {
                let func_index = *self.file_import_indices.get("file_exists")
                    .ok_or_else(|| CompilerError::codegen_error("File.exists not registered".to_string(), None, None))?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Unknown File static method: {}", method_name),
                None,
                None
            ))
        }
    }

    fn generate_http_static_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        if !self.include_runtime_imports {
            return Err(CompilerError::codegen_error(
                "HTTP operations not available in minimal mode".to_string(),
                None,
                None
            ));
        }

        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        match method_name {
            "get" => {
                let func_index = *self.http_import_indices.get("http_get")
                    .ok_or_else(|| CompilerError::codegen_error("Http.get not registered".to_string(), None, None))?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "post" => {
                let func_index = *self.http_import_indices.get("http_post")
                    .ok_or_else(|| CompilerError::codegen_error("Http.post not registered".to_string(), None, None))?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Unknown Http static method: {}", method_name),
                None,
                None
            ))
        }
    }

    /// Generate compare.integer namespace functions
    fn generate_compare_integer_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        match method_name {
            "equal" => {
                let import_func_name = "compare_integer_equal";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32) // Boolean result
            },
            "notEqual" => {
                let import_func_name = "compare_integer_notEqual";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "lessThan" => {
                let import_func_name = "compare_integer_lessThan";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "greaterThan" => {
                let import_func_name = "compare_integer_greaterThan";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "lessEqual" => {
                let import_func_name = "compare_integer_lessEqual";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "greaterEqual" => {
                let import_func_name = "compare_integer_greaterEqual";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Unknown compare.integer method: {}", method_name),
                None,
                None
            ))
        }
    }

    /// Generate compare.number namespace functions
    fn generate_compare_number_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        let import_func_name = format!("compare_number_{}", method_name);
        let func_index = self.get_function_index_or_error(&import_func_name)?;
        instructions.push(Instruction::Call(func_index));
        Ok(WasmType::I32) // Boolean result
    }

    /// Generate conditional namespace functions
    fn generate_conditional_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        match method_name {
            "integer" => {
                let import_func_name = "conditional_integer";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "number" => {
                let import_func_name = "conditional_number";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::F64)
            },
            "string" => {
                let import_func_name = "conditional_string";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32) // String pointer
            },
            "boolean" => {
                let import_func_name = "conditional_boolean";
                let func_index = self.get_function_index_or_error(import_func_name)?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            _ => Err(CompilerError::codegen_error(
                format!("Unknown conditional method: {}", method_name),
                None,
                None
            ))
        }
    }

    /// Generate logical namespace functions
    fn generate_logical_method(
        &mut self,
        method_name: &str,
        arguments: &[crate::ast::Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate arguments
        for arg in arguments {
            self.generate_expression(arg, instructions)?;
        }

        let import_func_name = format!("logical_{}", method_name);
        let func_index = self.get_function_index_or_error(&import_func_name)?;
        instructions.push(Instruction::Call(func_index));
        Ok(WasmType::I32) // Boolean result
    }
}