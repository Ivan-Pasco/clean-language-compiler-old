//! Import and function registration for the `CodeGenerator`.
//! This module contains all `register_*` functions that set up WASM imports.

use super::native_stdlib;
use crate::error::CompilerError;
use crate::types::WasmType;
use tracing::debug;

impl super::CodeGenerator {
    /// Register file operation functions using WASM instructions from FileClass
    /// Only registers specification-compliant functions: file.read, file.write, file.append, file.exists, file.delete
    /// NOTE: File imports are now registered in register_import_functions_only() which is called first
    pub(crate) fn register_file_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::file_class::FileClass;

        // File imports are already registered by register_import_functions_only()
        // Just register the wrapper functions (file.read, file.write, etc.)
        let file_class = FileClass::new();
        file_class.register_functions(self)?;

        Ok(())
    }

    /// Register validator operation functions (validator.create, validator.ok, validator.isOk, etc.)
    pub fn register_validator_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::memory::MemoryManager;
        use crate::stdlib::validator::ValidatorManager;
        use std::cell::RefCell;
        use std::rc::Rc;

        // Create a MemoryManager and ValidatorManager instance.
        // Resolve mem_alloc index dynamically so the validator is resilient to
        // import ordering changes (e.g. when input_* imports are gated out).
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let mem_alloc_idx = self
            .get_function_index("mem_alloc")
            .unwrap_or(crate::stdlib::validator::DEFAULT_MEM_ALLOC_CALL_INDEX);
        let validator_manager =
            ValidatorManager::new_with_mem_alloc_idx(memory_manager, mem_alloc_idx);
        validator_manager.register_functions(self)?;

        Ok(())
    }

    /// Register JSON operations for parsing and stringifying JSON
    /// BOOK: json-module - pure WASM JSON implementation
    pub fn register_json_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::JsonClass;

        tracing::debug!(
            function_count = self.function_count,
            "Registering JSON operations"
        );

        let json_class = JsonClass::new();
        json_class.register_functions(self)?;

        tracing::debug!(
            function_count = self.function_count,
            "JSON operations registered successfully"
        );

        Ok(())
    }

    /// Register native memory allocation operations for standalone WASM execution
    /// These functions run entirely in WASM without host imports
    pub fn register_memory_operations(&mut self) -> Result<(), CompilerError> {
        // NATIVE: malloc - bump allocator for memory allocation
        // Uses global 0 (HEAP_PTR_GLOBAL) as heap pointer
        // Parameters: size (i32)
        // Returns: pointer (i32) to allocated memory
        let malloc_instructions = native_stdlib::memory::gen_malloc();
        let malloc_idx = self.register_function(
            "__malloc",
            &[WasmType::I32],
            Some(WasmType::I32),
            &malloc_instructions,
        )?;
        // Create alias for internal use
        self.add_function_alias("malloc", malloc_idx);

        // NATIVE: memcpy - byte-by-byte memory copy
        // Parameters: dest (i32), src (i32), len (i32)
        // Returns: void
        let memcpy_instructions = native_stdlib::memory::gen_memcpy();
        self.register_function(
            "__memcpy",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            None, // void return
            &memcpy_instructions,
        )?;
        // Create alias for internal use
        if let Some(memcpy_idx) = self.get_function_index("__memcpy") {
            self.add_function_alias("memcpy", memcpy_idx);
        }

        // NATIVE: string_concat - concatenates two strings using malloc
        // Parameters: str1_ptr (i32), str2_ptr (i32)
        // Returns: pointer (i32) to new concatenated string
        let concat_instructions = native_stdlib::string_ops::gen_concat(malloc_idx);
        self.register_function(
            "__string_concat",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &concat_instructions,
        )?;
        // NOTE: Do NOT alias "string.concat" here - that would overwrite the host import
        // registered separately. The host import is preferred because it
        // handles memory allocation more reliably in the runtime environment.

        // NATIVE: string_index_of - finds substring in string
        // Parameters: str_ptr (i32), search_ptr (i32)
        // Returns: index (i32) or -1 if not found
        let string_index_of_instructions = native_stdlib::string_ops::gen_index_of();
        let string_index_of_idx = self.register_function(
            "__string_index_of",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &string_index_of_instructions,
        )?;
        self.add_function_alias("string.indexOf", string_index_of_idx);

        // NATIVE: string_index_of_from - finds substring in string starting from index
        // Parameters: str_ptr (i32), search_ptr (i32), start_index (i32)
        // Returns: index (i32) or -1 if not found
        let string_index_of_from_instructions = native_stdlib::string_ops::gen_index_of_from();
        let string_index_of_from_idx = self.register_function_with_locals(
            "__string_index_of_from",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // search_len
                WasmType::I32, // i
                WasmType::I32, // j
                WasmType::I32, // match
            ],
            &string_index_of_from_instructions,
        )?;
        self.add_function_alias("string.indexOfFrom", string_index_of_from_idx);

        // NATIVE: string_contains - checks if string contains substring
        // Parameters: str_ptr (i32), search_ptr (i32)
        // Returns: boolean (i32)
        let string_contains_instructions =
            native_stdlib::string_ops::gen_contains(string_index_of_idx);
        let string_contains_idx = self.register_function(
            "__string_contains",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &string_contains_instructions,
        )?;
        self.add_function_alias("string.contains", string_contains_idx);

        // NATIVE: string_last_index_of - finds last occurrence of substring in string
        // Parameters: str_ptr (i32), search_ptr (i32)
        // Returns: index (i32) or -1 if not found
        let string_last_index_of_instructions = native_stdlib::string_ops::gen_last_index_of();
        let string_last_index_of_idx = self.register_function_with_locals(
            "__string_last_index_of",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // 6 extra locals
            &string_last_index_of_instructions,
        )?;
        self.add_function_alias("string.lastIndexOf", string_last_index_of_idx);

        // NATIVE: string_last_index_of_from - finds last occurrence of substring starting from index
        // Parameters: str_ptr (i32), search_ptr (i32), start_index (i32)
        // Returns: index (i32) or -1 if not found
        let string_last_index_of_from_instructions =
            native_stdlib::string_ops::gen_last_index_of_from();
        let string_last_index_of_from_idx = self.register_function_with_locals(
            "__string_last_index_of_from",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // search_len
                WasmType::I32, // i
                WasmType::I32, // j
                WasmType::I32, // match
                WasmType::I32, // result
            ],
            &string_last_index_of_from_instructions,
        )?;
        self.add_function_alias("string.lastIndexOfFrom", string_last_index_of_from_idx);

        // NATIVE: string_starts_with - checks if string starts with prefix
        // Parameters: str_ptr (i32), prefix_ptr (i32)
        // Returns: boolean (i32)
        let string_starts_with_instructions = native_stdlib::string_ops::gen_starts_with();
        let string_starts_with_idx = self.register_function_with_locals(
            "__string_starts_with",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32, WasmType::I32], // 3 extra locals
            &string_starts_with_instructions,
        )?;
        self.add_function_alias("string.startsWith", string_starts_with_idx);

        // NATIVE: string_ends_with - checks if string ends with suffix
        // Parameters: str_ptr (i32), suffix_ptr (i32)
        // Returns: boolean (i32)
        let string_ends_with_instructions = native_stdlib::string_ops::gen_ends_with();
        let string_ends_with_idx = self.register_function_with_locals(
            "__string_ends_with",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // 4 extra locals
            &string_ends_with_instructions,
        )?;
        self.add_function_alias("string.endsWith", string_ends_with_idx);

        // NATIVE: string_substring - extracts a substring from a string
        // Parameters: str_ptr (i32), start (i32), end (i32)
        // Returns: pointer (i32) to new string
        let string_substring_instructions = native_stdlib::string_ops::gen_substring(malloc_idx);
        let string_substring_idx = self.register_function_with_locals(
            "__string_substring",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32, WasmType::I32], // 3 extra locals: new_len, new_ptr, i
            &string_substring_instructions,
        )?;
        self.add_function_alias("string.substring", string_substring_idx);

        // NATIVE: int_to_string - converts integer to string using malloc
        // Parameters: value (i32)
        // Returns: pointer (i32) to new string
        let int_to_string_instructions =
            native_stdlib::type_conversions::gen_int_to_string(malloc_idx);
        self.register_function(
            "__int_to_string",
            &[WasmType::I32],
            Some(WasmType::I32),
            &int_to_string_instructions,
        )?;
        // Create aliases - int_to_string is the internal name used throughout the pipeline
        if let Some(its_idx) = self.get_function_index("__int_to_string") {
            self.add_function_alias("int_to_string", its_idx);
            self.add_function_alias("integer.toString", its_idx);
        }

        // NATIVE: bool_to_string - returns pointer to pre-allocated "true" or "false" string
        // Parameters: bool_value (i32, 0 or non-zero)
        // Returns: pointer (i32) to "true" or "false" string
        let true_ptr = self.add_string_to_pool("true");
        let false_ptr = self.add_string_to_pool("false");
        let bool_to_string_instructions =
            native_stdlib::type_conversions::gen_bool_to_string(true_ptr, false_ptr);
        self.register_function(
            "__bool_to_string",
            &[WasmType::I32],
            Some(WasmType::I32),
            &bool_to_string_instructions,
        )?;
        // Create aliases - bool_to_string is the internal name used throughout the pipeline
        if let Some(bts_idx) = self.get_function_index("__bool_to_string") {
            self.add_function_alias("bool_to_string", bts_idx);
            self.add_function_alias("boolean.toString", bts_idx);
        }

        // NATIVE: string_to_int - parses decimal string to integer
        // Parameters: str_ptr (i32)
        // Returns: integer (i32)
        let string_to_int_instructions = native_stdlib::type_conversions::gen_string_to_int();
        self.register_function(
            "__string_to_int",
            &[WasmType::I32],
            Some(WasmType::I32),
            &string_to_int_instructions,
        )?;
        // Create aliases - string_to_int is the internal name used throughout the pipeline
        if let Some(sti_idx) = self.get_function_index("__string_to_int") {
            self.add_function_alias("string_to_int", sti_idx);
            self.add_function_alias("string.toInteger", sti_idx);
        }

        // NATIVE: list_get_i32 - get element from i32 list
        // Parameters: list_ptr (i32), index (i32)
        // Returns: element (i32)
        let list_get_i32_instructions = native_stdlib::list_ops::gen_get_i32();
        self.register_function(
            "__list_get_i32",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_get_i32_instructions,
        )?;
        if let Some(lg_idx) = self.get_function_index("__list_get_i32") {
            self.add_function_alias("list.get", lg_idx);
        }

        // NATIVE: list_set_i32 - set element in i32 list
        // Parameters: list_ptr (i32), index (i32), value (i32)
        // Returns: list_ptr (i32) for chaining
        let list_set_i32_instructions = native_stdlib::list_ops::gen_set_i32();
        self.register_function(
            "__list_set_i32",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_set_i32_instructions,
        )?;
        if let Some(ls_idx) = self.get_function_index("__list_set_i32") {
            self.add_function_alias("list.set", ls_idx);
        }

        // NATIVE: list_pop_i32 - remove and return last element
        // Parameters: list_ptr (i32)
        // Returns: element (i32)
        let list_pop_i32_instructions = native_stdlib::list_ops::gen_pop_i32();
        self.register_function(
            "__list_pop_i32",
            &[WasmType::I32],
            Some(WasmType::I32),
            &list_pop_i32_instructions,
        )?;
        if let Some(lp_idx) = self.get_function_index("__list_pop_i32") {
            self.add_function_alias("list.pop", lp_idx);
        }

        // NATIVE: list_index_of_i32 - find index of element
        // Parameters: list_ptr (i32), value (i32)
        // Returns: index (i32) or -1 if not found
        let list_index_of_i32_instructions = native_stdlib::list_ops::gen_index_of_i32();
        let list_index_of_idx = self.register_function(
            "__list_index_of_i32",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_index_of_i32_instructions,
        )?;
        self.add_function_alias("list.indexOf", list_index_of_idx);

        // NATIVE: list_contains_i32 - check if list contains element
        // Parameters: list_ptr (i32), value (i32)
        // Returns: boolean (i32)
        let list_contains_i32_instructions =
            native_stdlib::list_ops::gen_contains_i32(list_index_of_idx);
        self.register_function(
            "__list_contains_i32",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_contains_i32_instructions,
        )?;
        if let Some(lc_idx) = self.get_function_index("__list_contains_i32") {
            self.add_function_alias("list.contains", lc_idx);
        }

        // NATIVE: list_push_i32 - add element to end of list (IN-PLACE MUTATION)
        // This mutates the list in place - no reallocation, no new pointer.
        // Safe because empty lists are pre-allocated with capacity 8.
        // Parameters: list_ptr (i32), value (i32)
        // Returns: list_ptr (i32) - same pointer, mutated in place
        let list_push_i32_instructions = native_stdlib::list_ops::gen_push_i32_inplace();
        self.register_function(
            "__list_push_i32",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_push_i32_instructions,
        )?;
        if let Some(lpu_idx) = self.get_function_index("__list_push_i32") {
            self.add_function_alias("list.push", lpu_idx);
            self.add_function_alias("array_push", lpu_idx);
        }

        Ok(())
    }

    /// Register list operation functions using WASM instructions from ListManager
    pub(crate) fn register_list_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::list_ops::ListManager;
        use crate::stdlib::memory::MemoryManager;
        use std::cell::RefCell;
        use std::rc::Rc;

        // Create a MemoryManager and ListManager instance
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(16))));
        let list_manager = ListManager::new(memory_manager);
        list_manager.register_functions(self)?;

        Ok(())
    }

    /// Register HTTP operation functions using HttpClass
    pub(crate) fn register_http_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::http_class::HttpClass;

        // Create an HttpClass instance and register its functions
        let http_class = HttpClass::new();
        http_class.register_functions(self)?;

        Ok(())
    }

    /// Register math operation functions using MathClass
    pub(crate) fn register_math_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::math_class::MathClass;
        use crate::types::WasmType;

        // FIRST: Register math imports that MathClass will alias
        // These must be registered before MathClass tries to create aliases

        // Core trig functions
        self.register_import_function(
            "env",
            "math_pow",
            &[WasmType::F64, WasmType::F64],
            Some(WasmType::F64),
        )?;
        self.register_import_function("env", "math_sin", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_cos", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_tan", &[WasmType::F64], Some(WasmType::F64))?;

        // Inverse trig functions
        self.register_import_function("env", "math_asin", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_acos", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_atan", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function(
            "env",
            "math_atan2",
            &[WasmType::F64, WasmType::F64],
            Some(WasmType::F64),
        )?;

        // Hyperbolic functions
        self.register_import_function("env", "math_sinh", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_cosh", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_tanh", &[WasmType::F64], Some(WasmType::F64))?;

        // Logarithmic and exponential functions
        self.register_import_function("env", "math_ln", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_log10", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_log2", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_exp", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_exp2", &[WasmType::F64], Some(WasmType::F64))?;

        // THEN: Create MathClass instance which creates aliases and native functions
        let math_class = MathClass::new();
        math_class.register_functions(self)?;

        Ok(())
    }

    /// Register string.split as an import
    /// This must be called BEFORE any stdlib functions are registered
    /// because WASM requires all imports to come before internal functions
    pub fn register_string_split_import(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        // Register string.split as an import that routes to the runtime function
        // This takes (string_ptr, delimiter_ptr) and returns a list pointer
        self.register_import_function(
            "env",
            "string.split",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;

        Ok(())
    }

    /// Register string trim functions as native WASM implementations
    /// These are pure WASM functions that trim whitespace from strings without host imports
    pub fn register_string_trim_imports(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        // Get malloc function index - required for allocating new trimmed strings
        let malloc_idx = self
            .get_function_index("__malloc")
            .or_else(|| self.get_function_index("malloc"))
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    "malloc not registered before trim functions",
                    None,
                    None,
                )
            })?;

        // NATIVE: string_trim - trims whitespace from both ends
        // Parameters: str_ptr (i32)
        // Returns: pointer (i32) to new trimmed string
        let trim_instructions = native_stdlib::string_ops::gen_trim(malloc_idx);
        let trim_idx = self.register_function_with_locals(
            "__string_trim",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // start_idx
                WasmType::I32, // end_idx
                WasmType::I32, // new_len
                WasmType::I32, // new_ptr
                WasmType::I32, // i
                WasmType::I32, // temp byte
            ],
            &trim_instructions,
        )?;

        // NATIVE: string_trim_start - trims whitespace from start
        // Parameters: str_ptr (i32)
        // Returns: pointer (i32) to new trimmed string
        let trim_start_instructions = native_stdlib::string_ops::gen_trim_start(malloc_idx);
        let trim_start_idx = self.register_function_with_locals(
            "__string_trim_start",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // start_idx
                WasmType::I32, // new_len
                WasmType::I32, // new_ptr
                WasmType::I32, // i
                WasmType::I32, // temp byte
            ],
            &trim_start_instructions,
        )?;

        // NATIVE: string_trim_end - trims whitespace from end
        // Parameters: str_ptr (i32)
        // Returns: pointer (i32) to new trimmed string
        let trim_end_instructions = native_stdlib::string_ops::gen_trim_end(malloc_idx);
        let trim_end_idx = self.register_function_with_locals(
            "__string_trim_end",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // end_idx
                WasmType::I32, // new_ptr
                WasmType::I32, // i
                WasmType::I32, // temp byte
            ],
            &trim_end_instructions,
        )?;

        // Add canonical dot-notation aliases
        self.add_function_alias("string.trim", trim_idx);
        self.add_function_alias("string.trimStart", trim_start_idx);
        self.add_function_alias("string.trimEnd", trim_end_idx);

        // NATIVE: string_to_upper - converts string to uppercase (ASCII)
        let to_upper_instructions = native_stdlib::string_ops::gen_to_upper(malloc_idx);
        let to_upper_idx = self.register_function_with_locals(
            "__string_to_upper",
            &[WasmType::I32],    // str_ptr
            Some(WasmType::I32), // returns new_ptr
            &[
                WasmType::I32, // str_len
                WasmType::I32, // new_ptr
                WasmType::I32, // i (loop counter)
                WasmType::I32, // ch
            ],
            &to_upper_instructions,
        )?;
        self.add_function_alias("string.toUpperCase", to_upper_idx);

        // NATIVE: string_to_lower - converts string to lowercase (ASCII)
        let to_lower_instructions = native_stdlib::string_ops::gen_to_lower(malloc_idx);
        let to_lower_idx = self.register_function_with_locals(
            "__string_to_lower",
            &[WasmType::I32],    // str_ptr
            Some(WasmType::I32), // returns new_ptr
            &[
                WasmType::I32, // str_len
                WasmType::I32, // new_ptr
                WasmType::I32, // i (loop counter)
                WasmType::I32, // ch
            ],
            &to_lower_instructions,
        )?;
        self.add_function_alias("string.toLowerCase", to_lower_idx);

        Ok(())
    }

    /// Register string_compare function import
    /// This is used for string equality comparisons (==, !=)
    pub fn register_string_compare_import(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        // Register string_compare as an import that compares two strings
        // Returns 1 if equal, 0 if not equal
        let idx = self.register_import_function(
            "env",
            "string_compare",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;

        // Add canonical alias — skip when the import was tree-shaken (u32::MAX sentinel)
        // to avoid inserting a bogus index that would generate an invalid Call instruction.
        if idx != u32::MAX {
            self.add_function_alias("string.compare", idx);
        }

        Ok(())
    }

    /// Register string_replace function import
    /// This is used for the string.replace() method
    pub fn register_string_replace_import(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        // Register string_replace as an import that replaces all occurrences of a substring
        // Parameters: (string_ptr: i32, search_ptr: i32, replace_ptr: i32) -> i32
        let idx = self.register_import_function(
            "env",
            "string_replace",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;

        // Add canonical aliases — skip when the import was tree-shaken (u32::MAX sentinel)
        // to avoid inserting a bogus index that would generate an invalid Call instruction.
        if idx != u32::MAX {
            self.add_function_alias("string.replace", idx);
            // replaceAll uses the same host function (replaces all occurrences)
            self.add_function_alias("string.replaceAll", idx);
        }

        Ok(())
    }

    /// Register string class operation functions using StringClass
    pub(crate) fn register_string_class_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::string_class::StringClass;

        // NOTE: string.split import is registered separately via register_string_split_import()
        // NOTE: string trim imports are registered separately via register_string_trim_imports()
        // NOTE: string replace import is registered separately via register_string_replace_import()
        // These are called earlier in the initialization sequence to ensure correct WASM indexing
        // Do NOT register them here to avoid duplicate registration issues

        // Create a StringClass instance and register its functions
        debug!("Creating StringClass instance");
        let string_class = StringClass::new();
        debug!(
            function_count = self.function_count,
            "Calling string_class.register_functions()"
        );
        string_class.register_functions(self)?;
        debug!(
            function_count = self.function_count,
            "StringClass registration completed"
        );

        Ok(())
    }

    /// Register list class operation functions using ListClass
    pub(crate) fn register_list_class_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::list_class::ListClass;

        // Create a ListClass instance with the dynamically resolved mem_alloc index
        // so list operations are resilient to import ordering changes.
        let mem_alloc_idx = self
            .get_function_index("mem_alloc")
            .unwrap_or(crate::stdlib::list_class::DEFAULT_MEM_ALLOC_CALL_INDEX_PUB);
        let list_class = ListClass::new_with_mem_alloc_idx(mem_alloc_idx);
        list_class.register_functions(self)?;

        Ok(())
    }

    /// Register method-style and list-behavior operations (isEmpty, isDefined, list.size, etc.)
    pub(crate) fn register_conditional_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::memory::MemoryManager;
        use std::cell::RefCell;
        use std::rc::Rc;

        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(16))));

        use crate::stdlib::method_style::MethodStyleManager;
        let method_style_manager = MethodStyleManager::new(memory_manager.clone());
        method_style_manager.register_functions(self)?;

        use crate::stdlib::list_behavior::ListBehaviorManager;
        let list_behavior_manager = ListBehaviorManager::new(memory_manager.clone());
        list_behavior_manager.register_functions(self)?;

        Ok(())
    }
}
