//! Utility methods for the `CodeGenerator`.
//! Contains type inference helpers, string pool management, memory utilities,
//! local variable management, and the built-in static method dispatcher.

use super::instruction_generator::LocalVarInfo;
use crate::ast::{Expression, SourceLocation, Statement, Type, Value};
use crate::error::CompilerError;
use crate::types::WasmType;
use tracing::debug;
use wasm_encoder::{EntityType, Function, Instruction};

impl super::CodeGenerator {
    /// Infer the element type of a list based on its declaration or context
    pub(crate) fn infer_list_element_type(
        &self,
        list_expr: &Expression,
    ) -> Result<WasmType, CompilerError> {
        use crate::ast::Expression;

        match list_expr {
            Expression::Variable(var_name) => {
                // Try to look up the variable type in our type context
                if let Some(clean_type) = self.variable_types.get(var_name) {
                    match clean_type {
                        Type::List(element_type) => {
                            // Convert the Clean Language element type to WASM type
                            match element_type.as_ref() {
                                Type::Integer => Ok(WasmType::I32),
                                Type::Number => Ok(WasmType::F64),
                                Type::String => Ok(WasmType::I32), // String pointers
                                Type::Boolean => Ok(WasmType::I32), // Booleans as i32
                                _ => Ok(WasmType::I32),            // Default to i32
                            }
                        }
                        _ => {
                            // Not a list type, default to i32 instead of name-based heuristics
                            // Name-based heuristics can be misleading (e.g., "numbers" contains "number")
                            Ok(WasmType::I32)
                        }
                    }
                } else {
                    // Variable not found in type context - this shouldn't happen for properly declared variables
                    // Default to i32 instead of unreliable name-based heuristics
                    Ok(WasmType::I32)
                }
            }
            // For other expressions, default to i32
            _ => Ok(WasmType::I32),
        }
    }

    // Add delegate methods to use instruction_generator
    // These should be part of the CodeGenerator implementation

    pub fn find_local(&self, name: &str) -> Option<LocalVarInfo> {
        self.variable_map.get(name).cloned()
    }

    // Removed duplicate get_function_index function - keeping the one defined earlier

