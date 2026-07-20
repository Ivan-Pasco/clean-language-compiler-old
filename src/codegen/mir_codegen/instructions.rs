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
                    // Normal unary operation — need the operand MIR type so we
                    // can emit the correct i32/f64 instructions (E0Xx avoids
                    // mismatches when negating a `number`).
                    let operand_type = self.get_operand_mir_type(operand);
                    self.load_operand(operand)?;
                    self.generate_unary_operation(op, operand_type.as_ref())?;
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

            MirOperation::CallCapability {
                receiver,
                capability_symbol,
                slot_index,
                arguments,
            } => {
                self.generate_call_capability(
                    instruction,
                    receiver,
                    *capability_symbol,
                    *slot_index,
                    arguments,
                )?;
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

                // For arrays we need a per-element byte stride. The GEP destination is
                // typed as Ptr<element_type> by the MIR builder, so the inner T tells
                // us the element MIR type. Elements that lower to f64/i64 must use an
                // 8-byte stride; everything else (i32-class types and pointers, which
                // are i32 in WASM MVP) uses 4. The MIR-builder side hardcoded Ptr<I32>
                // for years, which is why list<number>/list<integer64> read garbage
                // after element 0 (RUNTIME_ITERATE_LIST_NUMBER_WRONG_LOAD).
                let element_stride: i32 = if *is_array {
                    match instruction.dest.and_then(|d| self.value_to_type.get(&d)) {
                        Some(MirType::Ptr(inner)) => match **inner {
                            MirType::F64 | MirType::I64 | MirType::U64 => 8,
                            _ => 4,
                        },
                        _ => 4,
                    }
                } else {
                    0 // unused for struct field access
                };

                // For each index, load it and generate pointer arithmetic
                for (i, index) in indices.iter().enumerate() {
                    debug_mir!(index_num = i, index = ?index, "Processing index");
                    match self.load_operand(index) {
                        Ok(_) => {
                            debug_mir!(index_num = i, "Index loaded successfully");
                            // Calculate element address
                            if *is_array {
                                // For arrays: multiply index by element_stride and add header
                                // Array elements are at array_ptr + 16 + (index * stride)
                                self.current_instructions
                                    .push(Instruction::I32Const(element_stride));
                                self.current_instructions.push(Instruction::I32Mul);
                                self.current_instructions.push(Instruction::I32Add);
                                // Clean Language array layout:
                                //   Offset 0-3: Type marker (0)
                                //   Offset 4-7: Array length (i32)
                                //   Offset 8-11: Element size hint (ignored — driven by Ptr<T>)
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

                // Boxed Any layout: [tag@0][value1@4][value2@8]
                // Tags (from AnyTypeTag): 1=Integer, 2=Boolean, 3=Number,
                //                         4=String, 5=List, 6=Object
                // Boolean box: tag=2, value1 = 0 (false) or 1 (true).
                //
                // Prior to the fix, this operation returned `1` only when
                // tag == 2 and effectively ignored value1 — which happens to
                // be right for booleans stored via the standard boxing path
                // (value1 was always 1 when the Any was constructed from a
                // true boolean), but wrong for tag==4 (String) where a
                // caller doing `json.get(blob, "flag").toBoolean()` expected
                // "true"/"false" string parsing.
                //
                // New behaviour:
                //   tag == 2 (Boolean): return value1 (already 0 or 1)
                //   tag == 4 (String):  parse "true" as 1, everything else 0
                //   otherwise:          return 0 (safe default; Integer/Number/
                //                       collection coercions are out of scope)

                // Save the pointer to a temp so we can read tag AND value1.
                let ptr_local = self.next_local_index;
                self.next_local_index += 1;
                self.temp_local_types.insert(ptr_local, ValType::I32);

                self.load_operand(value)?;
                self.current_instructions
                    .push(Instruction::LocalSet(ptr_local));

                // Read the tag.
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));

                // tag == 2 (Boolean)?
                self.current_instructions.push(Instruction::I32Const(2));
                self.current_instructions.push(Instruction::I32Eq);
                self.current_instructions
                    .push(Instruction::If(wasm_encoder::BlockType::Result(
                        ValType::I32,
                    )));

                // tag=2: return value1 (0 or 1)
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));

                self.current_instructions.push(Instruction::Else);

                // Not Boolean. tag == 4 (String)?
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                self.current_instructions.push(Instruction::I32Const(4));
                self.current_instructions.push(Instruction::I32Eq);
                self.current_instructions
                    .push(Instruction::If(wasm_encoder::BlockType::Result(
                        ValType::I32,
                    )));

                // tag=4: value1 is an LP-string pointer. The parsed length is
                // the first 4 bytes (little-endian). We compare against the
                // literal `true` in-place:
                //   length == 4 && bytes at (ptr+4..ptr+8) == "true"
                // Byte-comparison via a single i32 load (little-endian) —
                // "true" = 0x65 0x75 0x72 0x74 → 0x65757274.
                //
                // This is intentionally strict: only lowercase "true" → 1;
                // any other string → 0. Matches the conservative behaviour
                // string.toBoolean uses on non-recognisable inputs elsewhere.
                let strptr_local = self.next_local_index;
                self.next_local_index += 1;
                self.temp_local_types.insert(strptr_local, ValType::I32);
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));
                self.current_instructions
                    .push(Instruction::LocalSet(strptr_local));

                // length check: *strptr == 4
                self.current_instructions
                    .push(Instruction::LocalGet(strptr_local));
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                self.current_instructions.push(Instruction::I32Const(4));
                self.current_instructions.push(Instruction::I32Eq);

                // bytes check: *(strptr+4) == 0x65757274 ("true" little-endian)
                self.current_instructions
                    .push(Instruction::LocalGet(strptr_local));
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 4,
                        align: 0, // may be unaligned inside string bytes
                        memory_index: 0,
                    }));
                self.current_instructions
                    .push(Instruction::I32Const(0x65757274));
                self.current_instructions.push(Instruction::I32Eq);

                self.current_instructions.push(Instruction::I32And);

                self.current_instructions.push(Instruction::Else);

                // Fallback for any other tag (Integer, Number, List, Object,
                // Null): return 0. Wider coercion is out of scope for the
                // String-tag fix; callers wanting Integer→bool should
                // explicitly compare against 0.
                self.current_instructions.push(Instruction::I32Const(0));

                self.current_instructions.push(Instruction::End); // close tag==4 else
                self.current_instructions.push(Instruction::End); // close tag==2 else

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

            // Short-circuit `a and b` / `a or b`. The rhs only executes
            // when the lhs cannot already determine the result; the rhs
            // instructions are inlined into the branch that runs in
            // that case.
            MirOperation::LogicalShortCircuit {
                is_and,
                lhs,
                rhs_instructions,
                rhs_value,
            } => {
                // Snapshot args before any &mut self call invalidates them.
                let is_and = *is_and;
                let lhs = lhs.clone();
                let rhs_instructions = rhs_instructions.clone();
                let rhs_value = rhs_value.clone();

                // Load the lhs onto the stack, then open an `if (result i32)`.
                self.load_operand(&lhs)?;
                self.current_instructions
                    .push(Instruction::If(wasm_encoder::BlockType::Result(
                        wasm_encoder::ValType::I32,
                    )));

                if is_and {
                    // `and`: when lhs is true, evaluate rhs; else push 0.
                    for instr in &rhs_instructions {
                        self.generate_instruction(instr)?;
                    }
                    self.load_operand(&rhs_value)?;
                    self.current_instructions.push(Instruction::Else);
                    self.current_instructions.push(Instruction::I32Const(0));
                } else {
                    // `or`: when lhs is true, push 1; else evaluate rhs.
                    self.current_instructions.push(Instruction::I32Const(1));
                    self.current_instructions.push(Instruction::Else);
                    for instr in &rhs_instructions {
                        self.generate_instruction(instr)?;
                    }
                    self.load_operand(&rhs_value)?;
                }
                self.current_instructions.push(Instruction::End);

                // Store the result in the dest local.
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                } else {
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            // Async host bridge: background someFunc(args)
            // Calls _async_fire(fn_name_ptr, fn_name_len, args_ptr, args_len) -> void
            MirOperation::AsyncFireCall { fn_name, arguments } => {
                debug_mir!(fn_name = %fn_name, arg_count = arguments.len(), "Processing AsyncFireCall");

                let fn_name_base = self.wasm_generator.get_or_create_string_offset(fn_name)?;
                let fn_name_content_ptr = fn_name_base + 4;
                let fn_name_len = fn_name.len() as i32;

                // Static empty JSON args array "[]"
                let args_json = "[]";
                let args_base = self.wasm_generator.get_or_create_string_offset(args_json)?;
                let args_content_ptr = args_base + 4;
                let args_len = args_json.len() as i32;

                // Load positional arguments so they can be evaluated (even though we pass them as
                // a serialised JSON blob in a future full implementation, evaluation ensures
                // side-effects still run and the compiler doesn't optimise them away).
                for arg in arguments {
                    self.load_operand(arg)?;
                    // Drop the individual argument value — they are not passed to the host
                    // bridge via the WASM stack; the host reads them from the JSON blob.
                    self.current_instructions.push(Instruction::Drop);
                }

                // Push (fn_name_ptr, fn_name_len, args_ptr, args_len) for _async_fire
                self.current_instructions
                    .push(Instruction::I32Const(fn_name_content_ptr as i32));
                self.current_instructions
                    .push(Instruction::I32Const(fn_name_len));
                self.current_instructions
                    .push(Instruction::I32Const(args_content_ptr as i32));
                self.current_instructions
                    .push(Instruction::I32Const(args_len));

                let fire_idx = *self
                    .wasm_generator
                    .function_map
                    .get("_async_fire")
                    .ok_or_else(|| CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            "_async_fire not registered".to_string(),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(instruction.location.clone()),
                        )),
                    })?;
                self.current_instructions.push(Instruction::Call(fire_idx));

                // _async_fire returns void — nothing to store
            }

            // Async host bridge: later x = someFunc(args)
            // Calls _async_await(fn_name_ptr, fn_name_len, args_ptr, args_len) -> i32 (result ptr)
            MirOperation::AsyncAwaitCall { fn_name, arguments } => {
                debug_mir!(fn_name = %fn_name, arg_count = arguments.len(), "Processing AsyncAwaitCall");

                let fn_name_base = self.wasm_generator.get_or_create_string_offset(fn_name)?;
                let fn_name_content_ptr = fn_name_base + 4;
                let fn_name_len = fn_name.len() as i32;

                // Static empty JSON args array "[]"
                let args_json = "[]";
                let args_base = self.wasm_generator.get_or_create_string_offset(args_json)?;
                let args_content_ptr = args_base + 4;
                let args_len = args_json.len() as i32;

                // Load positional arguments so side-effects are preserved.
                for arg in arguments {
                    self.load_operand(arg)?;
                    self.current_instructions.push(Instruction::Drop);
                }

                // Push (fn_name_ptr, fn_name_len, args_ptr, args_len) for _async_await
                self.current_instructions
                    .push(Instruction::I32Const(fn_name_content_ptr as i32));
                self.current_instructions
                    .push(Instruction::I32Const(fn_name_len));
                self.current_instructions
                    .push(Instruction::I32Const(args_content_ptr as i32));
                self.current_instructions
                    .push(Instruction::I32Const(args_len));

                let await_idx = *self
                    .wasm_generator
                    .function_map
                    .get("_async_await")
                    .ok_or_else(|| CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            "_async_await not registered".to_string(),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(instruction.location.clone()),
                        )),
                    })?;
                self.current_instructions.push(Instruction::Call(await_idx));

                // _async_await returns i32 (pointer to result string/value in memory)
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                } else {
                    // No destination — drop the return value
                    self.current_instructions.push(Instruction::Drop);
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
                // A Return terminator must leave a value on the stack matching the
                // function's declared return type before the bare Return
                // instruction (or nothing on the stack for void). Handle all three
                // shapes uniformly via push_zero_for_return_type. See its docs.
                let func_return_type = self
                    .current_function
                    .as_ref()
                    .map(|f| f.return_type.clone());
                let mut pushed_value = false;
                if let Some(return_value) = value {
                    if !matches!(return_value, MirOperand::Constant(MirConstant::Undefined)) {
                        // NOTE: Removed StringTuple expansion logic
                        // Since ConcreteType::String now maps to MirType::I32, strings are single i32 pointers
                        // No expansion needed - just load the operand directly
                        self.load_operand(return_value)?;
                        pushed_value = true;

                        // Coerce the return value type to match the function's declared return type
                        // (E007 fix). value_to_type covers both parameters and locals;
                        // get_operand_mir_type only covers func.locals, missing parameters.
                        let value_type = match return_value {
                            MirOperand::Value(vid) => self.value_to_type.get(vid).cloned(),
                            _ => self.get_operand_mir_type(return_value),
                        };
                        match (&func_return_type, &value_type) {
                            (Some(MirType::F64), Some(MirType::I32))
                            | (Some(MirType::F64), Some(MirType::I8))
                            | (Some(MirType::F64), Some(MirType::I16))
                            | (Some(MirType::F64), Some(MirType::U8))
                            | (Some(MirType::F64), Some(MirType::U16))
                            | (Some(MirType::F64), Some(MirType::U32)) => {
                                // Return value is integer but function declares number (f64) return.
                                // Insert signed integer-to-float conversion (E007 fix).
                                self.current_instructions.push(Instruction::F64ConvertI32S);
                            }
                            (Some(MirType::I32), Some(MirType::F64)) => {
                                // Return value is float but function declares integer (i32) return.
                                // Truncate to integer (saturating behaviour via TruncF64S).
                                self.current_instructions.push(Instruction::I32TruncF64S);
                            }
                            _ => {}
                        }
                    }
                }
                if !pushed_value {
                    self.push_zero_for_return_type(&func_return_type);
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
                    // Reverse-lookup: find the function name that maps to this
                    // index. Multiple names alias to the same index (e.g. the
                    // language alias `json.get` + the bridge alias `_json_get`
                    // + potentially a wrapper alias) — which name wins here
                    // determines which arm of the big call-dispatch match
                    // below fires downstream. If we iterate the raw HashMap,
                    // the "first match" depends on random hasher state and
                    // produces different codegen across recompiles (the root
                    // cause of the flaky WASM-out-of-bounds tasks_list_page
                    // failures reported in CODEGEN-STRING-ARG-ALIAS-JSONGET).
                    //
                    // Sort the candidates alphabetically so the same name
                    // always wins for a given index. Choosing the
                    // lexicographically smallest is arbitrary but stable and
                    // matches the sorted export order.
                    let mut candidates: Vec<&String> = self
                        .wasm_generator
                        .function_map
                        .iter()
                        .filter(|(_, &idx)| idx == function_index)
                        .map(|(name, _)| name)
                        .collect();
                    candidates.sort();
                    if let Some(name) = candidates.first() {
                        debug_mir!(
                            "DEBUG REVERSE LOOKUP: SymbolId({}) -> index {} -> name '{}'",
                            symbol_id.0,
                            function_index,
                            name
                        );
                        function_name = Some(name.to_string());
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
            Some("string.concat")
            | Some("string_concat")
            | Some("native_string_concat")
            | Some("string_concat_transient")
            | Some("__string_concat_transient") => {
                debug_mir!(": Matched string.concat (or transient variant)");
                // Both __string_concat and __string_concat_transient have
                // the same calling convention: (str_ptr1, str_ptr2) -> result_ptr.
                // The only difference is where the result is allocated
                // (__malloc vs __transient_alloc). DO NOT expand to (ptr, len)
                // pairs — just pass the struct pointers.
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
            | Some("input.yesNo")
            | Some("error") => {
                debug_mir!(": Matched input/error function - using load_string_pointer_only");
                // Input and error functions expect only (string_ptr) -> result
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

                    // Defensive guard against re-shipping CODEGEN-STRING-ALIAS-REGRESSED-0334
                    // (compiler 0.33.44 fingerprint 54887260). If the callee resolves to a
                    // plugin `expand_strings=true` bridge wrapper, its WASM code reads
                    // `mem[ptr+0]` as the Clean-string length prefix. Passing a boxed
                    // Any (whose offset 0 is the type tag byte) causes the wrapper to
                    // forward `(ptr+4, tag_byte_as_len)` — either an OOB host read or a
                    // garbage response. Catch this at codegen time in debug builds.
                    //
                    // Runtime cost in release builds is zero: `debug_assert!` compiles out.
                    debug_assert!(
                        !(function_name
                            .as_deref()
                            .is_some_and(|n| self.resolves_to_expand_strings_wrapper(n))
                            && matches!(arg, MirOperand::Value(v) if matches!(self.value_to_type.get(v), Some(MirType::Any)))),
                        "call-site guard: `{}` resolves to an expand_strings=true bridge wrapper \
                         (registered via register_pending_bridge_wrappers), which expects raw \
                         length-prefixed Clean String pointers. Argument {} is typed `MirType::Any` \
                         (boxed [tag][ptr] struct). The wrapper will read the tag byte (typically 4 \
                         for String) as the length and forward `(box_ptr+4, 4)` to the host — \
                         producing an out-of-bounds read or a garbage response. \
                         Fix at the MIR-builder level, not by inserting an unbox here: \
                         the whole point of the guard is that the caller was already wrong \
                         upstream, and hiding the mismatch here masks the real bug. \
                         See fingerprint 54887260abf6 for the incident that motivates this guard.",
                        function_name.as_deref().unwrap_or("<unknown>"),
                        i
                    );

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

                    // Bridge function fallback: when function_signature is None (bridge
                    // functions are not in function_signatures), use bridge_param_types
                    // to detect Number (f64) params and insert the required conversion.
                    // Without this, passing an integer to a bridge Number param causes
                    // WASM validation failure: "type mismatch: expected f64, found i32"
                    // (reproduces CODEGEN_F64 — _ui_intersect_observe, _ui_set_scroll, etc.).
                    if param_types.is_none() {
                        if let Some(bridge_params) = bridge_handler_params.as_ref() {
                            if i < bridge_params.len()
                                && matches!(
                                    bridge_params[i],
                                    crate::builtins::registry::BuiltinType::Number
                                )
                            {
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
                                        "DEBUG CALL ARGS:   Bridge Number param[{}]: inserting i32→f64 conversion",
                                        i
                                    );
                                    self.current_instructions
                                        .push(Instruction::F64ConvertI32S);
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
                        let bridge_after_id = self.maybe_emit_probe_before_bridge_call(
                            function_index,
                            &instruction.location,
                        );
                        self.current_instructions
                            .push(Instruction::Call(function_index));
                        self.maybe_emit_probe_after_call(function_index, &instruction.location);
                        self.maybe_emit_probe_after_bridge_call(
                            bridge_after_id,
                            function_index,
                            &instruction.location,
                        );
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
                                        "time",
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
                                    "time",
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
                        })
                        // Plugin-emitted helper lookup: "auth.jwt.sign" → "jwt_sign"
                        // (see language_to_helper_map above). Same fix as the
                        // NamedFunction arm below; kept in parity so calls whose
                        // callee arrives as SymbolFunction (resolver path) and
                        // NamedFunction (parser fallback path) both resolve.
                        .or_else(|| {
                            self.language_to_helper_map.get(&function_name).and_then(
                                |helper_name| {
                                    self.wasm_generator.function_map.get(helper_name).copied()
                                },
                            )
                        });

                        if let Some(function_index) = function_index {
                            tracing::trace!(
                                name = %function_name,
                                index = function_index,
                                "Calling function at WASM index"
                            );
                            let bridge_after_id = self.maybe_emit_probe_before_bridge_call(
                                function_index,
                                &instruction.location,
                            );
                            self.current_instructions
                                .push(Instruction::Call(function_index));
                            self.maybe_emit_probe_after_call(function_index, &instruction.location);
                            self.maybe_emit_probe_after_bridge_call(
                                bridge_after_id,
                                function_index,
                                &instruction.location,
                            );
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
                            let help = build_did_you_mean_hint(
                                &function_name,
                                &self.wasm_generator.function_map,
                            );
                            return Err(CompilerError::Codegen {
                                context: Box::new(crate::error::ErrorContext::new(
                                    format!(
                                        "Function '{}' (SymbolId({})) not found in function map during code generation",
                                        function_name, symbol_id.0
                                    ),
                                    help,
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
                        })
                        // Plugin-emitted helper lookup: "auth.jwt.sign" → "jwt_sign"
                        // via language_to_helper_map. Fixes FRAME-AUTH-JWT-HELPERS-UNREACHABLE:
                        // the helper WAS emitted (appended to program.functions during
                        // framework-block expansion) but its language-facing dotted name
                        // was previously dropped as "LSP-only" and had no route to the
                        // helper's WASM index.
                        .or_else(|| {
                            self.language_to_helper_map
                                .get(name)
                                .and_then(|helper_name| {
                                    self.wasm_generator.function_map.get(helper_name).copied()
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
                        let bridge_after_id =
                            self.maybe_emit_probe_before_bridge_call(idx, &instruction.location);
                        self.current_instructions.push(Instruction::Call(idx));
                        self.maybe_emit_probe_after_call(idx, &instruction.location);
                        self.maybe_emit_probe_after_bridge_call(
                            bridge_after_id,
                            idx,
                            &instruction.location,
                        );
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
                        let help = build_did_you_mean_hint(name, &self.wasm_generator.function_map);
                        return Err(CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                format!("Function '{}' not found in function map", name),
                                help,
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
                        .is_some_and(|rt| matches!(rt, MirType::Void));

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
                } else {
                    // Last-resort source-type lookup: query the wasm_generator for the
                    // function's registered WASM return type. This catches cases where
                    // neither function_signatures (MIR-level) nor function_return_types
                    // (stdlib/bridge registry) resolved a type — e.g. plugin-DSL-generated
                    // helpers, HTTP server wrappers, or any internal function whose
                    // result is stored into a typed local. Without this, an i32 call
                    // result stored into an f64 local (or vice versa) skips coercion
                    // and produces "type mismatch: expected f64, found i32" at WASM
                    // validation time (CODEGEN_F64 / fp 1a20405b).
                    let wasm_return_mir = function_name.as_ref().and_then(|name| {
                        match self.wasm_generator.get_wasm_return_type(name) {
                            Some(Some(crate::types::WasmType::I32)) => Some(MirType::I32),
                            Some(Some(crate::types::WasmType::I64)) => Some(MirType::I64),
                            Some(Some(crate::types::WasmType::F32)) => Some(MirType::F32),
                            Some(Some(crate::types::WasmType::F64)) => Some(MirType::F64),
                            _ => None,
                        }
                    });

                    if let Some(dest_type) = self.value_to_type.get(&dest) {
                        debug_mir!(
                            "DEBUG VOID CHECK: dest={:?}, dest_type={:?}, function={:?}, wasm_return_mir={:?}",
                            dest,
                            dest_type,
                            function_name,
                            wasm_return_mir
                        );
                        let is_any_or_ptr_void = matches!(dest_type, MirType::Any)
                            || matches!(dest_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void));

                        if is_any_or_ptr_void {
                            // If the function is a known void built-in, do NOT store —
                            // there is nothing on the stack. The MIR may assign a dest
                            // (Any-typed) to a void call as a byproduct of expression
                            // lowering, but the WASM-level call left the stack empty
                            // because the bridge signature is `-> void`. Storing here
                            // produces COM001 "expected i32 but nothing on stack".
                            //
                            // Fingerprint pattern: repro is a bare `server.sleep(0)`
                            // or similar void call to a namespaced built-in whose
                            // callsite propagates as Any through the MIR.
                            let is_known_void_by_name = function_name
                                .as_deref()
                                .and_then(|name| self.function_return_types.get(name))
                                .is_some_and(|rt| {
                                    matches!(rt, MirType::Void)
                                        || matches!(rt, MirType::Ptr(inner) if matches!(**inner, MirType::Void))
                                });
                            let is_known_void_hardcoded = matches!(
                                function_name.as_deref(),
                                Some("print")
                                    | Some("printl")
                                    | Some("list.set")
                                    | Some("list.clear")
                                    | Some("list.setFlags")
                                    | Some("pairs.set")
                                    | Some("__pairs_set")
                                    | Some("mem_release")
                                    | Some("mem_retain")
                                    | Some("mem_scope_push")
                                    | Some("mem_scope_pop")
                                    | Some("string_builder_reclaim")
                                    | Some("__string_builder_reclaim")
                                    | Some("transient_scope_exit")
                                    | Some("__transient_scope_exit")
                                    | Some("server.sleep")
                                    | Some("_server_sleep")
                                    | Some("_state_reset_all")
                                    | Some("_state_reset_named")
                                    | Some("http.setUserAgent")
                                    | Some("http.setTimeout")
                                    | Some("http.setMaxRedirects")
                                    | Some("http.enableCookies")
                            );
                            if is_known_void_by_name || is_known_void_hardcoded {
                                debug_mir!(
                                    "DEBUG ANY DEST: Skipping store for known void {:?}",
                                    function_name
                                );
                            } else {
                                debug_mir!(
                                    "DEBUG ANY DEST: Storing value to Any/dynamic type dest {:?}",
                                    dest
                                );
                                // For Any/Ptr(Void) destinations no coercion is appropriate
                                // (Any is the boxed-pointer representation; treat as opaque).
                                self.store_to_local(dest)?;
                            }
                        } else if wasm_return_mir.is_some() {
                            // We now know the source type from the WASM type section;
                            // route through the conversion helper to handle i32↔f64 mismatch.
                            self.store_to_local_with_conversion(dest, wasm_return_mir)?;
                        } else {
                            // Concrete dest type but unknown source — store as-is.
                            self.store_to_local(dest)?;
                        }
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
                            } else if wasm_return_mir.is_some() {
                                self.store_to_local_with_conversion(dest, wasm_return_mir)?;
                            } else {
                                self.store_to_local(dest)?;
                            }
                        } else if wasm_return_mir.is_some() {
                            self.store_to_local_with_conversion(dest, wasm_return_mir)?;
                        } else {
                            self.store_to_local(dest)?;
                        }
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
                let is_known_void_builtin = matches!(
                    function_name.as_deref(),
                    Some("print")
                        | Some("printl")
                        | Some("list.set")
                        | Some("list.clear")
                        | Some("list.setFlags")
                        | Some("pairs.set")
                        | Some("__pairs_set")
                        | Some("mem_release")
                        | Some("mem_retain")
                        | Some("mem_scope_push")
                        | Some("mem_scope_pop")
                        | Some("string_builder_reclaim")
                        | Some("__string_builder_reclaim")
                        | Some("transient_scope_exit")
                        | Some("__transient_scope_exit")
                        | Some("server.sleep")
                        | Some("_server_sleep")
                        | Some("_state_reset_all")
                        | Some("_state_reset_named")
                        // http_client client-configuration setters — void return,
                        // called as expression statements. The stdlib wrappers in
                        // src/stdlib/http_class.rs register these with
                        // `return_type: None`, but the WASM-level lookup in
                        // `wasm_function_is_void` misses them at codegen time.
                        // Without this branch the fallback emits a spurious Drop
                        // on top of the empty stack the void call leaves behind,
                        // producing COM001 "expected i32 but nothing on stack".
                        // Fingerprint 0cf6c5198b50 (COM001).
                        | Some("http.setUserAgent")
                        | Some("http.setTimeout")
                        | Some("http.setMaxRedirects")
                        | Some("http.enableCookies")
                );

                if is_known_void_builtin {
                    debug_mir!(
                        " CALL NO DEST: Known void built-in function: {:?}",
                        function_name
                    );
                    true
                } else {
                    // Check function_return_types registry — populated by
                    // register_plugin_bridge_imports for plugin.toml bridge
                    // functions with returns = "void", AND seeded upfront
                    // from `mir_program.functions` (in `generate()`) so user
                    // functions resolve here too. Without the upfront seed,
                    // user-class void methods called as expression-statements
                    // fall through and emit a spurious DROP, causing WASM
                    // validation failure ("expected a type but nothing on
                    // stack"). Treat both `Void` and `Ptr(Void)` as void —
                    // `ConcreteType::Null` (the void marker propagated from
                    // the typechecker) lowers to `MirType::Ptr(Void)` via
                    // `MirType::from_concrete_type`, so a user function with
                    // no declared return type ends up registered as
                    // `Ptr(Void)`, not `Void`.
                    //
                    // Look up by the call-site name AND by a dot-stripped
                    // variant. `MirOperand::NamedFunction { name }` for a
                    // static call uses the qualified form "ClassName.method"
                    // (set in mir_builder/expressions.rs StaticMethodCall
                    // arm) but functions are registered in `function_signatures`
                    // / `function_return_types` under the bare method name
                    // ("method"). Mirroring the strip-prefix chain that
                    // `wasm_function_is_void` already uses keeps this path
                    // consistent.
                    let lookup_void = |name: &str,
                                       rts: &std::collections::HashMap<String, MirType>|
                     -> bool {
                        if let Some(rt) = rts.get(name) {
                            return matches!(rt, MirType::Void)
                                || matches!(rt, MirType::Ptr(inner) if matches!(**inner, MirType::Void));
                        }
                        if let Some(dot_pos) = name.find('.') {
                            let bare = &name[dot_pos + 1..];
                            if let Some(rt) = rts.get(bare) {
                                return matches!(rt, MirType::Void)
                                    || matches!(rt, MirType::Ptr(inner) if matches!(**inner, MirType::Void));
                            }
                        }
                        false
                    };
                    let is_registered_void = function_name
                        .as_deref()
                        .map(|name| lookup_void(name, &self.function_return_types))
                        .unwrap_or(false);

                    if is_registered_void {
                        debug_mir!(
                            " CALL NO DEST: Registered void function: {:?}",
                            function_name
                        );
                        true
                    } else {
                        // Last-resort WASM-level lookup. CODEGEN_STACK_REMAINING
                        // (fp fa0584d8): the StaticMethodCall MIR lowering
                        // already sets `dest = None` when the call is to a
                        // void user-class method, but the call may reach
                        // codegen as a NamedFunction whose Clean Language
                        // name doesn't match any of the hardcoded builtins
                        // and isn't in `function_return_types` (that registry
                        // only holds plugin bridge imports). Without this
                        // check the fallback defaults to non-void → emits
                        // Drop on top of the empty stack left by the void
                        // WASM call → wasmparser reports "expected a type
                        // but nothing on stack". Querying the WASM-level
                        // type registry by the same name variants the
                        // NamedFunction resolution uses closes the gap: a
                        // function registered with no WASM result type IS
                        // void at the WASM level.
                        let wasm_says_void = function_name
                            .as_deref()
                            .map(|name| self.wasm_function_is_void(name))
                            .unwrap_or(false);

                        if wasm_says_void {
                            debug_mir!(
                                " CALL NO DEST: WASM-level signature is void: {:?}",
                                function_name
                            );
                            true
                        } else {
                            // NOTE: For functions without signatures called as expression statements,
                            // default to NON-VOID (add DROP) to prevent stack pollution
                            debug_mir!(" CALL NO DEST: Unknown function without signature, defaulting to non-void (adding DROP for safety)");
                            false
                        }
                    }
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

    /// STATE-A heap-probe hunt — if the callee at `function_index` is
    /// `__string_builder_append` or `__string_builder_finalize`, emit a
    /// `call $_probe_ptr(callsite_id, result_ptr)` immediately after the
    /// call, preserving the returned pointer on the stack for the caller.
    ///
    /// Sequence emitted (only when `--emit-heap-probes` is set AND the
    /// callee matches):
    ///
    /// ```text
    ///   (Instruction::Call(fn_idx) was just pushed by the caller)
    ///   local.tee <probe_scratch>   ;; ptr stays on the stack
    ///   i32.const <callsite_id>
    ///   local.get <probe_scratch>
    ///   call $_probe_ptr
    /// ```
    ///
    /// No-op when the flag is off (early return). No-op when `_probe_ptr`
    /// wasn't registered (a defensive check — shouldn't happen because the
    /// import registration path is gated on the same flag).
    ///
    /// Also records a [`ProbeCallsite`] entry for the sidecar JSON.
    pub(super) fn maybe_emit_probe_after_call(
        &mut self,
        function_index: u32,
        instruction_loc: &crate::ast::SourceLocation,
    ) {
        if !crate::emit_heap_probes_override() {
            return;
        }

        // Which string_builder alias does this index belong to? We match on
        // the *raw* internal name registered by `register_function_with_locals`
        // (see codegen_registration.rs:175 for append, :195 for finalize).
        // The public aliases `string_builder_append` / `string_builder_finalize`
        // also map to the same index — either resolves.
        let append_idx = self
            .wasm_generator
            .function_map
            .get("__string_builder_append")
            .copied();
        let finalize_idx = self
            .wasm_generator
            .function_map
            .get("__string_builder_finalize")
            .copied();

        let callee_label: &str = if Some(function_index) == append_idx {
            "string_builder_append"
        } else if Some(function_index) == finalize_idx {
            "string_builder_finalize"
        } else {
            return;
        };

        // The probe import must exist. If it doesn't, the CLI flag was set
        // but the import registration was skipped for some reason — bail
        // rather than emitting a broken call.
        let Some(&probe_ptr_idx) = self.wasm_generator.function_map.get("_probe_ptr") else {
            return;
        };

        // Allocate an i32 scratch local to tee the returned pointer into.
        let scratch = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(scratch, ValType::I32);

        // Assign this callsite an id. Sidecar order matches emission order.
        let callsite_id: u32 = (self.probe_callsites.len() as u32) + 1;

        // Emit the probe sequence. `LocalTee` keeps the value on the stack
        // for the caller to consume, while also writing it into `scratch`.
        self.current_instructions
            .push(Instruction::LocalTee(scratch));
        self.current_instructions
            .push(Instruction::I32Const(callsite_id as i32));
        self.current_instructions
            .push(Instruction::LocalGet(scratch));
        self.current_instructions
            .push(Instruction::Call(probe_ptr_idx));

        // Record the sidecar entry.
        let caller_name = self
            .current_function
            .as_ref()
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let source = if instruction_loc.file.is_empty() {
            format!("<unknown>:{}", instruction_loc.line)
        } else {
            format!("{}:{}", instruction_loc.file, instruction_loc.line)
        };
        self.probe_callsites.push(super::ProbeCallsite {
            id: callsite_id,
            function: callee_label.to_string(),
            caller: caller_name,
            source,
        });
    }

    /// STATE-A bridge-probe hunt — if the callee at `function_index` is a
    /// host-imported bridge function AND `--emit-bridge-probes` is set, emit
    /// `call $_probe_ptr(before_id, 0)` immediately BEFORE the pending
    /// `Instruction::Call(function_index)`. Returns `Some(after_id)` that the
    /// caller passes to `maybe_emit_probe_after_bridge_call` right after the
    /// call is pushed; returns `None` when no after-probe should fire.
    ///
    /// A callee is considered a bridge iff any of these hold:
    /// - the function name at `function_index` appears as a value in
    ///   `language_to_bridge_map` (e.g. `_db_query`), OR
    /// - its name appears as a key in `bridge_param_types`
    ///   (the plugin-bridge wrapper table).
    ///
    /// No-op when the flag is off. Also records a `ProbeCallsite` entry with
    /// `function` = `"bridge_before:<name>"` in the sidecar.
    pub(super) fn maybe_emit_probe_before_bridge_call(
        &mut self,
        function_index: u32,
        instruction_loc: &crate::ast::SourceLocation,
    ) -> Option<u32> {
        if !crate::emit_bridge_probes_override() {
            return None;
        }

        let bridge_name = self.bridge_name_for_index(function_index)?;

        let probe_ptr_idx = self
            .wasm_generator
            .function_map
            .get("_probe_ptr")
            .copied()?;

        // BEFORE probe — no payload to probe yet, use 0 as the ptr argument.
        // (The interesting datum is the callsite id + implicit __heap_ptr the
        // host-side probe captures in its bridge.)
        let before_id: u32 = (self.probe_callsites.len() as u32) + 1;
        self.current_instructions
            .push(Instruction::I32Const(before_id as i32));
        self.current_instructions.push(Instruction::I32Const(0));
        self.current_instructions
            .push(Instruction::Call(probe_ptr_idx));

        let caller_name = self
            .current_function
            .as_ref()
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let source = if instruction_loc.file.is_empty() {
            format!("<unknown>:{}", instruction_loc.line)
        } else {
            format!("{}:{}", instruction_loc.file, instruction_loc.line)
        };
        self.probe_callsites.push(super::ProbeCallsite {
            id: before_id,
            function: format!("bridge_before:{}", bridge_name),
            caller: caller_name,
            source,
        });

        // Reserve the after_id up front so both sides of the pair are
        // sequentially numbered. Returning it also acts as an "emit after
        // probe" flag — None means the caller must NOT emit an after probe
        // (bridge unprobeable, or flag off).
        let after_id: u32 = (self.probe_callsites.len() as u32) + 1;
        Some(after_id)
    }

    /// STATE-A bridge-probe hunt — emit `call $_probe_ptr(after_id, ptr)`
    /// AFTER a bridge call whose BEFORE probe returned `Some(after_id)`.
    /// When the callee returns i32, tees the returned pointer so the caller
    /// still consumes it. When the callee returns non-i32 (i64/f32/f64) OR
    /// void, emits `_probe_ptr(after_id, 0)` — the fact that control returned
    /// is itself the observable event.
    ///
    /// The pending call must already be pushed onto `current_instructions`.
    pub(super) fn maybe_emit_probe_after_bridge_call(
        &mut self,
        after_id: Option<u32>,
        function_index: u32,
        instruction_loc: &crate::ast::SourceLocation,
    ) {
        let Some(after_id) = after_id else {
            return;
        };

        let Some(bridge_name) = self.bridge_name_for_index(function_index) else {
            return;
        };

        let probe_ptr_idx = match self.wasm_generator.function_map.get("_probe_ptr").copied() {
            Some(idx) => idx,
            None => return,
        };

        let return_type = self
            .wasm_generator
            .wasm_function_return_types
            .get(&bridge_name)
            .copied()
            .flatten();

        match return_type {
            Some(crate::types::WasmType::I32) => {
                // Tee the returned pointer so it stays on the stack for the
                // caller, and pass a copy to the probe.
                let scratch = self.next_local_index;
                self.next_local_index += 1;
                self.temp_local_types.insert(scratch, ValType::I32);
                self.current_instructions
                    .push(Instruction::LocalTee(scratch));
                self.current_instructions
                    .push(Instruction::I32Const(after_id as i32));
                self.current_instructions
                    .push(Instruction::LocalGet(scratch));
                self.current_instructions
                    .push(Instruction::Call(probe_ptr_idx));
            }
            _ => {
                // Void / non-i32 return — the ptr slot has no meaningful
                // value; use 0. The call already left nothing (void) or a
                // value the caller consumes (i64/f32/f64), so we do NOT tee.
                self.current_instructions
                    .push(Instruction::I32Const(after_id as i32));
                self.current_instructions.push(Instruction::I32Const(0));
                self.current_instructions
                    .push(Instruction::Call(probe_ptr_idx));
            }
        }

        let caller_name = self
            .current_function
            .as_ref()
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let source = if instruction_loc.file.is_empty() {
            format!("<unknown>:{}", instruction_loc.line)
        } else {
            format!("{}:{}", instruction_loc.file, instruction_loc.line)
        };
        self.probe_callsites.push(super::ProbeCallsite {
            id: after_id,
            function: format!("bridge_after:{}", bridge_name),
            caller: caller_name,
            source,
        });
    }

    /// Reverse-lookup: given a WASM function index, return the bridge's raw
    /// name if the index refers to a known bridge function, else `None`.
    ///
    /// Consulted only when a probe flag is set; the O(n) scans are gated
    /// behind that check by callers.
    fn bridge_name_for_index(&self, function_index: u32) -> Option<String> {
        // First look through language_to_bridge_map values.
        for bridge_name in self.language_to_bridge_map.values() {
            if let Some(&idx) = self.wasm_generator.function_map.get(bridge_name) {
                if idx == function_index {
                    return Some(bridge_name.clone());
                }
            }
        }
        // Then look through bridge_param_types keys (plugin bridge wrappers).
        for name in self.bridge_param_types.keys() {
            if let Some(&idx) = self.wasm_generator.function_map.get(name) {
                if idx == function_index {
                    return Some(name.clone());
                }
            }
        }
        None
    }

    /// Lower a `MirOperation::CallCapability` into a runtime class-id switch
    /// that dispatches to the correct concrete method implementation.
    ///
    /// Emits, for each class that conforms to the target capability:
    ///
    /// ```text
    /// receiver.class_id == class_N
    ///     ? call class_N::method(receiver, args...)
    ///     : (continue to next arm)
    /// ```
    ///
    /// The final arm falls through to `unreachable` — the type checker
    /// guarantees the receiver's runtime class is one of the conforming
    /// classes, so this trap is defensive rather than reachable.
    fn generate_call_capability(
        &mut self,
        instruction: &MirInstruction,
        receiver: &MirOperand,
        capability_symbol: SymbolId,
        slot_index: usize,
        arguments: &[MirOperand],
    ) -> Result<(), CompilerError> {
        // Look up conforming classes for this capability method slot.
        let entries = self
            .capability_dispatch
            .get(&(capability_symbol, slot_index))
            .cloned()
            .unwrap_or_default();

        if entries.is_empty() {
            return Err(CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    format!(
                        "CallCapability: no class conforms to capability #{} slot {} — resolver should have prevented this",
                        capability_symbol.0, slot_index
                    ),
                    None,
                    crate::error::ErrorType::Codegen,
                    Some(instruction.location.clone()),
                )),
            });
        }

        // Determine WASM result type from destination local, if any. Used to
        // pick the right `BlockType` for each `if` branch so the WASM stack
        // shape validates.
        let result_val_type = instruction.dest.and_then(|d| {
            self.value_to_type.get(&d).and_then(|mir_ty| match mir_ty {
                MirType::I32 | MirType::Ptr(_) => Some(ValType::I32),
                MirType::F64 => Some(ValType::F64),
                MirType::I64 | MirType::U64 => Some(ValType::I64),
                MirType::Void => None,
                _ => Some(ValType::I32),
            })
        });
        let block_type = match result_val_type {
            Some(vt) => wasm_encoder::BlockType::Result(vt),
            None => wasm_encoder::BlockType::Empty,
        };

        // Stash receiver in a local so we can load it once per arm without
        // re-evaluating side effects.
        self.load_operand(receiver)?;
        let receiver_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(receiver_local, ValType::I32);
        self.current_instructions
            .push(Instruction::LocalSet(receiver_local));

        // Emit an if/else-if chain over class_ids.
        // Structure per arm:
        //   load receiver.class_id
        //   i32.const <class_id>
        //   i32.eq
        //   if <blocktype>
        //     load receiver
        //     load each arg
        //     call <method_symbol_index>
        //   else
        //     ... next arm ...
        //   end
        //
        // The final else emits `unreachable` for the "should never happen"
        // case where a class not in the conformance set somehow reached here.
        let n_arms = entries.len();
        for (class_id, method_sym) in &entries {
            // Load receiver.class_id (i32 at offset 0 of instance header).
            self.current_instructions
                .push(Instruction::LocalGet(receiver_local));
            self.current_instructions
                .push(Instruction::I32Load(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2, // 4-byte aligned
                    memory_index: 0,
                }));
            self.current_instructions
                .push(Instruction::I32Const(*class_id as i32));
            self.current_instructions.push(Instruction::I32Eq);
            self.current_instructions.push(Instruction::If(block_type));

            // Then branch: call the concrete method.
            self.current_instructions
                .push(Instruction::LocalGet(receiver_local));
            for arg in arguments {
                self.load_operand(arg)?;
            }
            let Some(&fn_idx) = self.symbol_to_function_index.get(method_sym) else {
                return Err(CompilerError::Codegen {
                    context: Box::new(crate::error::ErrorContext::new(
                        format!(
                            "CallCapability: method SymbolId({}) has no WASM function index",
                            method_sym.0
                        ),
                        None,
                        crate::error::ErrorType::Codegen,
                        Some(instruction.location.clone()),
                    )),
                });
            };
            self.current_instructions.push(Instruction::Call(fn_idx));
            // Concrete-class methods have an implicit `return this` even
            // when the source method is void — the constructor/method
            // convention pushes `this` (i32) as the return value. When our
            // dispatch block type is Empty (the capability method is void),
            // drop that pushed value to keep the block balanced. When the
            // block type has a Result, we assume the callee left the right
            // value on the stack for the caller to consume.
            if matches!(block_type, wasm_encoder::BlockType::Empty) {
                // Look up the callee's declared return type. Non-void return
                // means the callee left something on the stack we need to drop.
                // Void functions encode as either MirType::Void or the legacy
                // MirType::Ptr(Box::new(MirType::Void)) — both mean "no
                // WASM-level return value". See blocks.rs:288-289 for the
                // same pattern in the function-signature builder. Missing this
                // second form caused a Drop to be emitted after `void draw()`
                // in capability dispatch, tripping `expected a type but
                // nothing on stack` on tests/cln/core/capabilities/009_*.cln
                // (COM001 fp 58bb119efe2e, surfaced when class-method
                // is_static was corrected to include has_capabilities).
                let callee_returns_something = self
                    .function_signatures
                    .get(method_sym)
                    .map(|f| {
                        !matches!(f.return_type, MirType::Void)
                            && !matches!(&f.return_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void))
                    })
                    .unwrap_or(true); // Assume yes when unknown; safer to drop.
                if callee_returns_something {
                    self.current_instructions.push(Instruction::Drop);
                }
            }
            self.current_instructions.push(Instruction::Else);
        }

        // Innermost else: unreachable. Stack shape must match block_type,
        // but `unreachable` is stack-polymorphic in WASM, so no fixup needed.
        self.current_instructions.push(Instruction::Unreachable);

        // Close all the `if` blocks (one End per If we opened).
        for _ in 0..n_arms {
            self.current_instructions.push(Instruction::End);
        }

        // Store the result into the destination local, if any.
        if let Some(dest) = instruction.dest {
            self.store_to_local(dest)?;
        }

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

    /// Returns true when the WASM-registered function with the given
    /// Clean Language name has no result type (i.e. the WASM `call`
    /// pushes nothing onto the stack). Mirrors the name-variant chain
    /// used by `MirOperand::NamedFunction` resolution so the same
    /// Clean Language name a call site uses ("Auth.find_or_create_user",
    /// "list.set", "input.integer", "req.body", …) resolves to the same
    /// WASM type entry. Used by the expression-statement Drop decision
    /// to suppress the Drop when the resolved WASM function returns no
    /// value — see CODEGEN_STACK_REMAINING (fp fa0584d8).
    pub(super) fn wasm_function_is_void(&self, name: &str) -> bool {
        let try_name = |candidate: &str| -> Option<bool> {
            self.wasm_generator
                .get_wasm_return_type(candidate)
                .map(|rt| rt.is_none())
        };

        if let Some(is_void) = try_name(name) {
            return is_void;
        }

        // dot↔underscore conversion (e.g. "input.integer" ↔ "input_integer").
        let alt_name = if name.contains('.') {
            Some(name.replace('.', "_"))
        } else if name.contains('_') {
            Some(name.replace('_', "."))
        } else {
            None
        };
        if let Some(alt) = &alt_name {
            if let Some(is_void) = try_name(alt) {
                return is_void;
            }
        }

        // Strip the first qualifier ("ModuleOrClass.fn" → "fn"). Module
        // imports and user-class methods are registered under the bare
        // method name.
        if let Some(dot_pos) = name.find('.') {
            let bare = &name[dot_pos + 1..];
            if let Some(is_void) = try_name(bare) {
                return is_void;
            }
        }

        // Bridge function alias lookup ("req.body" → "_req_body").
        if let Some(bridge_name) = self.language_to_bridge_map.get(name) {
            if let Some(is_void) = try_name(bridge_name) {
                return is_void;
            }
        }

        false
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
    ///
    /// `operand_type` is used to pick the correct instruction family (i32
    /// vs f64). Negating a `number` (f64) with `0 - x` via i32 ops produces
    /// a type-mismatch validation error, so this path must branch by type.
    pub(super) fn generate_unary_operation(
        &mut self,
        op: &MirUnaryOp,
        operand_type: Option<&MirType>,
    ) -> Result<(), CompilerError> {
        let is_f64 = matches!(operand_type, Some(MirType::F64));
        match op {
            MirUnaryOp::Neg => {
                if is_f64 {
                    // F64Neg is a single-instruction float negation and
                    // preserves IEEE-754 sign correctly (e.g. -0.0, -NaN).
                    self.current_instructions.push(Instruction::F64Neg);
                } else {
                    // Integer negate via two's complement: -x = ~x + 1.
                    // Stackless — no temp local required, and avoids the
                    // earlier bug where `0 - x` was emitted as
                    // `[x, 0] ; i32.sub` = x (identity, not negation).
                    self.current_instructions.push(Instruction::I32Const(-1));
                    self.current_instructions.push(Instruction::I32Xor);
                    self.current_instructions.push(Instruction::I32Const(1));
                    self.current_instructions.push(Instruction::I32Add);
                }
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
    ///
    /// Handles boxed Any tag layout: `[tag@0][value1@4][value2@8]`, tags:
    /// 1=Integer, 2=Boolean, 3=Number(f64), 4=String, 5=List, 6=Object.
    ///
    /// - tag=3 (Number): f64 at offset 4+8, truncate to i32
    /// - tag=4 (String): value1 is an LP-string pointer; parse via `string_to_int`
    /// - anything else: read value1 (offset 4) as raw i32
    pub(super) fn emit_unbox_to_i32(&mut self) -> Result<(), CompilerError> {
        debug_mir!("Unboxing any value to i32");

        // Resolve `string_to_int` before emitting any instructions so a missing
        // helper is a clean codegen error rather than an invalid WASM.
        let string_to_int_idx = *self
            .wasm_generator
            .function_map
            .get("string_to_int")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "string_to_int function not found in function_map for Any->i32 unbox"
                        .to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

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

        // Not tag=3. Now check tag=4 (String): value1 is an LP-string pointer
        // (see AnyTypeTag::String in src/mir/mir_types.rs). Without this branch
        // `json.get(blob, key).toInteger()` returned the LP-string address as
        // the integer — resolves CODEGEN-UNBOX-TO-I32-MISSING-STRING-TAG-CASE
        // (#0ccc47714523) and its downstream node-server symptom
        // BRIDGE-JSON-GET-INTEGER-RETURNS-POINTER-AGGREGATE-QUERY (#61ef80a34ec6).
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions.push(Instruction::I32Const(4));
        self.current_instructions.push(Instruction::I32Eq);
        self.current_instructions
            .push(Instruction::If(wasm_encoder::BlockType::Result(
                ValType::I32,
            )));

        // Type tag is 4 (String): value1 at offset 4 is an LP-string pointer.
        // string_to_int walks the length prefix internally, so hand it the
        // pointer directly.
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions
            .push(Instruction::Call(string_to_int_idx));

        self.current_instructions.push(Instruction::Else);

        // Fallback (tag 1 = Integer, anything unknown): read value1@4 as i32.
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }));

        self.current_instructions.push(Instruction::End); // close tag==4 else
        self.current_instructions.push(Instruction::End); // close tag==3 else

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
    /// Emit code to unbox an Any value to f64.
    ///
    /// - tag=3 (Number): value stored as two i32 halves (value1@4, value2@8);
    ///   combine and reinterpret as f64 (the pre-fix path, kept for Numbers).
    /// - tag=4 (String): value1 is an LP-string pointer; parse via
    ///   `string_to_float` — resolves the f64 twin of
    ///   CODEGEN-UNBOX-TO-I32-MISSING-STRING-TAG-CASE (#0ccc47714523).
    /// - tag=1 (Integer): value1@4 is an i32; convert to f64.
    ///
    /// Any other tag falls through to the Number path (existing behaviour),
    /// which is wrong for Booleans / Lists / Objects but out of scope here.
    pub(super) fn emit_unbox_to_f64(&mut self) -> Result<(), CompilerError> {
        debug_mir!("Unboxing any value to f64");

        // Resolve `string_to_float` up front so a missing helper is a codegen
        // error rather than an invalid WASM at runtime.
        let string_to_float_idx = *self
            .wasm_generator
            .function_map
            .get("string_to_float")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "string_to_float function not found in function_map for Any->f64 unbox"
                        .to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

        // Save pointer to temp
        let ptr_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(ptr_local, ValType::I32);
        self.current_instructions
            .push(Instruction::LocalSet(ptr_local));

        // Read tag
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));

        // tag == 4 (String)?
        self.current_instructions.push(Instruction::I32Const(4));
        self.current_instructions.push(Instruction::I32Eq);
        self.current_instructions
            .push(Instruction::If(wasm_encoder::BlockType::Result(
                ValType::F64,
            )));

        // Type tag is 4 (String): value1@4 is LP-string pointer, parse via
        // string_to_float. Without this, `.toNumber()` on a json.get() result
        // returned garbage f64 bits synthesised from an LP-pointer.
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions
            .push(Instruction::Call(string_to_float_idx));

        self.current_instructions.push(Instruction::Else);

        // Not String. Now check tag == 1 (Integer)?
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions.push(Instruction::I32Const(1));
        self.current_instructions.push(Instruction::I32Eq);
        self.current_instructions
            .push(Instruction::If(wasm_encoder::BlockType::Result(
                ValType::F64,
            )));

        // Type tag is 1 (Integer): value1@4 is a signed i32, promote to f64.
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions.push(Instruction::F64ConvertI32S);

        self.current_instructions.push(Instruction::Else);

        // Fallback (tag 3 = Number): value stored as two i32 halves at 4 and 8.
        // Combine and reinterpret as f64. This is the pre-fix path, kept for
        // Number-tagged Anys (which don't fit in a single i32).
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions.push(Instruction::I64ExtendI32U);

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

        self.current_instructions.push(Instruction::I64Or);
        self.current_instructions
            .push(Instruction::F64ReinterpretI64);

        self.current_instructions.push(Instruction::End); // close tag==1 else
        self.current_instructions.push(Instruction::End); // close tag==4 else

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

        // Create a result local
        let result_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(result_local, ValType::I32);

        // Null-guard the boxed pointer BEFORE reading the tag. `json.get`,
        // `__json_get_path`, and other Any-returning bridges return 0 on
        // miss/OOB. `emit_read_any_tag` would then load memory[0], which is
        // zeroed on the standalone runner but contains WASM data-segment
        // bytes on real hosts (clean-server, clean-node-server). A non-zero
        // byte there gets misinterpreted as a valid tag (e.g. 4 = String),
        // routing execution into the String branch which reads mem[ptr+4]
        // at address 4 — garbage. The reporter's `while item != ""` loop
        // then compares against garbage-length strings and either loops
        // forever (OOM/stack trap) or terminates non-deterministically.
        // See WASM-TRAP-JSON-ARRAY-ITER fp 156e745b63d9.
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions.push(Instruction::I32Eqz);
        self.current_instructions
            .push(Instruction::If(BlockType::Empty));
        {
            // Allocate a fresh 4-byte length-prefixed empty string.
            if let Some(&malloc_idx) = self.wasm_generator.function_map.get("malloc") {
                self.current_instructions.push(Instruction::I32Const(4));
                self.current_instructions
                    .push(Instruction::Call(malloc_idx));
                self.current_instructions
                    .push(Instruction::LocalTee(result_local));
                self.current_instructions.push(Instruction::I32Const(0));
                self.current_instructions
                    .push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
            } else {
                // No malloc registered — extremely unlikely; leave 0 and
                // let the caller trip its own null-guard.
                self.current_instructions.push(Instruction::I32Const(0));
                self.current_instructions
                    .push(Instruction::LocalSet(result_local));
            }
        }
        self.current_instructions.push(Instruction::Else);

        // Read the tag (non-null path)
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.emit_read_any_tag()?;

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
                        // String case: value1 (offset 4) is already an LP-string
                        // pointer. Read it directly rather than routing through
                        // `emit_unbox_to_i32` — the latter's tag==4 branch (added
                        // in f9b25f08 to fix .toInteger() on Any(String)) PARSES
                        // the LP-string via `string_to_int`, which would turn
                        // every string like "row0" into 0, losing the pointer.
                        // Regression: `codegen_html_interp_cross_iter_recur`,
                        // `codegen_string_arg_alias_jsonget_multi_concat`, and
                        // the string-accumulator tests all read Any(String)
                        // results into `string` locals via this path.
                        self.current_instructions
                            .push(Instruction::LocalGet(ptr_local));
                        self.current_instructions.push(Instruction::I32Load(
                            wasm_encoder::MemArg {
                                offset: 4,
                                align: 2,
                                memory_index: 0,
                            },
                        ));
                        self.current_instructions
                            .push(Instruction::LocalSet(result_local));
                    }
                    self.current_instructions.push(Instruction::Else);
                    {
                        // Default case: Null (ptr==0), Array (tag 5),
                        // Object (tag 6), or any other tag.
                        //
                        // Null → empty length-prefixed string. The
                        // common shape `while catJ != "": catJ =
                        // json.get(arr, i.toString())` depends on
                        // out-of-bounds / missing-key lookups yielding
                        // `""` to terminate the walk. Returning the
                        // literal text "null" (what json.dataToText
                        // would emit) breaks termination.
                        //
                        // Array / Object → call `json.dataToText` to
                        // produce the canonical JSON text. The legacy
                        // emission (`int_to_string(ptr+4)`) treated the
                        // inner pointer as an integer, producing
                        // nonsensical decimal output or — far worse —
                        // garbage when a downstream consumer read those
                        // bytes as a length-prefixed Clean string. That
                        // is the SSR empty-content root cause surfaced
                        // by 0.30.379's CMP-SSR-MALLOC-OOM-PAGE-RENDER
                        // fix: `string catJ = json.get(catJson, "0")`
                        // on an array of objects unboxed to the inner
                        // object header pointer, and the next read of
                        // catJ.length / catJ[0] returned header bytes,
                        // not the JSON text.
                        self.current_instructions
                            .push(Instruction::LocalGet(ptr_local));
                        self.current_instructions.push(Instruction::I32Eqz);
                        self.current_instructions
                            .push(Instruction::If(BlockType::Empty));
                        {
                            // Null: allocate a fresh 4-byte empty
                            // length-prefixed string (length=0 at
                            // offset 0, no data bytes).
                            if let Some(&malloc_idx) =
                                self.wasm_generator.function_map.get("malloc")
                            {
                                let empty_local = self.next_local_index;
                                self.next_local_index += 1;
                                self.temp_local_types.insert(empty_local, ValType::I32);
                                self.current_instructions.push(Instruction::I32Const(4));
                                self.current_instructions
                                    .push(Instruction::Call(malloc_idx));
                                self.current_instructions
                                    .push(Instruction::LocalTee(empty_local));
                                self.current_instructions.push(Instruction::I32Const(0));
                                self.current_instructions.push(Instruction::I32Store(
                                    wasm_encoder::MemArg {
                                        offset: 0,
                                        align: 2,
                                        memory_index: 0,
                                    },
                                ));
                                self.current_instructions
                                    .push(Instruction::LocalGet(empty_local));
                                self.current_instructions
                                    .push(Instruction::LocalSet(result_local));
                            } else {
                                // No malloc available (shouldn't happen
                                // in practice — malloc is always
                                // registered). Last-resort: a null
                                // pointer. Downstream string ops will
                                // trap, which is fine because this
                                // branch is unreachable.
                                self.current_instructions.push(Instruction::I32Const(0));
                                self.current_instructions
                                    .push(Instruction::LocalSet(result_local));
                            }
                        }
                        self.current_instructions.push(Instruction::Else);
                        {
                            // Non-null Array/Object/other: stringify via
                            // json.dataToText (registered as a stdlib
                            // export; see `src/stdlib/json_class.rs`).
                            if let Some(&data_to_text_idx) =
                                self.wasm_generator.function_map.get("json.dataToText")
                            {
                                self.current_instructions
                                    .push(Instruction::LocalGet(ptr_local));
                                self.current_instructions
                                    .push(Instruction::Call(data_to_text_idx));
                                self.current_instructions
                                    .push(Instruction::LocalSet(result_local));
                            } else {
                                // Fallback (no json.dataToText
                                // available — stripped build).
                                self.current_instructions
                                    .push(Instruction::LocalGet(ptr_local));
                                self.emit_unbox_to_i32()?;
                                self.current_instructions
                                    .push(Instruction::Call(int_to_string_idx));
                                self.current_instructions
                                    .push(Instruction::LocalSet(result_local));
                            }
                        }
                        self.current_instructions.push(Instruction::End); // End null check
                    }
                    self.current_instructions.push(Instruction::End); // End String if
                }
                self.current_instructions.push(Instruction::End); // End Number if
            }
            self.current_instructions.push(Instruction::End); // End Boolean if
        }
        self.current_instructions.push(Instruction::End); // End Integer if
        self.current_instructions.push(Instruction::End); // End null-guard if

        // Push the result onto the stack
        self.current_instructions
            .push(Instruction::LocalGet(result_local));

        debug_mir!("any.toString() dispatch complete");
        Ok(())
    }
}

/// Build a "did you mean X?" hint for a not-found function name.
///
/// Uses:
/// - A curated list of common legacy misnames (parseInt, toInt, floatVal, …).
/// - Falls back to a Levenshtein-distance search over the actual function_map
///   keys so the hint always names something that really exists in this build.
///
/// Reported bugs this helps: b6d1b80449d8, 5cdbcb58ee83 (`string.toInt`),
/// ceb568e0aaa7, 8653d059d75d (`parseInt`), 6e43af3832db (`s.replace` — already
/// aliased in current builds but appears when a user is on an older cln).
fn build_did_you_mean_hint(
    target: &str,
    function_map: &std::collections::HashMap<String, u32>,
) -> Option<String> {
    // Curated fast-path aliases (misname → canonical Clean name).
    // Kept small and only for high-signal cases so the hint stays trustworthy.
    let curated: &[(&str, &str)] = &[
        ("parseInt", "string.toInteger"),
        ("parseInteger", "string.toInteger"),
        ("parseFloat", "string.toNumber"),
        ("parseNumber", "string.toNumber"),
        ("parseBoolean", "string.toBoolean"),
        ("string.toInt", "string.toInteger"),
        ("string.toFloat", "string.toNumber"),
        ("string.parseInt", "string.toInteger"),
        ("string.parseFloat", "string.toNumber"),
        ("Integer.parse", "string.toInteger"),
        ("Number.parse", "string.toNumber"),
    ];
    for (misname, canonical) in curated {
        if *misname == target && function_map.contains_key(*canonical) {
            return Some(format!(
                "'{}' is not a Clean built-in — use `{}` instead. See foundation/spec/stdlib-reference.md.",
                target, canonical
            ));
        }
    }

    // Fuzzy match: search function_map for near-neighbours of `target`.
    let keys: Vec<&str> = function_map.keys().map(String::as_str).collect();
    let suggestions = crate::error::ErrorUtils::suggest_similar_names(target, &keys, 3);
    if suggestions.is_empty() {
        None
    } else {
        Some(suggestions.join(" "))
    }
}
