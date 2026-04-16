//! MIR instruction-to-WASM emission.
//!
//! Contains `generate_instruction`, `generate_terminator`, and all helpers that
//! directly emit WASM instructions for individual MIR operations.

use super::*;
use wasm_encoder::{BlockType, Instruction, ValType};

impl MirCodeGenerator<'_> {
    /// Generate WASM instructions from a single MIR instruction.
    pub(super) fn generate_instruction(
        &mut self,
        instruction: &MirInstruction,
    ) -> Result<(), CompilerError> {
        match &instruction.operation {
            MirOperation::Copy { source } => {
                // Load source operand and store to destination
                self.load_operand(source)?;
                if let Some(dest) = instruction.dest {
                    // Track string constants being copied to locals
                    if let MirOperand::Constant(MirConstant::String(index)) = source {
                        tracing::trace!(
                            value_id = ?dest.0,
                            string_index = index,
                            "Tracking string constant"
                        );
                        self.value_to_string_index.insert(dest, *index);
                    }
                    // NOTE: Pass source type for automatic type conversion
                    let source_type = self.get_operand_mir_type(source);
                    self.store_to_local_with_conversion(dest, source_type)?;
                } else {
                    // No destination - drop the value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::Phi { incoming } => {
                // SSA Phi nodes are NO-OPs in WASM structured control flow.
                //
                // Why: Phi marks where values from different control flow paths merge.
                // In proper SSA with Phi nodes for loops:
                // 1. INIT block: Sets initial value (e.g., counter = 0) to the Phi result local
                // 2. HEADER block: Phi node (THIS - just a marker, NO-OP in WASM)
                // 3. BODY block: Updates value, Copy to Phi result local
                // 4. Jump back to HEADER
                //
                // The Phi result local ALREADY has the right value from:
                // - First iteration: INIT block set it
                // - Subsequent iterations: BODY's Copy instruction updated it
                //
                // If we generate code for Phi, we'd RESET the value every iteration!
                // Therefore: Phi is a complete NO-OP in WASM codegen.
                debug_mir!(
                    "DEBUG PHI: Phi node is NO-OP: dest={:?}, incoming={:?}",
                    instruction.dest,
                    incoming
                );
            }

            MirOperation::BinaryOp { op, left, right } => {
                // NOTE: Type-aware binary operations with automatic conversions
                let left_is_float = self.is_float_operand(left);
                let right_is_float = self.is_float_operand(right);

                // Load left operand
                self.load_operand(left)?;
                // Convert left to f64 if right is float and left is not
                if !left_is_float && right_is_float {
                    self.current_instructions.push(Instruction::F64ConvertI32S);
                }

                // Load right operand
                self.load_operand(right)?;
                // Convert right to f64 if left is float and right is not
                if left_is_float && !right_is_float {
                    self.current_instructions.push(Instruction::F64ConvertI32S);
                }

                // Generate the operation
                self.generate_binary_operation(op, left, right)?;
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                } else {
                    // No destination - drop the result to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::UnaryOp { op, operand } => {
                // BOOK: required-operator - Special handling for Required operator
                // Required needs to check if value is null and trap if so
                if matches!(op, MirUnaryOp::Required) {
                    // Required assertion: value! traps if value is null
                    self.load_operand(operand)?;

                    if let Some(dest) = instruction.dest {
                        // Use dest local to store value and perform null check
                        if let Some(&local_index) = self.value_to_local.get(&dest) {
                            // Store to local and keep on stack with tee
                            self.current_instructions
                                .push(Instruction::LocalTee(local_index));
                            // Check if null (0)
                            self.current_instructions.push(Instruction::I32Eqz);
                            // If null, trap
                            self.current_instructions
                                .push(Instruction::If(wasm_encoder::BlockType::Empty));
                            self.current_instructions.push(Instruction::Unreachable);
                            self.current_instructions.push(Instruction::End);
                            // Value is still in local, load it back for the result
                            self.current_instructions
                                .push(Instruction::LocalGet(local_index));
                            // Store to dest (which is the same local)
                            self.current_instructions
                                .push(Instruction::LocalSet(local_index));
                        } else {
                            // No local mapping - just do the check and drop
                            self.current_instructions.push(Instruction::I32Eqz);
                            self.current_instructions
                                .push(Instruction::If(wasm_encoder::BlockType::Empty));
                            self.current_instructions.push(Instruction::Unreachable);
                            self.current_instructions.push(Instruction::End);
                        }
                    } else {
                        // No destination - just check and drop
                        // Stack: [value]
                        // Check if null
                        self.current_instructions.push(Instruction::I32Eqz);
                        self.current_instructions
                            .push(Instruction::If(wasm_encoder::BlockType::Empty));
                        self.current_instructions.push(Instruction::Unreachable);
                        self.current_instructions.push(Instruction::End);
                        // Value was consumed by the check, nothing to drop
                    }
                } else {
                    // Normal unary operation
                    self.load_operand(operand)?;
                    self.generate_unary_operation(op)?;
                    if let Some(dest) = instruction.dest {
                        self.store_to_local(dest)?;
                    } else {
                        // No destination - drop the result to avoid stack pollution
                        self.current_instructions.push(Instruction::Drop);
                    }
                }
            }

            MirOperation::Load { source } => {
                tracing::trace!(
                    source = ?source,
                    "Processing Load operation"
                );
                // Load from memory
                match self.load_operand(source) {
                    Ok(_) => debug_mir!("Load operand successful"),
                    Err(e) => {
                        debug_mir!(error = ?e, "Load operand failed");
                        return Err(e);
                    }
                }

                // Add memory load instruction based on destination type
                if let Some(dest) = instruction.dest {
                    // Get the type of the destination to determine which load instruction to use
                    let dest_type = self
                        .value_to_type
                        .get(&dest)
                        .cloned()
                        .unwrap_or(MirType::I32);

                    match dest_type {
                        MirType::F64 => {
                            self.current_instructions.push(Instruction::F64Load(
                                wasm_encoder::MemArg {
                                    offset: 0,
                                    align: 3, // f64 alignment is 8 bytes (2^3)
                                    memory_index: 0,
                                },
                            ));
                            debug_mir!("Added F64Load instruction");
                        }
                        MirType::F32 => {
                            self.current_instructions.push(Instruction::F32Load(
                                wasm_encoder::MemArg {
                                    offset: 0,
                                    align: 2, // f32 alignment is 4 bytes (2^2)
                                    memory_index: 0,
                                },
                            ));
                            debug_mir!("Added F32Load instruction");
                        }
                        _ => {
                            // Default to I32Load for integer types and pointers
                            self.current_instructions.push(Instruction::I32Load(
                                wasm_encoder::MemArg {
                                    offset: 0,
                                    align: 2, // i32 alignment is 4 bytes (2^2)
                                    memory_index: 0,
                                },
                            ));
                            debug_mir!("Added I32Load instruction");
                        }
                    }

                    match self.store_to_local(dest) {
                        Ok(_) => debug_mir!("Load operation completed successfully"),
                        Err(e) => {
                            debug_mir!(error = ?e, "Failed to store Load result");
                            return Err(e);
                        }
                    }
                } else {
                    // No destination - use I32Load as default and drop the loaded value
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::Store { destination, value } => {
                // Store to memory
                self.load_operand(destination)?;
                self.load_operand(value)?;

                // Determine store instruction based on value type
                let value_type = if let MirOperand::Value(value_id) = value {
                    self.value_to_type
                        .get(value_id)
                        .cloned()
                        .unwrap_or(MirType::I32)
                } else {
                    MirType::I32 // Default for constants and other operands
                };

                match value_type {
                    MirType::F64 => {
                        self.current_instructions.push(Instruction::F64Store(
                            wasm_encoder::MemArg {
                                offset: 0,
                                align: 3, // f64 alignment is 8 bytes (2^3)
                                memory_index: 0,
                            },
                        ));
                    }
                    MirType::F32 => {
                        self.current_instructions.push(Instruction::F32Store(
                            wasm_encoder::MemArg {
                                offset: 0,
                                align: 2, // f32 alignment is 4 bytes (2^2)
                                memory_index: 0,
                            },
                        ));
                    }
                    _ => {
                        // Default to I32Store for integer types and pointers
                        self.current_instructions.push(Instruction::I32Store(
                            wasm_encoder::MemArg {
                                offset: 0,
                                align: 2, // i32 alignment is 4 bytes (2^2)
                                memory_index: 0,
                            },
                        ));
                    }
                }
            }

            MirOperation::Call {
                function,
                arguments,
            } => {
                self.generate_call_instruction(instruction, function, arguments)?;
            }

            MirOperation::GetElementPtr {
                base,
                indices,
                is_array,
            } => {
                debug_mir!(
                    " CODEGEN GEP: base={:?}, indices={:?}, is_array={}",
                    base,
                    indices,
                    is_array
                );
                debug_mir!(
                    "DEBUG CODEGEN GEP: value_to_local map has {} entries",
                    self.value_to_local.len()
                );
                if let MirOperand::Value(vid) = base {
                    debug_mir!(
                        "DEBUG CODEGEN GEP: Looking for base ValueId({}) in value_to_local",
                        vid.0
                    );
                    if self.value_to_local.contains_key(vid) {
                        debug_mir!(
                            "DEBUG CODEGEN GEP: Base ValueId({}) FOUND in value_to_local",
                            vid.0
                        );
                    } else {
                        debug_mir!(
                            "DEBUG CODEGEN GEP: Base ValueId({}) NOT FOUND in value_to_local!",
                            vid.0
                        );
                    }
                }

                tracing::trace!(
                    base = ?base,
                    indices = ?indices,
                    is_array = is_array,
                    "Processing GetElementPtr"
                );

                // Get element pointer for array/struct access
                match self.load_operand(base) {
                    Ok(_) => debug_mir!("Base operand loaded successfully"),
                    Err(e) => {
                        debug_mir!(error = ?e, "Failed to load base operand");
                        return Err(e);
                    }
                }

                // For each index, load it and generate pointer arithmetic
                for (i, index) in indices.iter().enumerate() {
                    debug_mir!(index_num = i, index = ?index, "Processing index");
                    match self.load_operand(index) {
                        Ok(_) => {
                            debug_mir!(index_num = i, "Index loaded successfully");
                            // Calculate element address
                            if *is_array {
                                // For arrays: multiply index by 4 (element size) and add header
                                // Array elements are at array_ptr + 16 + (index * 4)
                                self.current_instructions.push(Instruction::I32Const(4));
                                self.current_instructions.push(Instruction::I32Mul);
                                self.current_instructions.push(Instruction::I32Add);
                                // Clean Language array layout:
                                //   Offset 0-3: Type marker (0)
                                //   Offset 4-7: Array length (i32)
                                //   Offset 8-11: Element size (4)
                                //   Offset 12-15: Unused
                                //   Offset 16+: Elements start here
                                self.current_instructions.push(Instruction::I32Const(16));
                                self.current_instructions.push(Instruction::I32Add);
                            } else {
                                // For class fields: the index IS the byte offset (already calculated)
                                // Just add it directly to the base pointer
                                self.current_instructions.push(Instruction::I32Add);
                            }
                        }
                        Err(e) => {
                            debug_mir!(index_num = i, error = ?e, "Failed to load index");
                            return Err(e);
                        }
                    }
                }

                // Store the calculated address to destination
                if let Some(dest) = instruction.dest {
                    debug_mir!(dest = ?dest, "Storing result to destination");
                    match self.store_to_local(dest) {
                        Ok(_) => debug_mir!("GetElementPtr completed successfully"),
                        Err(e) => {
                            debug_mir!(error = ?e, "Failed to store to destination");
                            return Err(e);
                        }
                    }
                } else {
                    debug_mir!("No destination for GetElementPtr result");
                }
            }

            MirOperation::AsyncAssign { source } => {
                debug_mir!(source = ?source, "Processing AsyncAssign");

                // For async assignments, we load the source value and store it
                // In a full async implementation, this would involve setting up async state
                // For now, we treat it as a regular assignment with future resolution semantics
                self.load_operand(source)?;

                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("AsyncAssign completed successfully");
                } else {
                    // No destination - drop the value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("AsyncAssign: No destination, dropped result");
                }
            }

            MirOperation::BoxAny {
                value,
                type_tag,
                source_type,
            } => {
                debug_mir!(?value, ?type_tag, ?source_type, "Processing BoxAny");

                // Load the value onto the stack
                self.load_operand(value)?;

                // Call the boxing helper
                self.emit_box_value(*type_tag, source_type)?;

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("BoxAny completed successfully");
                } else {
                    // No destination - drop the boxed value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("BoxAny: No destination, dropped result");
                }
            }

            MirOperation::AnyToString { value } => {
                debug_mir!(?value, "Processing AnyToString with type dispatch");

                // Load the boxed any pointer onto the stack
                self.load_operand(value)?;

                // Call the any_to_string helper which does type dispatch
                self.emit_any_to_string()?;

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("AnyToString completed successfully");
                } else {
                    // No destination - drop the string pointer to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("AnyToString: No destination, dropped result");
                }
            }

            MirOperation::UnboxAnyToI32 { value } => {
                debug_mir!(?value, "Processing UnboxAnyToI32");

                // Load the boxed any pointer onto the stack
                self.load_operand(value)?;

                // Unbox to i32
                self.emit_unbox_to_i32()?;

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("UnboxAnyToI32 completed successfully");
                } else {
                    // No destination - drop the unboxed value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("UnboxAnyToI32: No destination, dropped result");
                }
            }

            MirOperation::UnboxAnyToF64 { value } => {
                debug_mir!(?value, "Processing UnboxAnyToF64");

                // Load the boxed any pointer onto the stack
                self.load_operand(value)?;

                // Unbox to f64
                self.emit_unbox_to_f64()?;

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("UnboxAnyToF64 completed successfully");
                } else {
                    // No destination - drop the unboxed value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("UnboxAnyToF64: No destination, dropped result");
                }
            }

            MirOperation::UnboxAnyToBoolean { value } => {
                debug_mir!(?value, "Processing UnboxAnyToBoolean");

                // Load the boxed any pointer onto the stack
                self.load_operand(value)?;

                // Boxed Any layout: [type_tag: i32 @ offset 0] [value: i32/f64 @ offset 4]
                // Type tags: 1 = false, 2 = true, 3 = number, 4 = string, 5 = array, 6 = object
                // For boolean: tag == 2 means true (return 1), anything else involving tag 1 = false
                // Simple approach: read tag, check if tag == 2
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                self.current_instructions.push(Instruction::I32Const(2));
                self.current_instructions.push(Instruction::I32Eq);
                // Result: 1 if tag was 2 (true), 0 otherwise (including tag 1 = false)

                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("UnboxAnyToBoolean completed successfully");
                } else {
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("UnboxAnyToBoolean: No destination, dropped result");
                }
            }

            MirOperation::AnyGetField { object, key } => {
                debug_mir!(?object, ?key, "Processing AnyGetField (JSON object access)");

                // Load the JSON object pointer (Any type)
                self.load_operand(object)?;

                // NOTE: Objects are now boxed as [tag][raw_ptr][0]
                // We need to unbox by reading the raw object pointer at offset 4
                // This extracts the actual object structure pointer from the boxed any value
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));

                // Load the key string and expand to (content_ptr, len) format
                // __json_get_field expects: (raw_object_ptr: i32, key_ptr: i32, key_len: i32)
                self.load_string_argument_for_print(key)?;

                // Call __json_get_field(raw_object_ptr: i32, key_ptr: i32, key_len: i32) -> i32
                let json_get_field_idx = self.get_or_register_json_get_field()?;
                self.current_instructions
                    .push(Instruction::Call(json_get_field_idx));

                // Store result (Any pointer to field value or null) if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("AnyGetField completed successfully");
                } else {
                    // No destination - drop the field value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("AnyGetField: No destination, dropped result");
                }
            }

            MirOperation::AnyGetIndex { array, index } => {
                debug_mir!(?array, ?index, "Processing AnyGetIndex (JSON array access)");

                // Load the JSON array pointer (Any type)
                self.load_operand(array)?;

                // NOTE: Arrays are now boxed as [tag][raw_ptr][0]
                // We need to unbox by reading the raw array pointer at offset 4
                // This extracts the actual array structure pointer from the boxed any value
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));

                // Load the integer index
                self.load_operand(index)?;

                // Call __json_get_index(raw_array_ptr: i32, index: i32) -> i32
                let json_get_index_idx = self.get_or_register_json_get_index()?;
                self.current_instructions
                    .push(Instruction::Call(json_get_index_idx));

                // Store result (Any pointer to element or null) if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("AnyGetIndex completed successfully");
                } else {
                    // No destination - drop the element pointer to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("AnyGetIndex: No destination, dropped result");
                }
            }

            MirOperation::Alloca { size, alignment: _ } => {
                debug_mir!(size = ?size, "Processing Alloca - converting to mem_alloc call");

                // Allocate heap memory by calling mem_alloc
                // mem_alloc signature: (type_id: i32, size: i32) -> i32 (pointer)
                // For class instances, we use type_id = 0 (generic object)

                // Push type_id argument (0 for generic allocation)
                self.current_instructions.push(Instruction::I32Const(0));

                // Push size argument
                self.load_operand(size)?;

                // Get mem_alloc function index from function_map
                let mem_alloc_idx = *self
                    .wasm_generator
                    .function_map
                    .get("mem_alloc")
                    .ok_or_else(|| CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            "mem_alloc function not found in function_map".to_string(),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(instruction.location.clone()),
                        )),
                    })?;

                // Call mem_alloc
                self.current_instructions
                    .push(Instruction::Call(mem_alloc_idx));

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("Alloca completed successfully, stored to {:?}", dest);
                } else {
                    // No destination - drop the allocated pointer to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("Alloca: No destination, dropped result");
                }
            }

            MirOperation::Cast { value, target_type } => {
                debug_mir!(value = ?value, target_type = ?target_type, "Processing Cast");

                // Get the source type by checking value_to_type or inferring from operand
                let source_type = if let MirOperand::Value(vid) = value {
                    self.value_to_type.get(vid).cloned()
                } else {
                    None
                };

                // Load the value onto the stack
                self.load_operand(value)?;

                // Generate appropriate conversion instruction
                match (source_type.as_ref(), target_type) {
                    // Integer to Float conversions
                    (Some(MirType::I32), MirType::F64) | (None, MirType::F64) => {
                        // Convert i32 to f64 (signed conversion)
                        self.current_instructions.push(Instruction::F64ConvertI32S);
                        debug_mir!("Cast: I32 -> F64 using F64ConvertI32S");
                    }

                    // Float to Integer conversions
                    (Some(MirType::F64), MirType::I32) => {
                        // Convert f64 to i32 (truncate)
                        self.current_instructions.push(Instruction::I32TruncF64S);
                        debug_mir!("Cast: F64 -> I32 using I32TruncF64S");
                    }

                    // Same type - no conversion needed
                    (Some(MirType::I32), MirType::I32) | (Some(MirType::F64), MirType::F64) => {
                        debug_mir!("Cast: Same type, no conversion needed");
                    }

                    // Pointer casts - treat as no-op in WASM (all pointers are i32)
                    (Some(MirType::Ptr(_)), MirType::Ptr(_)) => {
                        debug_mir!("Cast: Pointer to pointer, no conversion needed");
                    }

                    // Default: log warning but don't fail
                    _ => {
                        debug_mir!(
                            source = ?source_type,
                            target = ?target_type,
                            "Cast: Unknown type conversion, treating as no-op"
                        );
                    }
                }

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("Cast completed successfully, stored to {:?}", dest);
                } else {
                    // No destination - drop the casted value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("Cast: No destination, dropped result");
                }
            }

            MirOperation::Select {
                condition,
                true_value,
                false_value,
            } => {
                debug_mir!(
                    condition = ?condition,
                    true_value = ?true_value,
                    false_value = ?false_value,
                    "Processing Select operation"
                );

                // WASM select instruction semantics:
                // Pop: condition (i32), val2, val1 (in that order from stack top)
                // Push: val1 if condition != 0, else val2
                //
                // So we push in order: true_value (val1), false_value (val2), condition
                // Result: if condition is true (non-zero), true_value is returned
                //         if condition is false (zero), false_value is returned
                self.load_operand(true_value)?;
                self.load_operand(false_value)?;
                self.load_operand(condition)?;
                self.current_instructions.push(Instruction::Select);

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("Select completed, stored to {:?}", dest);
                } else {
                    // No destination - drop the selected value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("Select: No destination, dropped result");
                }
            }

            MirOperation::GlobalLoad {
                global_id,
                global_type,
            } => {
                debug_mir!(
                    global_id = ?global_id,
                    global_type = ?global_type,
                    "Processing GlobalLoad operation"
                );

                // Look up the global index for this state variable
                if let Some(&global_index) = self.state_global_indices.get(global_id) {
                    self.current_instructions
                        .push(Instruction::GlobalGet(global_index));
                    debug_mir!(
                        global_index = global_index,
                        "Emitted GlobalGet for state variable"
                    );

                    // Store result if there's a destination
                    if let Some(dest) = instruction.dest {
                        self.store_to_local(dest)?;
                        debug_mir!("GlobalLoad completed, stored to {:?}", dest);
                    }
                } else {
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            format!(
                                "State variable global not found for SymbolId {:?}",
                                global_id
                            ),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(instruction.location.clone()),
                        )),
                    });
                }
            }

            MirOperation::GlobalStore {
                global_id,
                value,
                global_type,
            } => {
                debug_mir!(
                    global_id = ?global_id,
                    value = ?value,
                    global_type = ?global_type,
                    "Processing GlobalStore operation"
                );

                // Look up the global index for this state variable
                if let Some(&global_index) = self.state_global_indices.get(global_id) {
                    // Load the value to store
                    self.load_operand(value)?;

                    // Store to global
                    self.current_instructions
                        .push(Instruction::GlobalSet(global_index));
                    debug_mir!(
                        global_index = global_index,
                        "Emitted GlobalSet for state variable"
                    );
                } else {
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            format!(
                                "State variable global not found for SymbolId {:?}",
                                global_id
                            ),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(instruction.location.clone()),
                        )),
                    });
                }
            }

            _ => {
                // Unsupported MIR operation
                return Err(CompilerError::Codegen {
                    context: Box::new(crate::error::ErrorContext::new(
                        format!(
                            "MIR operation not yet implemented: {:?}",
                            instruction.operation
                        ),
                        None,
                        crate::error::ErrorType::Codegen,
                        Some(instruction.location.clone()),
                    )),
                });
            }
        }

        Ok(())
    }

    /// Generate WASM terminator instruction.
    #[allow(dead_code)] // Used internally by generate_basic_block
    pub(super) fn generate_terminator(
        &mut self,
        terminator: &MirTerminator,
    ) -> Result<(), CompilerError> {
        match terminator {
            MirTerminator::Return { value } => {
                if let Some(return_value) = value {
                    // Don't load undefined values - they represent void returns
                    if !matches!(return_value, MirOperand::Constant(MirConstant::Undefined)) {
                        // NOTE: Removed StringTuple expansion logic
                        // Since ConcreteType::String now maps to MirType::I32, strings are single i32 pointers
                        // No expansion needed - just load the operand directly
                        self.load_operand(return_value)?;
                    }
                }
                self.current_instructions.push(Instruction::Return);
            }

            MirTerminator::Jump { target } => {
                // Fallthrough to next block (structured control flow handled by block ordering)
                debug_mir!("DEBUG MIR: Skipping Jump to {:?} (fallthrough)", target);
            }

            MirTerminator::Branch {
                condition,
                true_block,
                false_block,
            } => {
                // Evaluate condition (if/else structure handled by generate_branch_block)
                self.load_operand(condition)?;
                // Pop the condition value since we're not using it
                self.current_instructions.push(Instruction::Drop);
                debug_mir!(
                    "DEBUG MIR: Skipping Branch to {:?}/{:?} (fallthrough)",
                    true_block,
                    false_block
                );
            }

            MirTerminator::Unreachable => {
                self.current_instructions.push(Instruction::Unreachable);
            }

            MirTerminator::Trap => {
                // Contract violation trap - generates WASM unreachable
                self.current_instructions.push(Instruction::Unreachable);
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Call instruction helper (extracted from generate_instruction to reduce nesting)
    // -----------------------------------------------------------------------

    fn generate_call_instruction(
        &mut self,
        instruction: &MirInstruction,
        function: &MirOperand,
        arguments: &[MirOperand],
    ) -> Result<(), CompilerError> {
        debug_mir!(
            "DEBUG CALL START: function={:?}, arguments_len={}",
            function,
            arguments.len()
        );

        tracing::trace!(
            function = ?function,
            arguments = arguments.len(),
            "Processing Call operation"
        );

        // Flag to track if print calls were already emitted (for multi-arg print)
        let mut call_already_emitted = false;

        // Get function signature to determine parameter types
        let (mut function_name, function_signature, symbol_id_opt) = match function {
            MirOperand::Function(symbol_id) => {
                debug_mir!(" CALL SYMBOL: SymbolId({})", symbol_id.0);
                let name = self.get_function_name_by_symbol(*symbol_id);
                debug_mir!(" CALL NAME FROM SYMBOL: {:?}", name);
                let sig = self.function_signatures.get(symbol_id).cloned();
                (name, sig, Some(*symbol_id))
            }
            MirOperand::NamedFunction { name, symbol_id } => {
                debug_mir!(
                    "DEBUG CALL NAMED FUNCTION: name='{}', SymbolId({})",
                    name,
                    symbol_id.0
                );
                // NOTE: For namespace functions (SymbolId(0)), don't use the signature
                // because SymbolId(0) is shared by all namespace functions and maps to "print"
                // which has a Void return type. This causes namespace functions like list.add
                // to incorrectly be treated as void functions.
                let sig = if symbol_id.0 == 0 {
                    None // Don't use signature for namespace functions
                } else {
                    self.function_signatures.get(symbol_id).cloned()
                };
                (Some(name.clone()), sig, Some(*symbol_id))
            }
            _ => (None, None, None),
        };

        // NOTE: For stdlib namespace functions (SymbolId(0)), try reverse lookup
        // NamedFunction operands already have the correct name, so skip reverse lookup for them
        // Only do reverse lookup for plain Function operands with missing/wrong names
        let needs_reverse_lookup = matches!(function, MirOperand::Function(_))
            && (function_name.is_none()
                || (symbol_id_opt.is_some_and(|id| id.0 == 0)
                    && function_name.as_deref() == Some("print")));

        if needs_reverse_lookup {
            if let MirOperand::Function(symbol_id) = function {
                if let Some(&function_index) = self.symbol_to_function_index.get(symbol_id) {
                    // Reverse-lookup: find the function name that maps to this index
                    for (name, &index) in &self.wasm_generator.function_map {
                        if index == function_index {
                            debug_mir!(
                                "DEBUG REVERSE LOOKUP: SymbolId({}) -> index {} -> name '{}'",
                                symbol_id.0,
                                function_index,
                                name
                            );
                            function_name = Some(name.clone());
                            break;
                        }
                    }
                }
            }
        }

        debug_mir!(function_name = ?function_name, "Function name resolved");

        // NOTE: String expansion should only happen for built-in functions
        // User-defined functions receive string pointers (to [len|content] structure)
        // Functions that need string arguments expanded to (content_ptr, len)
        debug_mir!(
            "DEBUG FUNCTION MATCH: function_name={:?}, arguments={}",
            function_name,
            arguments.len()
        );
        match function_name.as_deref() {
            Some("print") | Some("printl") => {
                debug_mir!(": Matched print function, loading {} arguments", arguments.len());
                // NOTE: For multi-argument print, we must call print ONCE PER ARGUMENT
                // The print function takes (content_ptr, length) - only 2 params
                // So print("Value:", x) should emit TWO print calls, not one

                // Get the print function index once
                let print_func_name = function_name.as_deref().unwrap_or("print");
                let print_idx = *self.wasm_generator.function_map.get(print_func_name)
                    .ok_or_else(|| CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            format!("Print function '{}' not found in function map", print_func_name),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(instruction.location.clone()),
                        )),
                    })?;

                for (i, arg) in arguments.iter().enumerate() {
                    debug_mir!(": Loading print arg[{}]: {:?}", i, arg);
                    // Load this argument's (content_ptr, length) onto stack
                    self.load_string_argument_for_print(arg)?;
                    // Call print immediately for this argument
                    self.current_instructions.push(Instruction::Call(print_idx));
                    debug_mir!(": Called print for arg[{}]", i);
                }

                // Mark that we've already emitted the print calls
                call_already_emitted = true;
            }
            Some("string.concat") | Some("string_concat") | Some("native_string_concat") => {
                debug_mir!(": Matched string.concat");
                // FIXED: native_string_concat expects 2 i32 arguments: (str_ptr1, str_ptr2) -> result_ptr
                // Each pointer points to a string structure: [4-byte length][data bytes]
                // DO NOT expand to (ptr, len) pairs - just pass the struct pointers
                for arg in arguments {
                    self.load_string_pointer_only(arg)?;
                }
            }
            Some("input")
            | Some("input_string")
            | Some("input_integer")
            | Some("input_float")
            | Some("input_yesno")
            | Some("input.integer")  // Dot notation variants
            | Some("input.float")
            | Some("input.yesNo") => {
                debug_mir!(": Matched input function - using load_string_pointer_only");
                // FIXED: Input functions now expect only (prompt_ptr) -> result
                // They no longer take length parameter
                for arg in arguments {
                    self.load_string_pointer_only(arg)?;
                }
            }
            Some("input_range") => {
                // input_range expects (prompt_ptr, prompt_len, min, max) -> result
                // Only expand the first argument (prompt string)
                if !arguments.is_empty() {
                    self.load_string_argument_for_print(&arguments[0])?;
                    // Load remaining arguments normally (min, max)
                    for arg in &arguments[1..] {
                        self.load_operand(arg)?;
                    }
                }
            }
            // REMOVED: Hardcoded string expansion for bridge functions
            // Bridge functions with expand_strings=true now use wrapper functions
            // that handle the expansion automatically. The wrapper receives original
            // Clean Language string pointers and expands them to (ptr+4, len) pairs.
            // No special handling needed here - just use normal load_operand.
            Some(name) if name.starts_with("math.") => {
                // NOTE: Math functions expect f64 parameters
                // Convert i32 (integer) arguments to f64 (number) automatically
                for arg in arguments {
                    self.load_operand(arg)?;
                    // Check if this is an integer constant or integer value
                    // For now, assume integers need conversion (MIR should track this properly)
                    // Integer constants and values default to i32, math functions expect f64
                    if matches!(arg, MirOperand::Constant(MirConstant::Integer(_))) {
                        // Convert i32 to f64
                        self.current_instructions.push(Instruction::F64ConvertI32S);
                    } else if let Some(MirOperand::Value(value_id)) = Some(arg) {
                        // Check if the value type is an integer type
                        if let Some(mir_type) = self.value_to_type.get(value_id) {
                            if matches!(
                                mir_type,
                                MirType::I32
                                    | MirType::I8
                                    | MirType::I16
                                    | MirType::U8
                                    | MirType::U16
                                    | MirType::U32
                            ) {
                                self.current_instructions.push(Instruction::F64ConvertI32S);
                            }
                        }
                    }
                }
            }
            Some(name)
                if name.starts_with("number")
                    || name == "float_to_string"
                    || name == "integer.toNumber" =>
            {
                // NOTE: number conversion functions expect f64 parameters
                // number_to_string, float_to_string, etc. expect f64
                // Convert i32 (integer) arguments to f64 (number) automatically
                // NOTE: string.toNumber and boolean.toNumber take i32 inputs (pointer/boolean)
                // and should NOT be converted - only integer.toNumber needs conversion
                for arg in arguments {
                    self.load_operand(arg)?;
                    if matches!(arg, MirOperand::Constant(MirConstant::Integer(_))) {
                        self.current_instructions.push(Instruction::F64ConvertI32S);
                    } else if let Some(MirOperand::Value(value_id)) = Some(arg) {
                        if let Some(mir_type) = self.value_to_type.get(value_id) {
                            if matches!(
                                mir_type,
                                MirType::I32
                                    | MirType::I8
                                    | MirType::I16
                                    | MirType::U8
                                    | MirType::U16
                                    | MirType::U32
                            ) {
                                self.current_instructions.push(Instruction::F64ConvertI32S);
                            }
                        }
                    }
                }
            }
            Some("string.toNumber") | Some("boolean.toNumber") => {
                // string.toNumber takes i32 (string pointer) -> f64
                // boolean.toNumber takes i32 (boolean value) -> f64
                // Do NOT convert the i32 argument to f64
                for arg in arguments {
                    self.load_operand(arg)?;
                }
            }
            _ => {
                // For user-defined functions and other built-ins, load arguments with automatic type conversion
                // String parameters are passed as pointers to [len|content] structure
                debug_mir!(
                    "DEBUG CALL ARGS: Loading {} arguments for function {:?}",
                    arguments.len(),
                    function_name
                );

                // Check if we have function signature to enable automatic type conversion
                let param_types = function_signature.as_ref().map(|sig| &sig.parameters);

                // Check if this is a bridge function call with handler parameters
                let bridge_handler_params = self.get_bridge_handler_params(function_name.as_deref());

                for (i, arg) in arguments.iter().enumerate() {
                    debug_mir!(" CALL ARGS:   Arg[{}]: {:?}", i, arg);

                    // Handler parameter: resolve function reference to handler index
                    if bridge_handler_params.as_ref().is_some_and(|params| {
                        i < params.len() && matches!(params[i], crate::builtins::registry::BuiltinType::Handler)
                    }) {
                        let handler_index = self.resolve_handler_argument(arg)?;
                        debug_mir!(
                            " CALL ARGS:   Arg[{}] is handler, resolved to index {}",
                            i, handler_index
                        );
                        // u32::MAX sentinel means resolve_handler_argument already
                        // pushed the value onto the stack (Value operand case)
                        if handler_index != u32::MAX {
                            self.current_instructions.push(Instruction::I32Const(handler_index as i32));
                        }
                        continue;
                    }

                    self.load_operand(arg)?;

                    // Automatic type conversion: if parameter expects f64 but we have i32, convert
                    if let Some(params) = param_types {
                        if i < params.len() {
                            let expected_param = &params[i];

                            // Check if parameter expects f64
                            if matches!(expected_param.param_type, MirType::F64) {
                                // Check if argument is integer type
                                let arg_is_int = match arg {
                                    MirOperand::Constant(MirConstant::Integer(_)) => true,
                                    MirOperand::Value(value_id) => self
                                        .value_to_type
                                        .get(value_id)
                                        .is_some_and(|t| {
                                            matches!(
                                                t,
                                                MirType::I32
                                                    | MirType::I8
                                                    | MirType::I16
                                                    | MirType::U8
                                                    | MirType::U16
                                                    | MirType::U32
                                            )
                                        }),
                                    _ => false,
                                };

                                if arg_is_int {
                                    debug_mir!(
                                        "DEBUG CALL ARGS:   Converting i32 arg[{}] to f64",
                                        i
                                    );
                                    self.current_instructions
                                        .push(Instruction::F64ConvertI32S);
                                }
                            }

                            // Check if parameter expects Any type
                            // For now, Any type accepts any i32 value (integer, boolean, pointer)
                            // f64 values need to be converted to i32 (truncated)
                            if matches!(expected_param.param_type, MirType::Any) {
                                // Get the argument's actual type
                                let arg_type = match arg {
                                    MirOperand::Constant(MirConstant::Float(_)) => Some(MirType::F64),
                                    MirOperand::Value(value_id) => self.value_to_type.get(value_id).cloned(),
                                    _ => None,
                                };

                                // If argument is f64, convert to i32 (truncate)
                                // This is a limitation - proper boxing would preserve the f64
                                if let Some(ref actual_type) = arg_type {
                                    if matches!(actual_type, MirType::F64) {
                                        debug_mir!(
                                            "DEBUG CALL ARGS:   Converting f64 arg[{}] to i32 for any type",
                                            i
                                        );
                                        self.current_instructions.push(Instruction::I32TruncF64S);
                                    }
                                }
                            }
                        }
                    }

                    debug_mir!(" CALL ARGS:   Arg[{}] loaded successfully", i);
                }
                debug_mir!(
                    "DEBUG CALL ARGS: Finished loading all {} arguments",
                    arguments.len()
                );
            }
        }

        // Generate function call (skip if already emitted for multi-arg print)
        if !call_already_emitted {
            match function {
                MirOperand::Function(symbol_id) => {
                    // NOTE: Try direct SymbolId -> index lookup first
                    // This avoids name collisions for constructors/methods with same names
                    if let Some(&function_index) = self.symbol_to_function_index.get(symbol_id) {
                        debug_mir!(
                            "DEBUG DIRECT LOOKUP: SymbolId({}) -> WASM index {} (DIRECT)",
                            symbol_id.0,
                            function_index
                        );
                        tracing::trace!(
                            symbol_id = symbol_id.0,
                            index = function_index,
                            "Calling function at WASM index (direct lookup)"
                        );
                        self.current_instructions
                            .push(Instruction::Call(function_index));
                    } else if let Some(function_name) = self.get_function_name_by_symbol(*symbol_id)
                    {
                        // Fallback to name-based lookup for built-in functions
                        debug_mir!(
                            "DEBUG LOOKUP: Looking up function '{}' in function_map",
                            function_name
                        );

                        // Try direct lookup first
                        let function_index = if let Some(&idx) =
                            self.wasm_generator.function_map.get(&function_name)
                        {
                            Some(idx)
                        } else {
                            // NOTE: Try underscore/dot conversion first
                            // "math_round" -> "math.round" or vice versa
                            let alt_name = if function_name.contains('_') {
                                function_name.replace('_', ".")
                            } else if function_name.contains('.') {
                                function_name.replace('.', "_")
                            } else {
                                String::new()
                            };

                            if !alt_name.is_empty() {
                                if let Some(&idx) = self.wasm_generator.function_map.get(&alt_name)
                                {
                                    debug_mir!(
                                        "DEBUG LOOKUP FALLBACK: Found '{}' as '{}'",
                                        function_name,
                                        alt_name
                                    );
                                    Some(idx)
                                } else {
                                    // Try namespace-prefixed variants for builtin functions
                                    // If "min" is not found, try "math.min", "string.min", etc.
                                    let namespaces = [
                                        "math",
                                        "string",
                                        "list",
                                        "file",
                                        "http",
                                        "compare",
                                        "conditional",
                                    ];
                                    namespaces.iter().find_map(|ns| {
                                        let qualified_name = format!("{}.{}", ns, function_name);
                                        debug_mir!(
                                            "DEBUG LOOKUP FALLBACK: Trying '{}'",
                                            qualified_name
                                        );
                                        self.wasm_generator
                                            .function_map
                                            .get(&qualified_name)
                                            .copied()
                                    })
                                }
                            } else {
                                // Try namespace-prefixed variants for builtin functions
                                let namespaces = [
                                    "math",
                                    "string",
                                    "list",
                                    "file",
                                    "http",
                                    "compare",
                                    "conditional",
                                ];
                                namespaces.iter().find_map(|ns| {
                                    let qualified_name = format!("{}.{}", ns, function_name);
                                    self.wasm_generator
                                        .function_map
                                        .get(&qualified_name)
                                        .copied()
                                })
                            }
                        }
                        // Bridge function lookup: "req.body" → "_req_body"
                        .or_else(|| {
                            self.language_to_bridge_map.get(&function_name).and_then(
                                |bridge_name| {
                                    self.wasm_generator.function_map.get(bridge_name).copied()
                                },
                            )
                        });

                        if let Some(function_index) = function_index {
                            tracing::trace!(
                                name = %function_name,
                                index = function_index,
                                "Calling function at WASM index"
                            );
                            self.current_instructions
                                .push(Instruction::Call(function_index));
                        } else {
                            // NOTE: No more silent fallbacks to index 0
                            // Return a proper error when function is not found in function_map
                            debug_mir!(
                                "DEBUG LOOKUP: Function '{}' not found in function_map!",
                                function_name
                            );
                            debug_mir!(
                                "DEBUG LOOKUP: function_map keys: {:?}",
                                self.wasm_generator.function_map.keys().collect::<Vec<_>>()
                            );
                            return Err(CompilerError::Codegen {
                                context: Box::new(crate::error::ErrorContext::new(
                                    format!(
                                        "Function '{}' (SymbolId({})) not found in function map during code generation",
                                        function_name, symbol_id.0
                                    ),
                                    None,
                                    crate::error::ErrorType::Codegen,
                                    Some(instruction.location.clone()),
                                )),
                            });
                        }
                    } else {
                        // NOTE: No more silent fallbacks to index 0
                        // Return a proper error when symbol ID cannot be resolved to a function name
                        return Err(CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                format!(
                                    "Cannot resolve SymbolId({}) to function name during code generation",
                                    symbol_id.0
                                ),
                                None,
                                crate::error::ErrorType::Codegen,
                                Some(instruction.location.clone()),
                            )),
                        });
                    }
                }
                MirOperand::NamedFunction { name, symbol_id: _ } => {
                    debug_mir!(
                        "DEBUG NAMED FUNCTION: Looking up function '{}' in function_map",
                        name
                    );

                    // Try direct lookup first
                    let function_index =
                        if let Some(&idx) = self.wasm_generator.function_map.get(name) {
                            Some(idx)
                        } else {
                            // Try underscore/dot conversion
                            // "input.integer" -> "input_integer" or vice versa
                            let alt_name = if name.contains('.') {
                                name.replace('.', "_")
                            } else if name.contains('_') {
                                name.replace('_', ".")
                            } else {
                                String::new()
                            };

                            if !alt_name.is_empty() {
                                self.wasm_generator.function_map.get(&alt_name).copied()
                            } else {
                                None
                            }
                        }
                        // Imported module call fallback: "ModuleName.function" → "function"
                        // When modules are merged into a single compilation unit, their
                        // functions are registered without the module prefix. Strip the
                        // first qualifier and look up the bare function name.
                        .or_else(|| {
                            if let Some(dot_pos) = name.find('.') {
                                let bare_name = &name[dot_pos + 1..];
                                self.wasm_generator.function_map.get(bare_name).copied()
                            } else {
                                None
                            }
                        })
                        // Bridge function lookup: "req.body" → "_req_body" via language_to_bridge_map
                        .or_else(|| {
                            self.language_to_bridge_map
                                .get(name)
                                .and_then(|bridge_name| {
                                    self.wasm_generator.function_map.get(bridge_name).copied()
                                })
                        });

                    if let Some(idx) = function_index {
                        debug_mir!(
                            "DEBUG NAMED FUNCTION CALL: Calling '{}' at WASM index {}",
                            name,
                            idx
                        );
                        tracing::trace!(
                            name = %name,
                            index = idx,
                            "Calling named function at WASM index"
                        );
                        self.current_instructions.push(Instruction::Call(idx));
                    } else {
                        // NOTE: Return a proper error when named function is not found
                        debug_mir!(
                            "DEBUG NAMED FUNCTION: Function '{}' not found in function_map!",
                            name
                        );
                        debug_mir!(
                            "DEBUG NAMED FUNCTION: Available functions: {:?}",
                            self.wasm_generator.function_map.keys().collect::<Vec<_>>()
                        );
                        return Err(CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                format!("Function '{}' not found in function map", name),
                                None,
                                crate::error::ErrorType::Codegen,
                                Some(instruction.location.clone()),
                            )),
                        });
                    }
                }
                _ => {
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            "Indirect function calls not yet supported",
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(instruction.location.clone()),
                        )),
                    });
                }
            }
        } // End of if !call_already_emitted

        // NOTE: Handle return values based on function signature
        debug_mir!(" CALL: Call operation completed");
        debug_mir!(
            "DEBUG CALL: function_name={:?}, has_dest={}",
            function_name,
            instruction.dest.is_some()
        );

        if let Some(dest) = instruction.dest {
            debug_mir!(" CALL DEST: Processing call with dest={:?}", dest);
            debug_mir!(" CALL DEST: function_name={:?}", function_name);

            // NOTE: Type conversion (F64 to I32) is handled by store_to_local_with_conversion
            // which is called in the signature/stdlib handling below. DO NOT add redundant
            // conversion here - it causes double truncation errors.

            if let Some(signature) = &function_signature {
                // Check if dest_type is Any or Ptr(Void) (unknown/dynamic type)
                // If instruction has a dest, the function returns a value
                let dest_type = self.value_to_type.get(&dest);
                let is_any_or_ptr_void = matches!(dest_type, Some(MirType::Any))
                    || matches!(dest_type, Some(MirType::Ptr(inner)) if matches!(**inner, MirType::Void));

                if is_any_or_ptr_void {
                    // Any/Ptr(Void) dest_type means dynamic type — use signature to determine handling
                    debug_mir!(
                        " SIG VOID: Unknown type dest {:?}, signature return type: {:?}",
                        dest,
                        signature.return_type
                    );

                    // Check if this is a known void function using the return type registry
                    let is_known_void_by_name = function_name
                        .as_deref()
                        .and_then(|name| self.function_return_types.get(name))
                        .map_or(false, |rt| matches!(rt, MirType::Void));

                    if is_known_void_by_name {
                        debug_mir!("DEBUG SIG VOID: Known void function by name - no DROP needed");
                    } else {
                        match &signature.return_type {
                            MirType::Void => {
                                // Function truly returns nothing - no DROP needed
                                debug_mir!(
                                    "DEBUG SIG VOID: Function returns Void - no drop needed"
                                );
                            }
                            _ => {
                                // NOTE: Ptr(Void) represents Any type which CAN hold return values
                                // Store the value to the local - Any type can store any pointer/value
                                debug_mir!(
                                    "DEBUG SIG VOID: Storing return value (type: {:?}) to Any type dest",
                                    signature.return_type
                                );
                                self.store_to_local_with_conversion(
                                    dest,
                                    Some(signature.return_type.clone()),
                                )?;
                            }
                        }
                    }
                } else {
                    match &signature.return_type {
                        MirType::Void => {
                            // NOTE: Void return type in signature means no value on stack
                            // No DROP needed - the function truly returns nothing
                            tracing::trace!(
                                function_name = ?function_name,
                                "Void function - no return value to store or drop"
                            );
                        }
                        MirType::StringTuple => {
                            // StringTuple functions return a SINGLE i32 pointer
                            // The pointer references memory formatted as: [4-byte length][content bytes]
                            // Just store the pointer directly - no Drop needed
                            tracing::trace!(
                                function_name = ?function_name,
                                "Handling StringTuple return (storing single i32 pointer)"
                            );

                            self.store_to_local_with_conversion(
                                dest,
                                Some(signature.return_type.clone()),
                            )?;

                            tracing::trace!("Stored StringTuple return as single i32 pointer");
                        }
                        _ => {
                            // Regular single-value return - with type conversion if needed
                            // NOTE: Pass return type for automatic f64->i32 conversion
                            self.store_to_local_with_conversion(
                                dest,
                                Some(signature.return_type.clone()),
                            )?;
                        }
                    }
                }
            } else {
                // Fallback: no signature available from SymbolId lookup
                // NOTE: Try looking up return type by function name for stdlib functions
                let stdlib_return_type = function_name
                    .as_ref()
                    .and_then(|name| self.get_stdlib_return_type(name));

                if let Some(return_type) = stdlib_return_type {
                    // Found stdlib return type - use it for type conversion
                    if !matches!(return_type, MirType::Void) {
                        debug_mir!(
                            "DEBUG STDLIB: Found return type {:?} for function {:?}",
                            return_type,
                            function_name
                        );
                        self.store_to_local_with_conversion(dest, Some(return_type))?;
                    }
                    // Void functions don't store anything
                } else if let Some(dest_type) = self.value_to_type.get(&dest) {
                    debug_mir!(
                        "DEBUG VOID CHECK: dest={:?}, dest_type={:?}, function={:?}",
                        dest,
                        dest_type,
                        function_name
                    );
                    let is_any_or_ptr_void = matches!(dest_type, MirType::Any)
                        || matches!(dest_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void));

                    if is_any_or_ptr_void {
                        debug_mir!(
                            "DEBUG ANY DEST: Storing value to Any/dynamic type dest {:?}",
                            dest
                        );
                    }
                    // Any, Ptr(Void), and concrete types all store the return value
                    self.store_to_local(dest)?;
                } else {
                    debug_mir!(
                        "DEBUG VOID CHECK: dest={:?} not found in value_to_type",
                        dest
                    );
                    // Last resort: check if this is a known void-returning built-in function
                    if let Some(function_name) = &function_name {
                        if function_name == "testFunction"
                            || function_name == "print"
                            || function_name == "printl"
                            || function_name == "list.set"
                            || function_name == "list.clear"
                        {
                            tracing::trace!(
                                name = %function_name,
                                "Skipping return value store for known void function"
                            );
                        } else {
                            self.store_to_local(dest)?;
                        }
                    } else {
                        self.store_to_local(dest)?;
                    }
                }
            }
        } else {
            // NOTE: Handle calls with no destination (expression statements)
            // For non-void functions, we need to DROP the return value to clean up the stack
            debug_mir!(" CALL NO DEST: Call has no destination, checking if return value needs to be dropped");

            // Check if this function returns void (no cleanup needed)
            let is_void_return = if let Some(signature) = &function_signature {
                debug_mir!(
                    "DEBUG CALL NO DEST: Found signature, return_type={:?}",
                    signature.return_type
                );
                // Check for void return types (Void or legacy Ptr(Void))
                matches!(signature.return_type, MirType::Void)
                    || matches!(&signature.return_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void))
            } else {
                debug_mir!("DEBUG CALL NO DEST: No signature found, checking fallback logic");
                // Fallback: check known void functions by name
                // These are builtin/stdlib functions that return nothing (modify in-place or have side effects only)
                // NOTE: list.push is NOT void - it returns the list for chaining
                let is_known_void_builtin = match function_name.as_deref() {
                    Some("print") | Some("printl") => true,
                    Some("list.set") | Some("list.clear") => true,
                    Some("mem_release") | Some("mem_retain") => true,
                    Some("mem_scope_push") | Some("mem_scope_pop") => true,
                    _ => false,
                };

                if is_known_void_builtin {
                    debug_mir!(
                        " CALL NO DEST: Known void built-in function: {:?}",
                        function_name
                    );
                    true
                } else {
                    // NOTE: For functions without signatures called as expression statements,
                    // default to NON-VOID (add DROP) to prevent stack pollution
                    debug_mir!(" CALL NO DEST: Unknown function without signature, defaulting to non-void (adding DROP for safety)");
                    false
                }
            };

            if !is_void_return {
                debug_mir!(" CALL NO DEST: Non-void function, adding DROP instruction");
                // Function returns a value but we're not using it (expression statement)
                // Drop the return value from the stack
                self.current_instructions.push(Instruction::Drop);
                tracing::trace!(
                    function_name = ?function_name,
                    "Dropped unused return value for call with no destination"
                );
            } else {
                debug_mir!(" CALL NO DEST: Void function, no DROP needed");
                tracing::trace!(
                    function_name = ?function_name,
                    "No DROP needed for void function call"
                );
            }
        }

        debug_mir!("DEBUG MIR: Call operation processing completed");
        Ok(())
    }

    /// Look up bridge function parameter types for handler detection.
    pub(super) fn get_bridge_handler_params(
        &self,
        function_name: Option<&str>,
    ) -> Option<Vec<crate::builtins::registry::BuiltinType>> {
        let name = function_name?;

        // Direct bridge function name lookup (e.g., "_ui_onEvent")
        if let Some(params) = self.bridge_param_types.get(name) {
            return Some(params.clone());
        }

        // Language name → bridge name lookup (e.g., "ui.onEvent" → "_ui_onEvent")
        if let Some(bridge_name) = self.language_to_bridge_map.get(name) {
            if let Some(params) = self.bridge_param_types.get(bridge_name) {
                return Some(params.clone());
            }
        }

        // Try underscore/dot conversion (e.g., "ui_onEvent" → "_ui_onEvent")
        let alt_name = if name.contains('.') {
            format!("_{}", name.replace('.', "_"))
        } else if name.contains('_') && !name.starts_with('_') {
            format!("_{}", name)
        } else {
            return None;
        };
        self.bridge_param_types.get(&alt_name).cloned()
    }

    /// Resolve a handler argument (function reference) to a handler index.
    pub(super) fn resolve_handler_argument(
        &mut self,
        arg: &MirOperand,
    ) -> Result<u32, CompilerError> {
        // Extract the function name from the operand
        let handler_name = match arg {
            MirOperand::Function(symbol_id) => {
                self.get_function_name_by_symbol(*symbol_id)
                    .ok_or_else(|| CompilerError::codegen_error(
                        format!(
                            "Handler function SymbolId({}) not found — did you define it in the functions: block?",
                            symbol_id.0
                        ),
                        None,
                        None,
                    ))?
            }
            MirOperand::NamedFunction { name, .. } => name.clone(),
            MirOperand::Constant(MirConstant::Integer(n)) => {
                // Already a literal integer — pass through as-is
                return Ok(*n as u32);
            }
            MirOperand::Value(value_id) => {
                // The MIR builder emits function references as Value(ValueId) with a
                // local named "funcref_<function_name>". Detect this pattern and
                // resolve the function name to a handler index, matching the
                // Function operand branch below.
                let local_name = self
                    .current_function
                    .as_ref()
                    .and_then(|f| f.locals.get(value_id))
                    .and_then(|l| l.name.as_ref())
                    .cloned();

                if let Some(name) = local_name {
                    if let Some(func_name) = name.strip_prefix("funcref_") {
                        // Function reference — assign a handler index like Function branch
                        let handler_name = func_name.to_string();
                        if let Some(&index) = self.handler_indices.get(&handler_name) {
                            return Ok(index);
                        }
                        let index = self.next_handler_index;
                        self.next_handler_index += 1;
                        self.handler_indices.insert(handler_name.clone(), index);
                        tracing::debug!(
                            handler = %handler_name,
                            index = index,
                            "Assigned handler index from funcref Value (will export as handle_event_{})",
                            index
                        );
                        return Ok(index);
                    }
                }

                // Plugin-generated code like `_http_route("GET", "/", 0)` produces
                // Value(ValueId) wrapping literal integers. Load the integer value
                // from the local and let the runtime use it as a handler index.
                if let Some(&local_index) = self.value_to_local.get(value_id) {
                    self.current_instructions
                        .push(Instruction::LocalGet(local_index));
                    // Return sentinel to tell caller value is already on stack
                    return Ok(u32::MAX);
                }
                return Err(CompilerError::codegen_error(
                    format!(
                        "Handler argument Value({:?}) not found in locals",
                        value_id
                    ),
                    None,
                    None,
                ));
            }
            other => {
                return Err(CompilerError::codegen_error(
                    format!(
                        "Expected function name for handler parameter, got {:?}. Pass a function name like 'myHandler' instead of a value.",
                        other
                    ),
                    None,
                    None,
                ));
            }
        };

        // Check if this handler was already assigned an index
        if let Some(&index) = self.handler_indices.get(&handler_name) {
            return Ok(index);
        }

        // Assign new handler index
        let index = self.next_handler_index;
        self.next_handler_index += 1;
        self.handler_indices.insert(handler_name.clone(), index);

        tracing::debug!(
            handler = %handler_name,
            index = index,
            "Assigned handler index (will export as handle_event_{})",
            index
        );

        Ok(index)
    }

    // -----------------------------------------------------------------------
    // Binary/unary operation helpers
    // -----------------------------------------------------------------------

    /// Generate WASM binary operation (type-aware).
    pub(super) fn generate_binary_operation(
        &mut self,
        op: &MirBinaryOp,
        left: &MirOperand,
        right: &MirOperand,
    ) -> Result<(), CompilerError> {
        // Determine if we're working with floats by checking operand types
        let is_float = self.is_float_operand(left) || self.is_float_operand(right);
        // Determine if we're working with strings
        let is_string = self.is_string_operand(left) || self.is_string_operand(right);

        // Handle string comparison specially - call string_compare function
        if is_string && matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne) {
            // Get the string_compare function index
            if let Some(compare_idx) = self.wasm_generator.get_function_index("string_compare") {
                // Call string_compare(left, right) - operands are already on stack
                // string_compare returns 0 for equal, non-zero for not-equal
                self.current_instructions
                    .push(Instruction::Call(compare_idx));
                // For Equal: string_compare returns 0 when equal, but WASM `if` needs
                // non-zero for true, so we invert with i32.eqz (0 → 1, non-zero → 0)
                if matches!(op, MirBinaryOp::Eq) {
                    self.current_instructions.push(Instruction::I32Eqz);
                }
                // For NotEqual: string_compare already returns non-zero when not equal,
                // which is exactly what WASM `if` expects — no inversion needed
                return Ok(());
            }
            // Fall through to pointer comparison if string_compare not available
        }

        let instruction = match op {
            // Arithmetic operations
            MirBinaryOp::Add => {
                if is_float {
                    Instruction::F64Add
                } else {
                    Instruction::I32Add
                }
            }
            MirBinaryOp::Sub => {
                if is_float {
                    Instruction::F64Sub
                } else {
                    Instruction::I32Sub
                }
            }
            MirBinaryOp::Mul => {
                if is_float {
                    Instruction::F64Mul
                } else {
                    Instruction::I32Mul
                }
            }
            MirBinaryOp::Div => {
                if is_float {
                    Instruction::F64Div
                } else {
                    Instruction::I32DivS
                }
            }
            MirBinaryOp::Rem => {
                if is_float {
                    // F64 doesn't have remainder, use modulo semantics (not perfect but functional)
                    // For proper implementation, this should call a helper function
                    Instruction::I32RemS // Fallback - this will cause type errors on f64
                } else {
                    Instruction::I32RemS
                }
            }

            // Comparison operations
            MirBinaryOp::Eq => {
                if is_float {
                    Instruction::F64Eq
                } else {
                    Instruction::I32Eq
                }
            }
            MirBinaryOp::Ne => {
                if is_float {
                    Instruction::F64Ne
                } else {
                    Instruction::I32Ne
                }
            }
            MirBinaryOp::Lt => {
                if is_float {
                    Instruction::F64Lt
                } else {
                    Instruction::I32LtS
                }
            }
            MirBinaryOp::Le => {
                if is_float {
                    Instruction::F64Le
                } else {
                    Instruction::I32LeS
                }
            }
            MirBinaryOp::Gt => {
                if is_float {
                    Instruction::F64Gt
                } else {
                    Instruction::I32GtS
                }
            }
            MirBinaryOp::Ge => {
                if is_float {
                    Instruction::F64Ge
                } else {
                    Instruction::I32GeS
                }
            }

            // Bitwise operations (only valid for integers)
            MirBinaryOp::And => Instruction::I32And,
            MirBinaryOp::Or => Instruction::I32Or,
            MirBinaryOp::Xor => Instruction::I32Xor,
            MirBinaryOp::Shl => Instruction::I32Shl,
            MirBinaryOp::Shr => Instruction::I32ShrS,
        };

        self.current_instructions.push(instruction);
        Ok(())
    }

    /// Helper: Check if an operand is a floating-point type.
    pub(super) fn is_float_operand(&self, operand: &MirOperand) -> bool {
        match operand {
            MirOperand::Constant(constant) => matches!(constant, MirConstant::Float(_)),
            MirOperand::Value(value_id) => {
                if let Some(mir_type) = self.value_to_type.get(value_id) {
                    matches!(mir_type, MirType::F32 | MirType::F64)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Helper: Check if an operand is a string type.
    pub(super) fn is_string_operand(&self, operand: &MirOperand) -> bool {
        match operand {
            MirOperand::Constant(constant) => matches!(constant, MirConstant::String(_)),
            MirOperand::Value(value_id) => {
                if let Some(mir_type) = self.value_to_type.get(value_id) {
                    // Strings are represented as Ptr(I8) for string literals and Ptr(U8) for
                    // host function results (toString, string operations). Both are string pointers.
                    matches!(mir_type, MirType::Ptr(inner) if matches!(inner.as_ref(), MirType::I8 | MirType::U8))
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Generate WASM unary operation.
    pub(super) fn generate_unary_operation(
        &mut self,
        op: &MirUnaryOp,
    ) -> Result<(), CompilerError> {
        match op {
            MirUnaryOp::Neg => {
                // Negate: 0 - x
                self.current_instructions.push(Instruction::I32Const(0));
                // Swap the order so we have: 0, x
                // Then subtract: 0 - x
                self.current_instructions.push(Instruction::I32Sub);
            }
            MirUnaryOp::Not => {
                // Logical not: x == 0
                self.current_instructions.push(Instruction::I32Const(0));
                self.current_instructions.push(Instruction::I32Eq);
            }
            MirUnaryOp::BitNot => {
                // Bitwise not: x ^ -1
                self.current_instructions.push(Instruction::I32Const(-1));
                self.current_instructions.push(Instruction::I32Xor);
            }
            // BOOK: required-operator - Required is handled specially in MirOperation::UnaryOp
            MirUnaryOp::Required => {
                // Required operator should be handled in UnaryOp match arm above
                // This should never be reached - just leave value on stack unchanged
                // (the check and trap happen in the special handling)
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Boxing / unboxing helpers for `any` type
    // -----------------------------------------------------------------------

    /// Emit code to box a value into an `any` type.
    ///
    /// Boxing allocates 12 bytes: `[tag:i32][value1:i32][value2:i32]`
    /// After this call, the boxed pointer is on the WASM stack.
    pub(super) fn emit_box_value(
        &mut self,
        tag: AnyTypeTag,
        source_type: &MirType,
    ) -> Result<(), CompilerError> {
        debug_mir!(?tag, ?source_type, "Boxing value to any type");

        // Get mem_alloc function index
        let mem_alloc_idx = *self
            .wasm_generator
            .function_map
            .get("mem_alloc")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "mem_alloc function not found in function_map for boxing".to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

        // The value to box is already on the stack
        // We need to save it to a temporary local first

        // Create a temporary local for the value
        let temp_local = self.next_local_index;
        self.next_local_index += 1;

        // Store the value to the temp local
        match source_type {
            MirType::F64 => {
                self.current_instructions
                    .push(Instruction::LocalSet(temp_local));
                self.temp_local_types.insert(temp_local, ValType::F64);
            }
            _ => {
                self.current_instructions
                    .push(Instruction::LocalSet(temp_local));
                self.temp_local_types.insert(temp_local, ValType::I32);
            }
        }

        // Allocate 12 bytes for the boxed structure
        // mem_alloc(type_id=0, size=12)
        self.current_instructions.push(Instruction::I32Const(0)); // type_id
        self.current_instructions.push(Instruction::I32Const(12)); // size
        self.current_instructions
            .push(Instruction::Call(mem_alloc_idx));

        // Save the pointer to another temp local
        let ptr_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(ptr_local, ValType::I32);
        self.current_instructions
            .push(Instruction::LocalTee(ptr_local));

        // Store the tag at offset 0
        // Stack: [ptr]
        self.current_instructions
            .push(Instruction::I32Const(tag.as_i32()));
        self.current_instructions
            .push(Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));

        // Store value1 at offset 4
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));

        match source_type {
            MirType::F64 => {
                // For f64, we need to reinterpret as i64 and split into two i32s
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions
                    .push(Instruction::I64ReinterpretF64);
                // Store low 32 bits
                self.current_instructions.push(Instruction::I32WrapI64);
                self.current_instructions
                    .push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));

                // Store value2 (high 32 bits) at offset 8
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions
                    .push(Instruction::I64ReinterpretF64);
                self.current_instructions.push(Instruction::I64Const(32));
                self.current_instructions.push(Instruction::I64ShrU);
                self.current_instructions.push(Instruction::I32WrapI64);
                self.current_instructions
                    .push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 8,
                        align: 2,
                        memory_index: 0,
                    }));
            }
            _ => {
                // For i32 types (integer, boolean, string pointer, etc.)
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions
                    .push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));

                // Store 0 in value2 at offset 8
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.current_instructions.push(Instruction::I32Const(0));
                self.current_instructions
                    .push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 8,
                        align: 2,
                        memory_index: 0,
                    }));
            }
        }

        // Push the boxed pointer onto the stack as the result
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));

        debug_mir!(?tag, "Boxing complete, pointer on stack");
        Ok(())
    }

    /// Emit code to unbox a value from an `any` type to i32.
    pub(super) fn emit_unbox_to_i32(&mut self) -> Result<(), CompilerError> {
        debug_mir!("Unboxing any value to i32");

        // Save pointer to a temp local so we can read both tag and value
        let ptr_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(ptr_local, ValType::I32);

        self.current_instructions
            .push(Instruction::LocalSet(ptr_local));

        // Read the type tag at offset 0
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));

        // Check if type tag is 3 (f64 number from JSON parsing)
        self.current_instructions.push(Instruction::I32Const(3));
        self.current_instructions.push(Instruction::I32Eq);
        self.current_instructions
            .push(Instruction::If(wasm_encoder::BlockType::Result(
                ValType::I32,
            )));

        // Type tag is 3 (Number): Read f64 at offset 4 and convert to i32
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::F64Load(wasm_encoder::MemArg {
                offset: 4,
                align: 3,
                memory_index: 0,
            }));
        // Convert f64 to i32 (truncate)
        self.current_instructions.push(Instruction::I32TruncF64S);

        self.current_instructions.push(Instruction::Else);

        // Type tag is not 3: Read i32 at offset 4 directly (type tag 1 = Integer)
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }));

        self.current_instructions.push(Instruction::End);

        Ok(())
    }

    /// Emit code to read the type tag from a boxed any value.
    pub(super) fn emit_read_any_tag(&mut self) -> Result<(), CompilerError> {
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
        Ok(())
    }

    /// Emit code to unbox a value to f64.
    pub(super) fn emit_unbox_to_f64(&mut self) -> Result<(), CompilerError> {
        debug_mir!("Unboxing any value to f64");

        // Save pointer to temp
        let ptr_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(ptr_local, ValType::I32);
        self.current_instructions
            .push(Instruction::LocalSet(ptr_local));

        // Read low bits from offset 4
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions.push(Instruction::I64ExtendI32U);

        // Read high bits from offset 8
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions.push(Instruction::I64ExtendI32U);
        self.current_instructions.push(Instruction::I64Const(32));
        self.current_instructions.push(Instruction::I64Shl);

        // Combine: high | low
        self.current_instructions.push(Instruction::I64Or);

        // Reinterpret as f64
        self.current_instructions
            .push(Instruction::F64ReinterpretI64);

        Ok(())
    }

    /// Emit code to convert an any value to string with proper type dispatch.
    pub(super) fn emit_any_to_string(&mut self) -> Result<(), CompilerError> {
        debug_mir!("Converting any value to string with type dispatch");

        // Get function indices for conversion functions
        let int_to_string_idx = *self
            .wasm_generator
            .function_map
            .get("int_to_string")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "int_to_string function not found".to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

        let float_to_string_idx = *self
            .wasm_generator
            .function_map
            .get("float_to_string")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "float_to_string function not found".to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

        let bool_to_string_idx = *self
            .wasm_generator
            .function_map
            .get("bool_to_string")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "bool_to_string function not found".to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

        // Save the boxed pointer to a local
        let ptr_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(ptr_local, ValType::I32);
        self.current_instructions
            .push(Instruction::LocalSet(ptr_local));

        // Read the tag
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.emit_read_any_tag()?;

        // Create a result local
        let result_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(result_local, ValType::I32);

        // Dispatch based on tag using if-else chain
        // if tag == 1 (Integer) -> int_to_string
        // else if tag == 2 (Boolean) -> bool_to_string
        // else if tag == 3 (Number) -> float_to_string
        // else if tag == 4 (String) -> return value directly
        // else -> int_to_string as fallback

        // Check for Integer (tag == 1)
        self.current_instructions
            .push(Instruction::I32Const(AnyTypeTag::Integer.as_i32()));
        self.current_instructions.push(Instruction::I32Eq);
        self.current_instructions
            .push(Instruction::If(BlockType::Empty));
        {
            // Integer case: call int_to_string
            self.current_instructions
                .push(Instruction::LocalGet(ptr_local));
            self.emit_unbox_to_i32()?;
            self.current_instructions
                .push(Instruction::Call(int_to_string_idx));
            self.current_instructions
                .push(Instruction::LocalSet(result_local));
        }
        self.current_instructions.push(Instruction::Else);
        {
            // Check for Boolean (tag == 2)
            self.current_instructions
                .push(Instruction::LocalGet(ptr_local));
            self.emit_read_any_tag()?;
            self.current_instructions
                .push(Instruction::I32Const(AnyTypeTag::Boolean.as_i32()));
            self.current_instructions.push(Instruction::I32Eq);
            self.current_instructions
                .push(Instruction::If(BlockType::Empty));
            {
                // Boolean case: call bool_to_string
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.emit_unbox_to_i32()?;
                self.current_instructions
                    .push(Instruction::Call(bool_to_string_idx));
                self.current_instructions
                    .push(Instruction::LocalSet(result_local));
            }
            self.current_instructions.push(Instruction::Else);
            {
                // Check for Number (tag == 3)
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.emit_read_any_tag()?;
                self.current_instructions
                    .push(Instruction::I32Const(AnyTypeTag::Number.as_i32()));
                self.current_instructions.push(Instruction::I32Eq);
                self.current_instructions
                    .push(Instruction::If(BlockType::Empty));
                {
                    // Number case: call float_to_string
                    self.current_instructions
                        .push(Instruction::LocalGet(ptr_local));
                    self.emit_unbox_to_f64()?;
                    self.current_instructions
                        .push(Instruction::Call(float_to_string_idx));
                    self.current_instructions
                        .push(Instruction::LocalSet(result_local));
                }
                self.current_instructions.push(Instruction::Else);
                {
                    // Check for String (tag == 4)
                    self.current_instructions
                        .push(Instruction::LocalGet(ptr_local));
                    self.emit_read_any_tag()?;
                    self.current_instructions
                        .push(Instruction::I32Const(AnyTypeTag::String.as_i32()));
                    self.current_instructions.push(Instruction::I32Eq);
                    self.current_instructions
                        .push(Instruction::If(BlockType::Empty));
                    {
                        // String case: return value directly (it's already a string pointer)
                        self.current_instructions
                            .push(Instruction::LocalGet(ptr_local));
                        self.emit_unbox_to_i32()?;
                        self.current_instructions
                            .push(Instruction::LocalSet(result_local));
                    }
                    self.current_instructions.push(Instruction::Else);
                    {
                        // Default case: treat as integer
                        self.current_instructions
                            .push(Instruction::LocalGet(ptr_local));
                        self.emit_unbox_to_i32()?;
                        self.current_instructions
                            .push(Instruction::Call(int_to_string_idx));
                        self.current_instructions
                            .push(Instruction::LocalSet(result_local));
                    }
                    self.current_instructions.push(Instruction::End); // End String if
                }
                self.current_instructions.push(Instruction::End); // End Number if
            }
            self.current_instructions.push(Instruction::End); // End Boolean if
        }
        self.current_instructions.push(Instruction::End); // End Integer if

        // Push the result onto the stack
        self.current_instructions
            .push(Instruction::LocalGet(result_local));

        debug_mir!("any.toString() dispatch complete");
        Ok(())
    }
}