    /// Get or create a function index for async runtime functions
    pub fn get_or_create_function_index(&mut self, name: &str) -> u32 {
        if let Some(index) = self.function_map.get(name) {
            *index
        } else {
            // Create a placeholder function index for async runtime functions
            let index = self.function_count;
            debug!(
                function_name = name,
                index = index,
                "get_or_create_function_index: Creating placeholder"
            );
            self.function_count += 1;
            self.function_map.insert(name.to_string(), index);
            self.function_names.push(name.to_string());

            // Add to import section for runtime functions
            match name {
                "create_future" => {
                    let func_type = self
                        .add_function_type(&[WasmType::I32], Some(WasmType::I32))
                        .unwrap();
                    self.import_section
                        .import("env", name, EntityType::Function(func_type));
                }
                "start_background_task" => {
                    let func_type = self
                        .add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))
                        .unwrap();
                    self.import_section
                        .import("env", name, EntityType::Function(func_type));
                }
                "execute_background" => {
                    let func_type = self
                        .add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))
                        .unwrap();
                    self.import_section
                        .import("env", name, EntityType::Function(func_type));
                }
                "resolve_future" => {
                    let func_type = self
                        .add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))
                        .unwrap();
                    self.import_section
                        .import("env", name, EntityType::Function(func_type));
                }
                "queue_background_task" => {
                    let func_type = self
                        .add_function_type(
                            &[WasmType::I32, WasmType::I32, WasmType::I32],
                            Some(WasmType::I32),
                        )
                        .unwrap();
                    self.import_section
                        .import("env", name, EntityType::Function(func_type));
                }
                "register_deferred_task" => {
                    let func_type = self
                        .add_function_type(
                            &[WasmType::I32, WasmType::I32, WasmType::I32],
                            Some(WasmType::I32),
                        )
                        .unwrap();
                    self.import_section
                        .import("env", name, EntityType::Function(func_type));
                }
                "queue_future_task" => {
                    let func_type = self
                        .add_function_type(
                            &[WasmType::I32, WasmType::I32, WasmType::I32],
                            Some(WasmType::I32),
                        )
                        .unwrap();
                    self.import_section
                        .import("env", name, EntityType::Function(func_type));
                }
                "associate_future_task" => {
                    let func_type = self
                        .add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))
                        .unwrap();
                    self.import_section
                        .import("env", name, EntityType::Function(func_type));
                }
                _ => {
                    // Default function signature for unknown async functions
                    let func_type = self
                        .add_function_type(&[WasmType::I32], Some(WasmType::I32))
                        .unwrap();
                    self.import_section
                        .import("env", name, EntityType::Function(func_type));
                }
            }

            index
        }
    }

    pub fn get_function_return_type(&self, index: u32) -> Result<WasmType, CompilerError> {
        self.instruction_generator.get_function_return_type(index)
    }

    /// Get the return type of a function by name
    pub(crate) fn get_function_return_type_by_name(&self, function_name: &str) -> WasmType {
        match function_name {
            // HTTP functions
            "http.get" | "http.post" | "http.put" | "http.delete" | "http.head"
            | "http.options" | "http.postJson" | "http.putJson" | "http.patchJson"
            | "http.encodeUrl" | "http.decodeUrl" => WasmType::I32, // String pointer
            "http.getResponseCode" => WasmType::I32, // Integer
            "http.getResponseHeaders" => WasmType::I32, // String pointer
            "http.setTimeout" | "http.setUserAgent" | "http.enableCookies" => WasmType::I32, // Void (represented as I32)

            // Math functions - Note: math.abs is handled specially in generate_expression
            "math.sin" | "math.cos" | "math.tan" | "math.asin" | "math.acos" | "math.atan"
            | "math.atan2" | "math.sinh" | "math.cosh" | "math.tanh" | "math.ln" | "math.log10"
            | "math.log2" | "math.exp" | "math.exp2" | "math.sqrt" | "math.floor" | "math.ceil"
            | "math.round" | "math.min" | "math.max" | "math.mod" | "math.pi" | "math.e" => {
                WasmType::F64
            } // Number

            // math.abs returns the same type as its input, handled specially
            "math.abs" => WasmType::F64,     // F64 version
            "math.abs.i32" => WasmType::I32, // I32 version

            // List functions
            "array.length" | "array.push" | "array.pop" | "array.indexOf" => WasmType::I32, // Integer
            "array.get" | "array.set" | "array.slice" | "array.concat" | "array.reverse"
            | "array.join" | "array.map" | "array.iterate" => WasmType::I32, // Pointer or void
            "array.contains" => WasmType::I32, // Boolean (as I32)

            // String functions
            "string.length" | "string.indexOf" | "string.lastIndexOf" | "string.compare"
            | "string.charCodeAt" => WasmType::I32, // Integer
            "string.concat" | "string.substring" | "string.toUpperCase" | "string.toLowerCase"
            | "string.trim" | "string.replace" | "string.replaceAll" | "string.split"
            | "string.join" | "string.padStart" | "string.padEnd" | "string.trimStart"
            | "string.trimEnd" | "string.charAt" | "string.toString" => WasmType::I32, // String pointer
            "string.startsWith"
            | "string.endsWith"
            | "string.contains"
            | "string.isEmpty"
            | "string.isNotEmpty"
            | "string.isBlank"
            | "string.isDefined"
            | "string.isNotDefined" => WasmType::I32, // Boolean (as I32)

            // File functions
            "file.read" => WasmType::I32, // String pointer
            "file.write" | "file.append" | "file.delete" => WasmType::I32, // Integer (success/failure)
            "file.exists" => WasmType::I32,                                // Boolean (as I32)

            // Memory management functions
            "mem_alloc" => WasmType::I32,         // Returns pointer
            "mem_collect" => WasmType::I32,       // Returns count
            "mem_get_ref_count" => WasmType::I32, // Returns reference count
            "mem_retain" | "mem_release" => WasmType::I32, // Void (as I32)

            // Type conversion functions
            "int_to_string" | "float_to_string" | "bool_to_string" => WasmType::I32, // String pointer
            "string_to_int" | "int_to_float" | "float_to_int" | "byte_to_int" | "int_to_byte" => {
                WasmType::I32
            }
            "string_to_float" => WasmType::F64,
            "i32_to_f64" => WasmType::F64,
            "f64_to_i32" | "i32_to_i64" | "i64_to_i32" => WasmType::I32,

            // Console I/O functions
            "print" | "printl" => WasmType::I32, // Void (as I32)
            "input" => WasmType::I32,            // String pointer
            "input.integer" | "input.range" => WasmType::I32,
            "input.number" => WasmType::F64,
            "input.yesNo" => WasmType::I32, // Boolean

            // List operations
            name if name.contains("list.") || name.contains("List.") => match name {
                "list.size" | "list.length" => WasmType::I32,
                "list.isEmpty" | "list.isNotEmpty" | "list.contains" => WasmType::I32, // Boolean
                "list.get" | "list.peek" | "list.remove" | "list.pop" => WasmType::I32,
                "list.allocate" | "list.add" | "list.set" => WasmType::I32,
                _ => WasmType::I32,
            },

            // Class method patterns (handle names like "Person_getName", "Rectangle_getArea")
            name if name.contains('_') => {
                if name.contains("get") && (name.contains("Name") || name.contains("String")) {
                    WasmType::I32 // String getter
                } else if name.contains("get")
                    && (name.contains("Area") || name.contains("Volume") || name.contains("Length"))
                {
                    WasmType::F64 // Numeric getter
                } else if name.contains("is") || name.contains("has") || name.contains("can") {
                    WasmType::I32 // Boolean predicate
                } else {
                    WasmType::I32 // Default for class methods
                }
            }

            // Default case
            _ => WasmType::I32, // Default to I32 for unknown functions
        }
    }

    pub fn get_array_get(&self) -> u32 {
        self.function_map.get("list.get").copied().unwrap_or(0)
    }

    pub fn get_array_length(&self) -> u32 {
        self.function_map.get("list.length").copied().unwrap_or(0)
    }

    pub fn get_matrix_get(&self) -> u32 {
        self.function_map.get("matrix_get").copied().unwrap_or(0)
    }

    pub fn register_function_with_locals(
        &mut self,
        name: &str,
        params: &[WasmType],
        return_type: Option<WasmType>,
        local_types: &[WasmType],
        instructions: &[Instruction],
    ) -> Result<u32, CompilerError> {
        // Get the current function index (this will be the index for the new function)
        let function_index = self.function_count;

        // Register with instruction_generator for internal tracking
        // Pass the function_index so InstructionGenerator uses the same index
        self.instruction_generator.register_function(
            name,
            params,
            return_type,
            instructions,
            function_index,
        )?;

        // Add the function type to the type section
        let type_index = self.add_function_type(params, return_type)?;

        // Add the function to the function section
        self.function_section.function(type_index);

        // Create a Function with explicit local types
        let locals_needed: Vec<(u32, wasm_encoder::ValType)> = local_types
            .iter()
            .map(|wasm_type| {
                (
                    1u32,
                    match wasm_type {
                        WasmType::I32 => wasm_encoder::ValType::I32,
                        WasmType::F64 => wasm_encoder::ValType::F64,
                        WasmType::I64 => wasm_encoder::ValType::I64,
                        WasmType::F32 => wasm_encoder::ValType::F32,
                        WasmType::V128 => wasm_encoder::ValType::V128,
                        WasmType::Unit => wasm_encoder::ValType::I32, // Default to I32 for Unit type
                    },
                )
            })
            .collect();

        let mut func = Function::new(locals_needed);
        for inst in instructions {
            func.instruction(inst);
        }

        // Always add END instruction to close the function body
        func.instruction(&Instruction::End);

        // Add the function body to the code section
        self.code_section.function(&func);

        // Update function tracking data (similar to register_function)
        self.function_names.push(name.to_string());
        self.function_map.insert(name.to_string(), function_index);

        // Increment function count and return the index
        self.function_count += 1;
        Ok(function_index)
    }

    pub fn register_function(
        &mut self,
        name: &str,
        params: &[WasmType],
        return_type: Option<WasmType>,
        instructions: &[Instruction],
    ) -> Result<u32, CompilerError> {
        // Get the current function index (this will be the index for the new function)
        let function_index = self.function_count;

        // Register with instruction_generator for internal tracking
        // Pass the function_index so InstructionGenerator uses the same index
        self.instruction_generator.register_function(
            name,
            params,
            return_type,
            instructions,
            function_index,
        )?;

        // Add the function type to the type section
        let type_index = self.add_function_type(params, return_type)?;

        // Add the function to the function section
        self.function_section.function(type_index);

        // Create a Function - parameters are automatically available as locals 0, 1, 2, ...
        // For complex functions, we need additional local variables beyond parameters
        // Determine how many locals are needed based on the highest LocalGet index in instructions
        let max_local_index = instructions
            .iter()
            .filter_map(|inst| match inst {
                Instruction::LocalGet(idx)
                | Instruction::LocalSet(idx)
                | Instruction::LocalTee(idx) => Some(*idx),
                _ => None,
            })
            .max()
            .unwrap_or(0);

        // Calculate how many locals we need beyond the parameters
        let param_count = params.len() as u32;
        let locals_needed: Vec<(u32, wasm_encoder::ValType)> = if max_local_index >= param_count {
            // We need additional locals beyond parameters
            let additional_locals = max_local_index - param_count + 1;
            // Default additional locals to I32, but this could be improved with type inference
            (0..additional_locals)
                .map(|_| (1u32, wasm_encoder::ValType::I32))
                .collect()
        } else {
            // No additional locals needed beyond parameters
            vec![]
        };

        let mut func = Function::new(locals_needed);

        // Add all generated instructions
        for inst in instructions {
            func.instruction(inst);
        }

        // Always add END instruction to close the function body
        func.instruction(&Instruction::End);

        // Add the function to the code section
        self.code_section.function(&func);

        // Do NOT add exports for stdlib functions - they are for internal use only
        // self.export_section.export(name, wasm_encoder::ExportKind::Func, function_index);

        // Update other tracking data
        self.function_names.push(name.to_string());
        self.function_map.insert(name.to_string(), function_index);
        self.function_count += 1;

        // Return the function index
        Ok(function_index)
    }

    /// Add an alias name for an existing function
    /// This allows a function to be referenced by multiple names
    pub fn add_function_alias(&mut self, alias: &str, function_index: u32) {
        self.function_map.insert(alias.to_string(), function_index);
    }

    /// Register a function with multiple return values (for WebAssembly multi-value returns)
    pub fn register_function_multi(
        &mut self,
        name: &str,
        params: &[WasmType],
        return_types: &[WasmType],
        instructions: &[Instruction],
    ) -> Result<u32, CompilerError> {
        // Get the current function index
        let function_index = self.function_count;

        // Add the function type to the type section using multi-value support
        let type_index = self.add_function_type_multi(params, return_types)?;

        // Add the function to the function section
        self.function_section.function(type_index);

        // Create a Function - parameters are automatically available as locals 0, 1, 2, ...
        // Determine how many locals are needed based on the highest LocalGet index in instructions
        let max_local_index = instructions
            .iter()
            .filter_map(|inst| match inst {
                Instruction::LocalGet(idx)
                | Instruction::LocalSet(idx)
                | Instruction::LocalTee(idx) => Some(*idx),
                _ => None,
            })
            .max()
            .unwrap_or(0);

        // Calculate how many locals we need beyond the parameters
        let param_count = params.len() as u32;
        let locals_needed: Vec<(u32, wasm_encoder::ValType)> = if max_local_index >= param_count {
            // We need additional locals beyond parameters
            let additional_locals = max_local_index - param_count + 1;
            // Default additional locals to I32, but this could be improved with type inference
            (0..additional_locals)
                .map(|_| (1u32, wasm_encoder::ValType::I32))
                .collect()
        } else {
            // No additional locals needed beyond parameters
            vec![]
        };

        let mut func = Function::new(locals_needed);

        // Add all generated instructions
        for inst in instructions {
            func.instruction(inst);
        }

        // Always add END instruction to close the function body
        func.instruction(&Instruction::End);

        // Add the function to the code section
        self.code_section.function(&func);

        // Update tracking data
        self.function_names.push(name.to_string());
        self.function_map.insert(name.to_string(), function_index);
        self.function_count += 1;

        // Also register with instruction_generator for internal tracking
        // For now, use first return type for compatibility
        // Pass the function_index so InstructionGenerator uses the same index
        let single_return = return_types.first().copied();
        self.instruction_generator.register_function(
            name,
            params,
            single_return,
            instructions,
            function_index,
        )?;

        // Return the function index
        Ok(function_index)
    }

    pub fn generate_error_handler_blocks(
        &mut self,
        try_block: &[Statement],
        _error_variable: Option<&str>,
        _catch_block: &[Statement],
        _location: &Option<SourceLocation>,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // For now, implement a simple try-catch mechanism using WASM's try-catch instructions
        // Note: Full exception handling in WASM requires the exception handling proposal

        // Generate try block instructions
        let mut try_instructions = Vec::new();
        for stmt in try_block {
            self.generate_statement(stmt, &mut try_instructions)?;
        }

        // For now, we'll implement a simplified version without actual exception handling
        // In a full implementation, this would use WASM's try-catch instructions

        // Add the try block instructions directly
        instructions.extend(try_instructions);

        // WASM exception handling proposal is not yet stable. The try block executes
        // directly; catch blocks are parsed but not emitted as WASM try-catch.

        Ok(())
    }

    pub fn allocate_string(&mut self, s: &str) -> Result<u32, CompilerError> {
        let result = self.memory_utils.allocate_string(s)?;
        Ok(result as u32)
    }

    /// Allocate a string for integration testing with 1-byte length prefix format
    pub fn allocate_simple_test_string(&mut self, s: &str) -> Result<u32, CompilerError> {
        let bytes = s.as_bytes();
        let len = bytes.len();

        if len > 255 {
            return Err(CompilerError::codegen_error(
                "Test string too long for 1-byte length",
                None,
                None,
            ));
        }

        // Find a suitable memory address for the test string
        // Use a high address to avoid conflicts
        let test_ptr = 8192u32; // Start at 8KB

        // Create data segment: [length_byte, string_bytes...]
        let mut data = Vec::with_capacity(len + 1);
        data.push(len as u8); // 1-byte length prefix
        data.extend_from_slice(bytes); // String content

        // Add the data segment to memory
        let _ = self.memory_utils.add_data_segment(test_ptr, &data);

        Ok(test_ptr)
    }

    pub fn allocate_array(&mut self, elements: &[Value]) -> Result<u32, CompilerError> {
        let result = self.memory_utils.allocate_array(elements)?;
        Ok(result as u32)
    }

    pub fn allocate_array_with_target_type(
        &mut self,
        elements: &[Value],
        target_element_type: Option<&Type>,
    ) -> Result<u32, CompilerError> {
        let result = self
            .memory_utils
            .allocate_array_with_target_type(elements, target_element_type)?;
        Ok(result as u32)
    }

    pub fn allocate_matrix(
        &mut self,
        data: &[f64],
        _rows: usize,
        cols: usize,
    ) -> Result<u32, CompilerError> {
        // Create a matrix from the flat array data
        let matrix_data: Vec<Vec<f64>> = data.chunks(cols).map(|chunk| chunk.to_vec()).collect();

        // Now call the memory utils allocate_matrix with the proper structure
        let result = self.memory_utils.allocate_matrix(&matrix_data)?;

        // Convert the usize result to u32
        Ok(result as u32)
    }

    pub fn retain_memory(&mut self, ptr: u32) -> Result<(), CompilerError> {
        self.memory_utils.retain(ptr as usize)
    }

    pub fn release_memory(&mut self, ptr: u32) -> Result<(), CompilerError> {
        self.memory_utils.release(ptr as usize)
    }

    pub(crate) fn generate_builtin_static_method_call(
        &mut self,
        class_name: &str,
        method: &str,
        arguments: &[Expression],
        instructions: &mut Vec<Instruction>,
    ) -> Result<Option<WasmType>, CompilerError> {
        match class_name {
            // Note: MathUtils static methods removed to avoid confusion with modules/MathUtils.clean
            // Use the existing modules/MathUtils.clean module instead
            // NOTE: StringUtils removed - all string operations are available in string_ops.rs
            // Use the existing string functions directly: string_length, string_concat, etc.
            "String" | "string" => {
                match method {
                    "length" => {
                        // Generate the string argument
                        self.generate_expression(&arguments[0], instructions)?;

                        // Call string length function
                        if let Some(string_length_index) = self.get_function_index("string.length")
                        {
                            instructions.push(Instruction::Call(string_length_index));
                            Ok(Some(WasmType::I32))
                        } else {
                            instructions.push(Instruction::I32Const(0)); // Placeholder
                            Ok(Some(WasmType::I32))
                        }
                    }
                    _ => Ok(None), // No handling for other string static methods
                }
            }
            "Math" | "math" => {
                match method {
                    "max" => {
                        // Generate the arguments
                        for arg in arguments {
                            self.generate_expression(arg, instructions)?;
                        }

                        // Call math max function
                        if let Some(max_index) = self.get_function_index("max") {
                            instructions.push(Instruction::Call(max_index));
                            // Return type depends on argument types - for now assume integer
                            Ok(Some(WasmType::I32))
                        } else {
                            instructions.push(Instruction::I32Const(0)); // Placeholder
                            Ok(Some(WasmType::I32))
                        }
                    }
                    _ => Ok(None), // No handling for other math static methods
                }
            }
            "List" => {
                match method {
                    "length" => {
                        // Generate the array argument
                        self.generate_expression(&arguments[0], instructions)?;

                        // Call array length function
                        if let Some(array_length_index) = self.get_function_index("array.length") {
                            instructions.push(Instruction::Call(array_length_index));
                            Ok(Some(WasmType::I32))
                        } else {
                            instructions.push(Instruction::I32Const(0)); // Placeholder
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "get" => {
                        // Generate array and index arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array.get") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // Element pointer
                        } else {
                            instructions.push(Instruction::I32Const(0));
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "set" => {
                        // Generate array, index, and value arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;
                        self.generate_expression(&arguments[2], instructions)?;

                        if let Some(function_index) = self.get_function_index("array.set") {
                            instructions.push(Instruction::Call(function_index));
                        }
                        Ok(None) // Void return
                    }
                    "push" => {
                        // Generate array and element arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_push") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "pop" => {
                        // Generate the array argument
                        self.generate_expression(&arguments[0], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_pop") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // Element
                        } else {
                            instructions.push(Instruction::I32Const(0));
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "contains" => {
                        // Generate array and item arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_contains") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // Boolean
                        } else {
                            instructions.push(Instruction::I32Const(0));
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "indexOf" => {
                        // Generate array and item arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_index_of") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // Index
                        } else {
                            instructions.push(Instruction::I32Const(-1));
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "slice" => {
                        // Generate array, start, and end arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;
                        if arguments.len() >= 3 {
                            self.generate_expression(&arguments[2], instructions)?;
                        } else {
                            instructions.push(Instruction::I32Const(-1)); // Use -1 for end if not provided
                        }

                        if let Some(function_index) = self.get_function_index("array_slice") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "concat" => {
                        // Generate two array arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_concat") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "reverse" => {
                        // Generate the array argument
                        self.generate_expression(&arguments[0], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_reverse") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "join" => {
                        // Generate array and separator arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_join") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // String pointer
                        } else {
                            instructions.push(Instruction::I32Const(0));
                            Ok(Some(WasmType::I32))
                        }
                    }
                    // Utility methods that can be implemented using basic operations
                    "isEmpty" => {
                        // Generate the array argument
                        self.generate_expression(&arguments[0], instructions)?;

                        // Call array.length and check if it's 0
                        if let Some(array_length_index) = self.get_function_index("array.length") {
                            instructions.push(Instruction::Call(array_length_index));
                            instructions.push(Instruction::I32Const(0));
                            instructions.push(Instruction::I32Eq); // length == 0
                            Ok(Some(WasmType::I32)) // Boolean
                        } else {
                            instructions.push(Instruction::I32Const(1)); // Assume empty
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "first" => {
                        // Generate the array argument
                        self.generate_expression(&arguments[0], instructions)?;

                        // Call array.get with index 0
                        instructions.push(Instruction::I32Const(0));
                        if let Some(function_index) = self.get_function_index("array.get") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // Element
                        } else {
                            instructions.push(Instruction::I32Const(0));
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "last" => {
                        // Generate the array argument
                        self.generate_expression(&arguments[0], instructions)?;

                        // Get array length - 1 and use as index
                        instructions.push(Instruction::LocalTee(0)); // Store array in local 0
                        if let Some(array_length_index) = self.get_function_index("array.length") {
                            instructions.push(Instruction::Call(array_length_index));
                            instructions.push(Instruction::I32Const(1));
                            instructions.push(Instruction::I32Sub); // length - 1
                            instructions.push(Instruction::LocalGet(0)); // Get array back
                            instructions.push(Instruction::LocalGet(1)); // Get index
                            if let Some(get_index) = self.get_function_index("array.get") {
                                instructions.push(Instruction::Call(get_index));
                            }
                            Ok(Some(WasmType::I32)) // Element
                        } else {
                            instructions.push(Instruction::I32Const(0));
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "isNotEmpty" => {
                        // Generate the array argument
                        self.generate_expression(&arguments[0], instructions)?;

                        // Call array.length and check if it's > 0
                        if let Some(array_length_index) = self.get_function_index("array.length") {
                            instructions.push(Instruction::Call(array_length_index));
                            instructions.push(Instruction::I32Const(0));
                            instructions.push(Instruction::I32GtS); // length > 0
                            Ok(Some(WasmType::I32)) // Boolean
                        } else {
                            instructions.push(Instruction::I32Const(0)); // Assume empty
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "lastIndexOf" => {
                        // Generate array and item arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_last_index_of")
                        {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // Index
                        } else {
                            instructions.push(Instruction::I32Const(-1));
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "insert" => {
                        // Generate array, index, and item arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;
                        self.generate_expression(&arguments[2], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_insert") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "remove" => {
                        // Generate array and index arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_remove") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // Removed element
                        } else {
                            instructions.push(Instruction::I32Const(0));
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "sort" => {
                        // Generate the array argument
                        self.generate_expression(&arguments[0], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_sort") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "map" => {
                        // Generate array and callback arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array.map") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "filter" => {
                        // Generate array and callback arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_filter") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "reduce" => {
                        // Generate array, callback, and initial value arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;
                        self.generate_expression(&arguments[2], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_reduce") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // Result value
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "forEach" => {
                        // Generate array and callback arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array.iterate") {
                            instructions.push(Instruction::Call(function_index));
                        }
                        Ok(None) // Void return
                    }
                    "fill" => {
                        // Generate size and value arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_fill") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    "range" => {
                        // Generate start and end arguments
                        self.generate_expression(&arguments[0], instructions)?;
                        self.generate_expression(&arguments[1], instructions)?;

                        if let Some(function_index) = self.get_function_index("array_range") {
                            instructions.push(Instruction::Call(function_index));
                            Ok(Some(WasmType::I32)) // List pointer
                        } else {
                            Ok(Some(WasmType::I32))
                        }
                    }
                    _ => Ok(None), // Method not found in List
                }
            }
            "File" => {
                match method {
                    "read" => {
                        // Generate the file path argument as string
                        self.generate_string_for_import(&arguments[0], instructions)?;

                        // Call the file_read import function
                        if let Some(file_read_index) =
                            self.file_import_indices.get("file_read").copied()
                        {
                            instructions.push(Instruction::Call(file_read_index));
                            Ok(Some(WasmType::I32)) // Returns pointer to file content or -1 for error
                        } else {
                            Err(CompilerError::codegen_error(
                                "File read function not found",
                                Some(
                                    "file_read import function needs to be registered".to_string(),
                                ),
                                None,
                            ))
                        }
                    }
                    "write" => {
                        // Generate file path and content arguments as strings
                        self.generate_string_for_import(&arguments[0], instructions)?;
                        self.generate_string_for_import(&arguments[1], instructions)?;

                        // Call the file_write import function
                        if let Some(file_write_index) =
                            self.file_import_indices.get("file_write").copied()
                        {
                            instructions.push(Instruction::Call(file_write_index));
                            Ok(Some(WasmType::I32)) // Returns 0 for success, -1 for error
                        } else {
                            Err(CompilerError::codegen_error(
                                "File write function not found",
                                Some(
                                    "file_write import function needs to be registered".to_string(),
                                ),
                                None,
                            ))
                        }
                    }
                    "append" => {
                        // Generate file path and content arguments as strings
                        self.generate_string_for_import(&arguments[0], instructions)?;
                        self.generate_string_for_import(&arguments[1], instructions)?;

                        // Call the file_append import function
                        if let Some(file_append_index) =
                            self.file_import_indices.get("file_append").copied()
                        {
                            instructions.push(Instruction::Call(file_append_index));
                            Ok(Some(WasmType::I32)) // Returns 0 for success, -1 for error
                        } else {
                            Err(CompilerError::codegen_error(
                                "File append function not found",
                                Some(
                                    "file_append import function needs to be registered"
                                        .to_string(),
                                ),
                                None,
                            ))
                        }
                    }
                    "exists" => {
                        // Generate the file path argument as string
                        self.generate_string_for_import(&arguments[0], instructions)?;

                        // Call the file_exists import function
                        if let Some(file_exists_index) =
                            self.file_import_indices.get("file_exists").copied()
                        {
                            instructions.push(Instruction::Call(file_exists_index));
                            Ok(Some(WasmType::I32)) // Returns 1 if exists, 0 if not
                        } else {
                            Err(CompilerError::codegen_error(
                                "File exists function not found",
                                Some(
                                    "file_exists import function needs to be registered"
                                        .to_string(),
                                ),
                                None,
                            ))
                        }
                    }
                    "delete" => {
                        // Generate the file path argument as string
                        self.generate_string_for_import(&arguments[0], instructions)?;

                        // Call the file_delete import function
                        if let Some(file_delete_index) =
                            self.file_import_indices.get("file_delete").copied()
                        {
                            instructions.push(Instruction::Call(file_delete_index));
                            Ok(Some(WasmType::I32)) // Returns 0 for success, -1 for error
                        } else {
                            Err(CompilerError::codegen_error(
                                "File delete function not found",
                                Some(
                                    "file_delete import function needs to be registered"
                                        .to_string(),
                                ),
                                None,
                            ))
                        }
                    }
                    "lines" => {
                        // Generate the file path argument as string
                        self.generate_string_for_import(&arguments[0], instructions)?;

                        // For now, use file_read and return the content as a single "line"
                        // In a full implementation, this would parse lines and return an array
                        if let Some(file_read_index) =
                            self.file_import_indices.get("file_read").copied()
                        {
                            instructions.push(Instruction::Call(file_read_index));
                            Ok(Some(WasmType::I32)) // Returns pointer to content (treating as single line for now)
                        } else {
                            Err(CompilerError::codegen_error(
                                "File read function not found for lines operation",
                                Some(
                                    "file_read import function needs to be registered".to_string(),
                                ),
                                None,
                            ))
                        }
                    }
                    _ => Ok(None), // Method not found in File
                }
            }
            "Http" => {
                match method {
                    "get" => {
                        // Generate the URL argument as string (ptr, len)
                        self.generate_string_for_import(&arguments[0], instructions)?;

                        // Call http_get import - requires bridge to provide implementation
                        if let Some(http_get_index) =
                            self.http_import_indices.get("http_get").copied()
                        {
                            instructions.push(Instruction::Call(http_get_index));
                            Ok(Some(WasmType::I32)) // Returns string pointer to response
                        } else {
                            Err(CompilerError::codegen_error(
                                "Http.get requires http_get import - ensure bridge provides this function",
                                Some("HTTP operations are platform-dependent and require a bridge implementation".to_string()),
                                None,
                            ))
                        }
                    }
                    "post" => {
                        // Generate URL and body arguments as strings
                        self.generate_string_for_import(&arguments[0], instructions)?;
                        self.generate_string_for_import(&arguments[1], instructions)?;

                        // Call http_post import
                        if let Some(http_post_index) =
                            self.http_import_indices.get("http_post").copied()
                        {
                            instructions.push(Instruction::Call(http_post_index));
                            Ok(Some(WasmType::I32)) // Returns string pointer to response
                        } else {
                            Err(CompilerError::codegen_error(
                                "Http.post requires http_post import - ensure bridge provides this function",
                                Some("HTTP operations are platform-dependent and require a bridge implementation".to_string()),
                                None,
                            ))
                        }
                    }
                    "put" => {
                        // Generate URL and body arguments as strings
                        self.generate_string_for_import(&arguments[0], instructions)?;
                        self.generate_string_for_import(&arguments[1], instructions)?;

                        // Call http_put import
                        if let Some(http_put_index) =
                            self.http_import_indices.get("http_put").copied()
                        {
                            instructions.push(Instruction::Call(http_put_index));
                            Ok(Some(WasmType::I32)) // Returns string pointer to response
                        } else {
                            Err(CompilerError::codegen_error(
                                "Http.put requires http_put import - ensure bridge provides this function",
                                Some("HTTP operations are platform-dependent and require a bridge implementation".to_string()),
                                None,
                            ))
                        }
                    }
                    "patch" => {
                        // Generate URL and body arguments as strings
                        self.generate_string_for_import(&arguments[0], instructions)?;
                        self.generate_string_for_import(&arguments[1], instructions)?;

                        // Call http_patch import
                        if let Some(http_patch_index) =
                            self.http_import_indices.get("http_patch").copied()
                        {
                            instructions.push(Instruction::Call(http_patch_index));
                            Ok(Some(WasmType::I32)) // Returns string pointer to response
                        } else {
                            Err(CompilerError::codegen_error(
                                "Http.patch requires http_patch import - ensure bridge provides this function",
                                Some("HTTP operations are platform-dependent and require a bridge implementation".to_string()),
                                None,
                            ))
                        }
                    }
                    "delete" => {
                        // Generate the URL argument as string
                        self.generate_string_for_import(&arguments[0], instructions)?;

                        // Call http_delete import
                        if let Some(http_delete_index) =
                            self.http_import_indices.get("http_delete").copied()
                        {
                            instructions.push(Instruction::Call(http_delete_index));
                            Ok(Some(WasmType::I32)) // Returns string pointer to response
                        } else {
                            Err(CompilerError::codegen_error(
                                "Http.delete requires http_delete import - ensure bridge provides this function",
                                Some("HTTP operations are platform-dependent and require a bridge implementation".to_string()),
                                None,
                            ))
                        }
                    }
                    _ => Ok(None), // Method not found in Http
                }
            }
            // Note: Second MathUtils section also removed for consistency
            _ => Ok(None), // Class not found in built-ins
        }
    }
}
