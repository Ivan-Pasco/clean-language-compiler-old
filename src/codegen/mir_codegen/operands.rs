//! Operand loading, constant emission, string expansion, and local-variable
//! store helpers for MIR code generation.

use super::*;
use wasm_encoder::{Instruction, ValType};

impl MirCodeGenerator<'_> {
    /// Load MIR operand onto WASM stack.
    pub(super) fn load_operand(&mut self, operand: &MirOperand) -> Result<(), CompilerError> {
        match operand {
            MirOperand::Value(value_id) => {
                if let Some(&local_index) = self.value_to_local.get(value_id) {
                    // For StringTuple, just load the single i32 pointer
                    // It will be stored as a pointer to memory with format: [4-byte length][content bytes]
                    // Functions that need (ptr, len) will handle expansion themselves (e.g., load_string_argument_for_print)
                    self.current_instructions
                        .push(Instruction::LocalGet(local_index));
                } else {
                    // NOTE: No more silent auto-allocation of missing ValueIds
                    // Return a proper error to surface MIR builder bugs
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            format!(
                                "ValueId({:?}) not found in local variable map during load_operand. \
                                This indicates the MIR builder did not properly track this value.",
                                value_id.0
                            ),
                            None,
                            crate::error::ErrorType::Codegen,
                            None,
                        )),
                    });
                }
            }

            MirOperand::Constant(constant) => {
                self.load_constant(constant)?;
            }

            MirOperand::Function(symbol_id) => {
                // Resolve the function's WASM index and emit it as an i32
                // constant. Each function reference now yields a distinct stable
                // integer (the WASM function index), so downstream host callers
                // (e.g. frame.ui handlers) receive unique handles instead of 0.
                let function_index = self
                    .symbol_to_function_index
                    .get(symbol_id)
                    .copied()
                    .unwrap_or(0);
                self.referenced_function_indices.insert(function_index);
                self.current_instructions
                    .push(Instruction::I32Const(function_index as i32));
            }

            MirOperand::NamedFunction { symbol_id, name } => {
                // Named function reference — same resolution as Function(symbol_id),
                // with a name-based fallback through function_map.
                let function_index = self
                    .symbol_to_function_index
                    .get(symbol_id)
                    .copied()
                    .or_else(|| self.wasm_generator.function_map.get(name).copied())
                    .unwrap_or(0);
                self.referenced_function_indices.insert(function_index);
                self.current_instructions
                    .push(Instruction::I32Const(function_index as i32));
            }

            MirOperand::Global(_symbol_id) => {
                // Global variable access (module globals indexed from 0)
                self.current_instructions.push(Instruction::GlobalGet(0));
            }
        }

        Ok(())
    }

    /// Load MIR constant onto WASM stack.
    pub(super) fn load_constant(&mut self, constant: &MirConstant) -> Result<(), CompilerError> {
        match constant {
            MirConstant::Integer(i) => {
                // Clean Language integers map to WASM i32, not i64
                self.current_instructions
                    .push(Instruction::I32Const(*i as i32));
            }
            MirConstant::Float(f) => {
                self.current_instructions.push(Instruction::F64Const(*f));
            }
            MirConstant::Boolean(b) => {
                self.current_instructions
                    .push(Instruction::I32Const(if *b { 1 } else { 0 }));
            }
            MirConstant::String(index) => {
                // NOTE: Load the string structure base offset (not index, not content offset)
                // String format in memory: [4-byte len][content]
                // Load base offset - points to the length field
                // The generate_terminator will expand this to (content_ptr, len) if needed
                if let Some(string_pool) = &self.string_pool {
                    if let Some(string_content) = string_pool.get(*index) {
                        let base_offset = self
                            .wasm_generator
                            .get_or_create_string_offset(string_content)?;
                        tracing::trace!(
                            index = index,
                            content = %string_content,
                            base_offset = base_offset,
                            "Loading string constant at base offset (points to [len|content] structure)"
                        );
                        self.current_instructions
                            .push(Instruction::I32Const(base_offset as i32));
                    } else {
                        return Err(CompilerError::Codegen {
                            context: Box::new(
                                crate::error::ErrorContext::new(
                                    format!("String index {} not found in string pool", index),
                                    None,
                                    crate::error::ErrorType::Codegen,
                                    None,
                                )
                                .with_error_code("COM001"),
                            ),
                        });
                    }
                } else {
                    return Err(CompilerError::Codegen {
                        context: Box::new(
                            crate::error::ErrorContext::new(
                                "No string pool available for string constant",
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )
                            .with_error_code("COM001"),
                        ),
                    });
                }
            }
            MirConstant::Null => {
                self.current_instructions.push(Instruction::I32Const(0));
            }
            MirConstant::Undefined => {
                // Undefined values are represented as 0
                self.current_instructions.push(Instruction::I32Const(0));
            }
            _ => {
                return Err(CompilerError::Codegen {
                    context: Box::new(crate::error::ErrorContext::new(
                        format!("Constant type not yet implemented: {:?}", constant),
                        None,
                        crate::error::ErrorType::Codegen,
                        Some(crate::ast::SourceLocation::default()),
                    )),
                });
            }
        }

        Ok(())
    }

    /// Load string pointer only (for input functions that don't need length).
    pub(super) fn load_string_pointer_only(
        &mut self,
        operand: &MirOperand,
    ) -> Result<(), CompilerError> {
        tracing::trace!(
            operand = ?operand,
            "load_string_pointer_only called"
        );
        match operand {
            MirOperand::Constant(MirConstant::String(index)) => {
                // For string constants, push the pointer to the string structure
                if let Some(string_pool) = &self.string_pool {
                    if let Some(string_content) = string_pool.get(*index) {
                        let data_offset = self
                            .wasm_generator
                            .get_or_create_string_offset(string_content)?;
                        // Push pointer to string structure (includes length prefix)
                        self.current_instructions
                            .push(Instruction::I32Const(data_offset as i32));
                    } else {
                        return Err(CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                format!("String constant {} not found in string pool", index),
                                None,
                                crate::error::ErrorType::Codegen,
                                Some(crate::ast::SourceLocation::default()),
                            )),
                        });
                    }
                } else {
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            "String pool not initialized for input function call".to_string(),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(crate::ast::SourceLocation::default()),
                        )),
                    });
                }
            }
            MirOperand::Value(_value_id) => {
                // NOTE: Always load values from their local variable, never use
                // cached string constant offsets. The value_to_string_index mapping only
                // records the INITIAL assignment - if the variable was reassigned (e.g.,
                // html = html + "..."), we need to read the CURRENT value from the local.
                // This fixes the string concatenation truncation bug where reassigned
                // string variables would print their original value instead of the new one.
                self.load_operand(operand)?;
            }
            _ => {
                // For other operand types, just load normally
                self.load_operand(operand)?;
            }
        }
        Ok(())
    }

    // NOTE: Full boxing implementation for `any` type is pending.
    // The `any` type currently works with i32 values only (integers, booleans, pointers).
    // f64 values are truncated to i32 when passed to `any` parameters.
    // Proper boxing with type tags and memory allocation will be implemented in a future version.
    // See MirType::Any and AnyTypeTag for the planned memory layout.

    /// Load string argument for print functions (expands to pointer + length).
    pub(super) fn load_string_argument_for_print(
        &mut self,
        operand: &MirOperand,
    ) -> Result<(), CompilerError> {
        debug_mir!(" LOAD_STRING: Called with operand: {:?}", operand);
        tracing::trace!(
            operand = ?operand,
            "load_string_argument_for_print called"
        );
        match operand {
            MirOperand::Constant(MirConstant::String(index)) => {
                debug_mir!(index = index, "Processing string constant");
                // For string constants, we need to expand to pointer + length
                if let Some(string_pool) = &self.string_pool {
                    if let Some(string_content) = string_pool.get(*index) {
                        debug_mir!(content = %string_content, "Found string content");
                        // Get the string offset in WASM memory using the underlying generator
                        let data_offset = self
                            .wasm_generator
                            .get_or_create_string_offset(string_content)?;
                        let str_len = string_content.len() as i32;
                        tracing::trace!(
                            offset = data_offset,
                            length = str_len,
                            "String offset and length"
                        );

                        // NOTE: data_offset points to the string structure [len|content]
                        // We need to skip the 4-byte length prefix to get to the content
                        let content_offset = data_offset + 4;

                        // Push pointer to string content (skipping 4-byte length prefix)
                        self.current_instructions
                            .push(Instruction::I32Const(content_offset as i32));
                        // Push string length
                        self.current_instructions
                            .push(Instruction::I32Const(str_len));
                    } else {
                        return Err(CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                format!("String constant {} not found in string pool", index),
                                None,
                                crate::error::ErrorType::Codegen,
                                Some(crate::ast::SourceLocation::default()),
                            )),
                        });
                    }
                } else {
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            "String pool not initialized for print function call".to_string(),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(crate::ast::SourceLocation::default()),
                        )),
                    });
                }
            }
            MirOperand::Value(value_id) => {
                debug_mir!(" LOAD_STRING: Processing Value({:?})", value_id);

                // Check the value type to determine if we need type conversion
                let value_type = self.get_value_type(*value_id);
                debug_mir!(" LOAD_STRING: Value type is {:?}", value_type);

                // NOTE: For non-string values, we must convert to string first
                // F64 (number type) -> float_to_string
                // I32 (integer type) -> int_to_string
                // Bool (boolean type) -> bool_to_string
                // Otherwise it's a string pointer -> expand normally

                if matches!(value_type, Some(MirType::F64)) {
                    debug_mir!(
                        " LOAD_STRING: Value is f64, converting to string via float_to_string"
                    );
                    // Load the f64 value
                    self.load_operand(operand)?;
                    // Get float_to_string function index
                    let float_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("float_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "float_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call float_to_string(f64) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(float_to_string_idx));

                    // Now we have a string pointer on the stack, expand it
                    let temp_local = self.next_local_index;
                    self.next_local_index += 1;
                    self.temp_local_types.insert(temp_local, ValType::I32);
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4)
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                } else if matches!(value_type, Some(MirType::Ptr(_))) {
                    // NOTE: Ptr(U8) values are string pointers - just expand to (content_ptr, length)
                    // This handles the result of toString() calls which return string pointers
                    debug_mir!(
                        " LOAD_STRING: Value is Ptr (string pointer), expanding to (ptr+4, len)"
                    );

                    // Load the string pointer from the local variable
                    self.load_operand(operand)?;

                    // Allocate a temporary local to hold the pointer
                    let temp_local = self.next_local_index;
                    self.next_local_index += 1;
                    self.temp_local_types.insert(temp_local, ValType::I32);

                    // Store pointer to temp local
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4, skipping length field)
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory at pointer location
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                } else if matches!(value_type, Some(MirType::I32)) {
                    // NOTE: Integer values need to be converted to strings
                    debug_mir!(" LOAD_STRING: Value is i32 (integer), converting to string via int_to_string");
                    // Load the i32 value
                    self.load_operand(operand)?;
                    // Get int_to_string function index (stdlib function)
                    let int_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("int_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "int_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call int_to_string(i32) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(int_to_string_idx));

                    // Now we have a string pointer on the stack, expand it
                    let temp_local = self.next_local_index;
                    self.next_local_index += 1;
                    self.temp_local_types.insert(temp_local, ValType::I32);
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4)
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                } else if matches!(value_type, Some(MirType::Bool)) {
                    // NOTE: Boolean values need to be converted to strings
                    debug_mir!(
                        " LOAD_STRING: Value is bool, converting to string via bool_to_string"
                    );
                    // Load the bool value
                    self.load_operand(operand)?;
                    // Get bool_to_string function index (stdlib function)
                    let bool_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("bool_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "bool_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call bool_to_string(i32) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(bool_to_string_idx));

                    // Now we have a string pointer on the stack, expand it
                    let temp_local = self.next_local_index;
                    self.next_local_index += 1;
                    self.temp_local_types.insert(temp_local, ValType::I32);
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4)
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                } else if matches!(value_type, Some(MirType::Any)) {
                    // Any-typed value (boxed pointer): convert to string via type dispatch,
                    // then expand the resulting string pointer to (content_ptr, length).
                    debug_mir!(" LOAD_STRING: Value is Any, converting via any_to_string dispatch");
                    self.load_operand(operand)?;
                    self.emit_any_to_string()?;
                    // emit_any_to_string leaves a string pointer on the stack
                    let temp_local = self.next_local_index;
                    self.next_local_index += 1;
                    self.temp_local_types.insert(temp_local, ValType::I32);
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));
                    // content_ptr = str_ptr + 4
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);
                    // length = mem[str_ptr]
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                } else {
                    // String or other pointer type - load and expand normally
                    // NOTE: Always load values from their local variable and expand
                    // to (content_ptr, length). Never use cached value_to_string_index mappings
                    // because the variable may have been reassigned (e.g., html = html + "...").
                    // The mapping only records the INITIAL assignment, not the current value.
                    // This fixes the string concatenation truncation bug.
                    tracing::trace!(
                        value_id = ?value_id.0,
                        "Expanding string pointer for ValueId (always load from local)"
                    );

                    // Load the string pointer from the local variable
                    debug_mir!(" LOAD_STRING: Loading operand to stack");
                    self.load_operand(operand)?;

                    // Allocate a temporary local to hold the pointer
                    let temp_local = self.next_local_index;
                    debug_mir!(" LOAD_STRING: Allocated temp_local={}", temp_local);
                    self.next_local_index += 1;
                    // Track type: string pointers are i32
                    self.temp_local_types.insert(temp_local, ValType::I32);

                    // Store pointer to temp local
                    debug_mir!(" LOAD_STRING: Storing pointer to temp_local");
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4, skipping length field)
                    debug_mir!(" LOAD_STRING: Calculating content_ptr = base_ptr + 4");
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory at pointer location
                    debug_mir!(" LOAD_STRING: Loading length from base_ptr");
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2, // 4-byte alignment for i32
                            memory_index: 0,
                        }));

                    debug_mir!(" LOAD_STRING: String pointer expansion completed - stack should have [content_ptr, length]");
                }
            }
            _ => {
                // NOTE: For non-string operands, we need to convert them to strings first
                // and then expand to (pointer, length) format

                // Check the operand type to determine if we need type conversion
                let operand_type = self.get_operand_mir_type(operand);
                debug_mir!(" LOAD_STRING: Non-string operand type: {:?}", operand_type);

                // Load the operand onto the stack
                self.load_operand(operand)?;

                // CRITICAL: For non-string operands, we must convert to string first
                // F64 (number type) -> float_to_string
                // I32 (integer type) -> int_to_string (but not string pointers)
                // Bool (boolean type) -> bool_to_string
                if matches!(operand_type, Some(MirType::F64)) {
                    debug_mir!(" LOAD_STRING: Converting f64 to string via float_to_string");
                    // Get float_to_string function index
                    let float_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("float_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "float_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call float_to_string(f64) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(float_to_string_idx));
                } else if matches!(operand_type, Some(MirType::I32)) {
                    debug_mir!(" LOAD_STRING: Converting i32 to string via int_to_string");
                    // Get int_to_string function index
                    let int_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("int_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "int_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call int_to_string(i32) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(int_to_string_idx));
                } else if matches!(operand_type, Some(MirType::Bool)) {
                    debug_mir!(" LOAD_STRING: Converting bool to string via bool_to_string");
                    // Get bool_to_string function index
                    let bool_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("bool_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "bool_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call bool_to_string(i32) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(bool_to_string_idx));
                }
                // For Ptr types and other string pointers, no conversion needed

                // The operand pushed a pointer to a string structure [len|content]
                // We need to expand this to (content_ptr, length) for printl

                // Allocate a temporary local to hold the pointer
                let temp_local = self.next_local_index;
                self.next_local_index += 1;
                // Track type: string pointers are i32
                self.temp_local_types.insert(temp_local, ValType::I32);

                // Store pointer to temp local
                self.current_instructions
                    .push(Instruction::LocalSet(temp_local));

                // Calculate content pointer (ptr + 4, skipping length field)
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions.push(Instruction::I32Const(4));
                self.current_instructions.push(Instruction::I32Add);

                // Load length from memory at pointer location
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2, // 4-byte alignment for i32
                        memory_index: 0,
                    }));
            }
        }
        debug_mir!("DEBUG MIR: load_string_argument_for_print completed successfully");
        Ok(())
    }

    /// Store value from WASM stack to local with automatic type conversion if needed.
    ///
    /// Checks if the value on the stack needs type conversion before storing.
    pub(super) fn store_to_local_with_conversion(
        &mut self,
        value_id: ValueId,
        source_type: Option<MirType>,
    ) -> Result<(), CompilerError> {
        if let Some(&local_index) = self.value_to_local.get(&value_id) {
            // Get destination type from function.locals
            if let Some(dest_type) = self.get_value_type(value_id) {
                // If we know the source type, check if conversion is needed
                if let Some(src_type) = source_type {
                    // NOTE: Add type conversions when assigning between i32 and f64
                    match (&src_type, &dest_type) {
                        // f64 → i32: Truncate float to integer
                        (MirType::F64, MirType::I32) => {
                            debug_mir!(
                                "DEBUG TYPE CONVERSION: Adding f64→i32 conversion for ValueId({:?})",
                                value_id.0
                            );
                            self.current_instructions.push(Instruction::I32TruncF64S);
                        }
                        // i32 → f64: Convert integer to float
                        (MirType::I32, MirType::F64) => {
                            debug_mir!(
                                "DEBUG TYPE CONVERSION: Adding i32→f64 conversion for ValueId({:?})",
                                value_id.0
                            );
                            self.current_instructions.push(Instruction::F64ConvertI32S);
                        }
                        // Same types or pointer types - no conversion needed
                        _ => {}
                    }
                }
            }

            // Store to local
            self.current_instructions
                .push(Instruction::LocalSet(local_index));
            Ok(())
        } else {
            // NOTE: No more silent auto-allocation of missing ValueIds
            // Return proper error to surface MIR builder bugs
            // All ValueIds must be properly registered in function.locals before codegen
            Err(CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    format!(
                        "ValueId({:?}) not found in local variable map during store_to_local. \
                        This indicates the MIR builder did not properly allocate this value in function.locals. \
                        All result values must be pre-allocated before code generation.",
                        value_id.0
                    ),
                    Some("Ensure MIR builder adds all ValueIds to function.locals before generating instructions".to_string()),
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })
        }
    }

    /// Store value to local without type conversion (backward compatibility wrapper).
    pub(super) fn store_to_local(&mut self, value_id: ValueId) -> Result<(), CompilerError> {
        self.store_to_local_with_conversion(value_id, None)
    }
}
