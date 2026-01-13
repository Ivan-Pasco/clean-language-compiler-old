//! Built-in function generation for Clean Language WebAssembly compiler.

use wasm_encoder::{Instruction, ValType};
use crate::error::CompilerError;
use crate::types::WasmType;
use super::CodeGenerator;
use super::native_stdlib;

impl CodeGenerator {
    /// Register all built-in standard library functions
    pub fn register_stdlib_functions(&mut self) -> Result<(), CompilerError> {
        // COMPLETE STANDARD LIBRARY: Enable all functions for full Clean Language support
        self.register_memory_operations()?; // NATIVE: malloc, memcpy for standalone WASM
        self.register_math_operations()?; // Re-enabled for power operator support
        // REMOVED: self.register_console_operations() creates obsolete wrapper functions
        // Console imports (print, printl, input, input_integer, etc.) are registered separately in new()
        self.register_type_conversion_operations()?; // Needed for toString() calls
        self.register_string_operations()?; // Critical for string methods like length()
        self.register_list_operations()?; // Critical for list methods like size()
        self.register_file_operations()?; // Needed for file I/O operations
        self.register_http_operations()?; // Needed for HTTP operations
        self.register_conditional_operations()?; // Needed for conditional operations
        
        // Register StandardLibrary functions including matrix operations and JSON
        // BOOK: json-module - StandardLibrary includes JsonClass for JSON operations
        self.register_standard_library_functions()?;
        
        Ok(())
    }
    
    /// Register all StandardLibrary functions including matrix operations
    fn register_standard_library_functions(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::StandardLibrary;
        
        // Initialize StandardLibrary and register its functions
        let mut stdlib = StandardLibrary::new();

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

    /// Register native memory allocation operations for standalone WASM execution
    pub fn register_memory_operations(&mut self) -> Result<(), CompilerError> {
        // NATIVE: malloc - bump allocator for memory allocation
        // Uses global 0 (HEAP_PTR_GLOBAL) as heap pointer
        // Parameters: size (i32)
        // Returns: pointer (i32) to allocated memory
        let malloc_instructions = native_stdlib::memory::gen_malloc();
        self.register_function(
            "__malloc",
            vec![ValType::I32],
            vec![ValType::I32],
            &malloc_instructions,
        )?;
        // Create alias for dot-notation access
        if let Some(malloc_idx) = self.get_function_index("__malloc") {
            self.add_function_alias("malloc", malloc_idx);
            self.add_function_alias("memory.alloc", malloc_idx);
        }

        // NATIVE: memcpy - byte-by-byte memory copy
        // Parameters: dest (i32), src (i32), len (i32)
        // Returns: void
        let memcpy_instructions = native_stdlib::memory::gen_memcpy();
        self.register_function(
            "__memcpy",
            vec![ValType::I32, ValType::I32, ValType::I32],
            vec![], // void return
            &memcpy_instructions,
        )?;
        // Create alias for dot-notation access
        if let Some(memcpy_idx) = self.get_function_index("__memcpy") {
            self.add_function_alias("memcpy", memcpy_idx);
            self.add_function_alias("memory.copy", memcpy_idx);
        }

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

        // Math.sqrt - NATIVE: uses WASM F64Sqrt instruction
        let sqrt_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::F64Sqrt,
        ];
        self.register_function("math_sqrt", vec![ValType::F64], vec![ValType::F64], &sqrt_instructions)?;
        
        // Math.sin - trigonometric function
        self.register_import_function("env", "math_sin", vec![ValType::F64], vec![ValType::F64])?;
        
        // Math.cos - trigonometric function
        self.register_import_function("env", "math_cos", vec![ValType::F64], vec![ValType::F64])?;
        
        // Math.tan - trigonometric function
        self.register_import_function("env", "math_tan", vec![ValType::F64], vec![ValType::F64])?;

        // Inverse trigonometric functions - host imports
        self.register_import_function("env", "math_asin", vec![ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "math_acos", vec![ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "math_atan", vec![ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "math_atan2", vec![ValType::F64, ValType::F64], vec![ValType::F64])?;

        // Hyperbolic functions - host imports
        self.register_import_function("env", "math_sinh", vec![ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "math_cosh", vec![ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "math_tanh", vec![ValType::F64], vec![ValType::F64])?;

        // Logarithmic and exponential functions - host imports
        self.register_import_function("env", "math_ln", vec![ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "math_log10", vec![ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "math_log2", vec![ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "math_exp", vec![ValType::F64], vec![ValType::F64])?;
        self.register_import_function("env", "math_exp2", vec![ValType::F64], vec![ValType::F64])?;

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
        // String length - native WASM implementation (no host function needed)
        let string_length_instructions = native_stdlib::gen_string_length();
        self.register_function("string_length", vec![ValType::I32], vec![ValType::I32], &string_length_instructions)?;
        // FIXED: string.concat takes 2 args (ptr1, ptr2) - each is a length-prefixed string pointer
        self.register_import_function("env", "string.concat", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        // Native substring implementation (replaces host import)
        let malloc_idx = self.get_function_index("__malloc")
            .ok_or_else(|| CompilerError::new("malloc not registered before substring"))?;
        let substring_instructions = native_stdlib::string_ops::gen_substring(malloc_idx);
        self.register_function_with_locals(
            "string_substring",
            &[WasmType::I32, WasmType::I32, WasmType::I32],  // str_ptr, start, end
            Some(WasmType::I32),  // returns new_ptr
            &[WasmType::I32, WasmType::I32, WasmType::I32],  // new_len, new_ptr, i
            &substring_instructions,
        )?;
        self.register_import_function("env", "string_index_of", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        // 3-arg indexOf with startIndex for searching from a specific position
        self.register_import_function("env", "string_index_of_from", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        // 3-arg lastIndexOf with startIndex for searching from a specific position (backwards)
        self.register_import_function("env", "string_last_index_of_from", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_to_upper", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_to_lower", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_trim", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_replace", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_split", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        // Register string.split to map directly to string_split runtime function
        self.register_import_function("env", "string.split", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        // Native string comparison functions
        let starts_with_instructions = native_stdlib::string_ops::gen_starts_with();
        self.register_function("string_starts_with", vec![ValType::I32, ValType::I32], vec![ValType::I32], &starts_with_instructions)?;

        let ends_with_instructions = native_stdlib::string_ops::gen_ends_with();
        self.register_function("string_ends_with", vec![ValType::I32, ValType::I32], vec![ValType::I32], &ends_with_instructions)?;

        let index_of_instructions = native_stdlib::string_ops::gen_index_of();
        let index_of_idx = self.register_function("string_index_of", vec![ValType::I32, ValType::I32], vec![ValType::I32], &index_of_instructions)?;

        // string_contains uses string_index_of internally
        let contains_instructions = native_stdlib::string_ops::gen_contains(index_of_idx);
        self.register_function("string_contains", vec![ValType::I32, ValType::I32], vec![ValType::I32], &contains_instructions)?;

        // Native lastIndexOf implementation
        let last_index_of_instructions = native_stdlib::string_ops::gen_last_index_of();
        let last_index_of_idx = self.register_function_with_locals(
            "string_last_index_of",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // 6 extra locals
            &last_index_of_instructions,
        )?;
        tracing::debug!(
            function_index = last_index_of_idx,
            "Registered native string_last_index_of function"
        );

        self.register_import_function("env", "string_compare", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;

        // String namespace functions (static method style)
        // These are aliases to the native implementations registered above
        if let Some(string_length_idx) = self.get_function_index("string_length") {
            self.add_function_alias("string.length", string_length_idx);
        }
        if let Some(contains_idx) = self.get_function_index("string_contains") {
            self.add_function_alias("string.contains", contains_idx);
        }
        if let Some(index_of_idx) = self.get_function_index("string_index_of") {
            self.add_function_alias("string.indexOf", index_of_idx);
        }
        // Register string.indexOfFrom alias for 3-arg indexOf (text, needle, startIndex)
        if let Some(index_of_from_idx) = self.get_function_index("string_index_of_from") {
            self.add_function_alias("string.indexOfFrom", index_of_from_idx);
        }
        if let Some(last_index_of_idx) = self.get_function_index("string_last_index_of") {
            self.add_function_alias("string.lastIndexOf", last_index_of_idx);
        }
        // Register string.lastIndexOfFrom alias for 3-arg lastIndexOf (text, needle, startIndex)
        if let Some(last_index_of_from_idx) = self.get_function_index("string_last_index_of_from") {
            self.add_function_alias("string.lastIndexOfFrom", last_index_of_from_idx);
        }
        if let Some(starts_with_idx) = self.get_function_index("string_starts_with") {
            self.add_function_alias("string.startsWith", starts_with_idx);
        }
        if let Some(ends_with_idx) = self.get_function_index("string_ends_with") {
            self.add_function_alias("string.endsWith", ends_with_idx);
        }
        // Trim functions - use underscore naming to match host functions
        let trim_start_idx = self.register_import_function("env", "string_trim_start", vec![ValType::I32], vec![ValType::I32])?;
        let trim_end_idx = self.register_import_function("env", "string_trim_end", vec![ValType::I32], vec![ValType::I32])?;
        // Add aliases for dot notation static method calls (string.trimStart, etc.)
        if let Some(trim_idx) = self.get_function_index("string_trim") {
            self.add_function_alias("string.trim", trim_idx);
        }
        self.add_function_alias("string.trimStart", trim_start_idx);
        self.add_function_alias("string.trimEnd", trim_end_idx);
        self.register_import_function("env", "string.replace", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32])?;
        // Use native substring implementation (alias to string_substring)
        if let Some(substring_idx) = self.get_function_index("string_substring") {
            self.add_function_alias("string.substring", substring_idx);
        }
        self.register_import_function("env", "string.toUpperCase", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string.toLowerCase", vec![ValType::I32], vec![ValType::I32])?;

        // Also register the underscore versions for method calls
        self.register_import_function("env", "string_toUpperCase", vec![ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "string_toLowerCase", vec![ValType::I32], vec![ValType::I32])?;

        Ok(())
    }

    fn register_list_operations(&mut self) -> Result<(), CompilerError> {
        // Array/List length - native WASM implementation (no host function needed)
        let list_length_instructions = native_stdlib::gen_list_length();
        self.register_function("array_length", vec![ValType::I32], vec![ValType::I32], &list_length_instructions)?;

        // Basic array operations - these require host functions for memory management
        self.register_import_function("env", "array_get", vec![ValType::I32, ValType::I32], vec![ValType::I32])?;
        self.register_import_function("env", "array_set", vec![ValType::I32, ValType::I32, ValType::I32], vec![])?;
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
        // list.length uses same native implementation as array_length (alias)
        if let Some(array_length_idx) = self.get_function_index("array_length") {
            self.add_function_alias("list.length", array_length_idx);
        }
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
        
        // String conversions - NATIVE implementations for int/bool, host for float
        // NOTE: int_to_string, bool_to_string, string_to_int are now NATIVE (registered in mod.rs)
        // with aliases: integer.toString, boolean.toString, string.toInteger
        self.register_import_function("env", "float_to_string", vec![ValType::F64], vec![ValType::I32])?;
        self.register_import_function("env", "string_to_float", vec![ValType::I32], vec![ValType::F64])?;
        self.register_import_function("env", "string_to_bool", vec![ValType::I32], vec![ValType::I32])?;

        // Method-style string conversions - only float/number need host imports
        self.register_import_function("env", "number.toString", vec![ValType::F64], vec![ValType::I32])?;
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
        // NATIVE WASM IMPLEMENTATIONS: All comparison, conditional, and logical operations
        // are now implemented using native WASM instructions instead of host imports.

        // ========== INTEGER COMPARISON FUNCTIONS (NATIVE I32 ops) ==========

        // compare_integer_equal: (a, b) -> a == b
        let i32_eq_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Eq,
        ];
        self.register_function("compare_integer_equal", vec![ValType::I32, ValType::I32], vec![ValType::I32], &i32_eq_instructions)?;
        self.add_function_alias("compare.integer.equal", self.get_function_index("compare_integer_equal").unwrap());

        // compare_integer_notEqual: (a, b) -> a != b
        let i32_ne_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Ne,
        ];
        self.register_function("compare_integer_notEqual", vec![ValType::I32, ValType::I32], vec![ValType::I32], &i32_ne_instructions)?;
        self.add_function_alias("compare.integer.notEqual", self.get_function_index("compare_integer_notEqual").unwrap());

        // compare_integer_lessThan: (a, b) -> a < b (signed)
        let i32_lt_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32LtS,
        ];
        self.register_function("compare_integer_lessThan", vec![ValType::I32, ValType::I32], vec![ValType::I32], &i32_lt_instructions)?;
        self.add_function_alias("compare.integer.lessThan", self.get_function_index("compare_integer_lessThan").unwrap());

        // compare_integer_greaterThan: (a, b) -> a > b (signed)
        let i32_gt_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32GtS,
        ];
        self.register_function("compare_integer_greaterThan", vec![ValType::I32, ValType::I32], vec![ValType::I32], &i32_gt_instructions)?;
        self.add_function_alias("compare.integer.greaterThan", self.get_function_index("compare_integer_greaterThan").unwrap());

        // compare_integer_lessEqual: (a, b) -> a <= b (signed)
        let i32_le_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32LeS,
        ];
        self.register_function("compare_integer_lessEqual", vec![ValType::I32, ValType::I32], vec![ValType::I32], &i32_le_instructions)?;
        self.add_function_alias("compare.integer.lessEqual", self.get_function_index("compare_integer_lessEqual").unwrap());

        // compare_integer_greaterEqual: (a, b) -> a >= b (signed)
        let i32_ge_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32GeS,
        ];
        self.register_function("compare_integer_greaterEqual", vec![ValType::I32, ValType::I32], vec![ValType::I32], &i32_ge_instructions)?;
        self.add_function_alias("compare.integer.greaterEqual", self.get_function_index("compare_integer_greaterEqual").unwrap());

        // ========== NUMBER (F64) COMPARISON FUNCTIONS (NATIVE F64 ops) ==========

        // compare_number_equal: (a, b) -> a == b
        let f64_eq_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Eq,
        ];
        self.register_function("compare_number_equal", vec![ValType::F64, ValType::F64], vec![ValType::I32], &f64_eq_instructions)?;
        self.add_function_alias("compare.number.equal", self.get_function_index("compare_number_equal").unwrap());

        // compare_number_notEqual: (a, b) -> a != b
        let f64_ne_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Ne,
        ];
        self.register_function("compare_number_notEqual", vec![ValType::F64, ValType::F64], vec![ValType::I32], &f64_ne_instructions)?;
        self.add_function_alias("compare.number.notEqual", self.get_function_index("compare_number_notEqual").unwrap());

        // compare_number_lessThan: (a, b) -> a < b
        let f64_lt_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Lt,
        ];
        self.register_function("compare_number_lessThan", vec![ValType::F64, ValType::F64], vec![ValType::I32], &f64_lt_instructions)?;
        self.add_function_alias("compare.number.lessThan", self.get_function_index("compare_number_lessThan").unwrap());

        // compare_number_greaterThan: (a, b) -> a > b
        let f64_gt_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Gt,
        ];
        self.register_function("compare_number_greaterThan", vec![ValType::F64, ValType::F64], vec![ValType::I32], &f64_gt_instructions)?;
        self.add_function_alias("compare.number.greaterThan", self.get_function_index("compare_number_greaterThan").unwrap());

        // compare_number_lessEqual: (a, b) -> a <= b
        let f64_le_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Le,
        ];
        self.register_function("compare_number_lessEqual", vec![ValType::F64, ValType::F64], vec![ValType::I32], &f64_le_instructions)?;
        self.add_function_alias("compare.number.lessEqual", self.get_function_index("compare_number_lessEqual").unwrap());

        // compare_number_greaterEqual: (a, b) -> a >= b
        let f64_ge_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Ge,
        ];
        self.register_function("compare_number_greaterEqual", vec![ValType::F64, ValType::F64], vec![ValType::I32], &f64_ge_instructions)?;
        self.add_function_alias("compare.number.greaterEqual", self.get_function_index("compare_number_greaterEqual").unwrap());

        // ========== CONDITIONAL FUNCTIONS (NATIVE select instruction) ==========

        // conditional_integer: (condition, then_val, else_val) -> result
        // WASM select: (then, else, condition) -> result
        let cond_i32_instructions = vec![
            Instruction::LocalGet(1), // then_val
            Instruction::LocalGet(2), // else_val
            Instruction::LocalGet(0), // condition
            Instruction::Select,
        ];
        self.register_function("conditional_integer", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32], &cond_i32_instructions)?;
        self.add_function_alias("conditional.integer", self.get_function_index("conditional_integer").unwrap());

        // conditional_string: (condition, then_val, else_val) -> result (pointers are i32)
        self.register_function("conditional_string", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32], &cond_i32_instructions)?;
        self.add_function_alias("conditional.string", self.get_function_index("conditional_string").unwrap());

        // conditional_boolean: (condition, then_val, else_val) -> result (booleans are i32)
        self.register_function("conditional_boolean", vec![ValType::I32, ValType::I32, ValType::I32], vec![ValType::I32], &cond_i32_instructions)?;
        self.add_function_alias("conditional.boolean", self.get_function_index("conditional_boolean").unwrap());

        // conditional_number: (condition, then_val, else_val) -> result (f64 values)
        // Note: select works for f64 too, condition still i32
        let cond_f64_instructions = vec![
            Instruction::LocalGet(1), // then_val (f64)
            Instruction::LocalGet(2), // else_val (f64)
            Instruction::LocalGet(0), // condition (i32)
            Instruction::Select,
        ];
        self.register_function("conditional_number", vec![ValType::I32, ValType::F64, ValType::F64], vec![ValType::F64], &cond_f64_instructions)?;
        self.add_function_alias("conditional.number", self.get_function_index("conditional_number").unwrap());

        // ========== LOGICAL FUNCTIONS (NATIVE I32 ops) ==========

        // logical_and: (a, b) -> a && b (bitwise AND for booleans)
        let and_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32And,
        ];
        self.register_function("logical_and", vec![ValType::I32, ValType::I32], vec![ValType::I32], &and_instructions)?;
        self.add_function_alias("logical.and", self.get_function_index("logical_and").unwrap());

        // logical_or: (a, b) -> a || b (bitwise OR for booleans)
        let or_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Or,
        ];
        self.register_function("logical_or", vec![ValType::I32, ValType::I32], vec![ValType::I32], &or_instructions)?;
        self.add_function_alias("logical.or", self.get_function_index("logical_or").unwrap());

        // logical_not: (a) -> !a (test if zero)
        let not_instructions = vec![
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
        ];
        self.register_function("logical_not", vec![ValType::I32], vec![ValType::I32], &not_instructions)?;
        self.add_function_alias("logical.not", self.get_function_index("logical_not").unwrap());

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
                let func_index = self.get_function_index_or_error("string.concat")?;
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
                let func_index = self.get_function_index_or_error("string_trim_start")?;
                instructions.push(Instruction::Call(func_index));
                Ok(WasmType::I32)
            },
            "trimEnd" | "rightTrim" => {
                let func_index = self.get_function_index_or_error("string_trim_end")?;
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
                let func_index = self.get_function_index_or_error("string.concat")?;
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