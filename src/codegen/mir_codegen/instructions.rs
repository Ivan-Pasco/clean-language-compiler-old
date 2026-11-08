//! MIR instruction-to-WASM emission.
//!
//! Contains `generate_instruction` and all helpers that
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

            MirOperation::BinaryOp { op, left, right }
                if self.any_null_comparison(op, left, right).is_some() =>
            {
                // `value == null` / `value != null` on an `any`: a null
                // pointer and a null-tag box (a JSON `null`, a `_json_get`
                // miss) are both null.
                let any_side = self
                    .any_null_comparison(op, left, right)
                    .expect("guarded by the match arm");
                self.load_operand(any_side)?;
                self.emit_any_is_null();
                if matches!(op, MirBinaryOp::Ne) {
                    self.current_instructions.push(Instruction::I32Eqz);
                }
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                } else {
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::BinaryOp { op, left, right } => {
                // NOTE: Type-aware binary operations with automatic conversions
                let left_is_float = self.is_float_operand(left);
                let right_is_float = self.is_float_operand(right);
                // `integer:64` operands are i64 in WASM. When either side is
                // i64 (and neither is a float), the operation runs in i64 and
                // the i32 side is sign-extended (integer widening, SEM001).
                let left_is_i64 = self.is_i64_operand(left);
                let right_is_i64 = self.is_i64_operand(right);
                let use_i64 = !left_is_float
                    && !right_is_float
                    && (left_is_i64 || right_is_i64)
                    && !self.is_string_operand(left)
                    && !self.is_string_operand(right);

                // Load left operand
                self.load_operand(left)?;
                // Convert left to f64 if right is float and left is not
                if !left_is_float && right_is_float {
                    self.current_instructions.push(if left_is_i64 {
                        Instruction::F64ConvertI64S
                    } else {
                        Instruction::F64ConvertI32S
                    });
                }
                if use_i64 && !left_is_i64 {
                    self.current_instructions.push(Instruction::I64ExtendI32S);
                }

                // Load right operand
                self.load_operand(right)?;
                // Convert right to f64 if left is float and right is not
                if left_is_float && !right_is_float {
                    self.current_instructions.push(if right_is_i64 {
                        Instruction::F64ConvertI64S
                    } else {
                        Instruction::F64ConvertI32S
                    });
                }
                if use_i64 && !right_is_i64 {
                    self.current_instructions.push(Instruction::I64ExtendI32S);
                }

                // Generate the operation
                if use_i64 {
                    self.generate_i64_binary_operation(op);
                } else {
                    self.generate_binary_operation(op, left, right)?;
                }
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
                    // The destination type decides the width of the load.
                    let dest_type = self
                        .value_to_type
                        .get(&dest)
                        .cloned()
                        .unwrap_or(MirType::I32);
                    self.current_instructions
                        .push(memory_load_instruction(&dest_type));

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
                        .push(memory_load_instruction(&MirType::I32));
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::Store { destination, value } => {
                // Store to memory
                self.load_operand(destination)?;
                self.load_operand(value)?;

                // The value type decides the width of the store.
                let value_type = match value {
                    MirOperand::Value(value_id) => self.value_to_type.get(value_id).cloned(),
                    other => self.get_operand_mir_type(other),
                }
                .unwrap_or(MirType::I32);
                self.current_instructions
                    .push(memory_store_instruction(&value_type));
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

                // A `later` binding whose expression is not a call (a call
                // is deferred to a task, `mir_builder/async_tasks.rs`): the
                // MIR builder has already evaluated it, so the binding is a
                // copy.
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
                self.emit_any_unbox(AnyUnboxTarget::I32)?;

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
                self.emit_any_unbox(AnyUnboxTarget::F64)?;

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

            MirOperation::UnboxAnyToI64 { value } => {
                self.load_operand(value)?;
                self.emit_any_unbox(AnyUnboxTarget::I64)?;
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                } else {
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::UnboxAnyToBoolean { value } => {
                self.load_operand(value)?;
                self.emit_any_unbox(AnyUnboxTarget::Bool)?;
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                } else {
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::AnyGetField { object, key } => {
                debug_mir!(?object, ?key, "Processing AnyGetField (JSON object access)");

                // The boxed `any` (`__json_get_field` checks its tag).
                self.load_operand(object)?;

                // Load the key string and expand to (content_ptr, len) format
                self.load_string_argument_for_print(key)?;

                // Call __json_get_field(any_ptr: i32, key_ptr: i32, key_len: i32) -> i32
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

                // The boxed `any` (`__json_get_index` checks its tag).
                self.load_operand(array)?;

                // Load the integer index
                self.load_operand(index)?;

                // Call __json_get_index(any_ptr: i32, index: i32) -> i32
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

                    // `integer:64` conversions (type-system.md §5: integer
                    // widening, sized→unsized, integer→number).
                    (Some(MirType::I32 | MirType::Bool), MirType::I64 | MirType::U64) => {
                        self.current_instructions.push(Instruction::I64ExtendI32S);
                    }
                    (Some(MirType::I64 | MirType::U64), MirType::I32) => {
                        self.current_instructions.push(Instruction::I32WrapI64);
                    }
                    (Some(MirType::I64 | MirType::U64), MirType::F64) => {
                        self.current_instructions.push(Instruction::F64ConvertI64S);
                    }
                    (Some(MirType::F64), MirType::I64 | MirType::U64) => {
                        self.current_instructions.push(Instruction::I64TruncF64S);
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
                    // Load the value to store, converted to the global's type
                    // (an `integer` literal stored into an `integer:64` state).
                    self.load_operand(value)?;
                    if let Some(src) = self.get_operand_mir_type(value) {
                        self.emit_numeric_coercion(&src, global_type);
                    }

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
                // A branch emitted by the rhs instructions (an `onError`
                // unwinding check) counts this `if` as a label.
                self.current_block_depth += 1;

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
                self.current_block_depth -= 1;
                self.current_instructions.push(Instruction::End);

                // Store the result in the dest local.
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                } else {
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            // `onError` unwinding after a call that may raise: while an error
            // is pending, branch to the exit of the innermost `onError` region
            // of this function (emitted as a loop, so the exit is a loop exit
            // label) or return the zero value of the function's type.
            MirOperation::ErrorCheck { region_exit } => {
                self.current_instructions.extend([
                    Instruction::GlobalGet(crate::codegen::native_stdlib::ERROR_STATE_GLOBAL),
                    Instruction::I32Const(1),
                    Instruction::I32And,
                    Instruction::If(BlockType::Empty),
                ]);
                self.current_block_depth += 1;
                match region_exit {
                    Some(exit) => {
                        let region_depth = self
                            .loop_context_stack
                            .iter()
                            .rev()
                            .find(|l| l.exit_block_id == *exit)
                            .map(|l| l.block_depth)
                            .ok_or_else(|| {
                                CompilerError::codegen_error(
                                    format!(
                                        "onError region exit {:?} is not an enclosing loop exit",
                                        exit
                                    ),
                                    None,
                                    None,
                                )
                            })?;
                        self.current_instructions
                            .push(Instruction::Br(self.current_block_depth - region_depth));
                    }
                    None => {
                        let return_type = self
                            .current_function
                            .as_ref()
                            .map(|f| f.return_type.clone());
                        self.push_zero_for_return_type(&return_type);
                        self.current_instructions.push(Instruction::Return);
                    }
                }
                self.current_block_depth -= 1;
                self.current_instructions.push(Instruction::End);
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
            // Math and number-conversion runtime functions take `number`
            // (f64) parameters (stdlib-reference.md §2), with integer
            // variants (`math.abs.i32`): each argument converts to the
            // parameter type the callee is registered with. (A user function
            // named `number...` is not a runtime function and takes the
            // generic path below.)
            Some(name)
                if (name.starts_with("math.")
                    || name.starts_with("number")
                    || name == "float_to_string"
                    || name == "integer.toNumber")
                    && self.wasm_generator.function_param_types(name).is_some() =>
            {
                let params = self
                    .wasm_generator
                    .function_param_types(name)
                    .unwrap_or_default();
                for (i, arg) in arguments.iter().enumerate() {
                    self.load_operand(arg)?;
                    let param = params.get(i).map(|param| match param {
                        crate::types::WasmType::F64 => MirType::F64,
                        crate::types::WasmType::I64 => MirType::I64,
                        crate::types::WasmType::F32 => MirType::F32,
                        _ => MirType::I32,
                    });
                    if let (Some(src), Some(dst)) = (self.get_operand_mir_type(arg), param) {
                        self.emit_numeric_coercion(&src, &dst);
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
                        self.emit_handler_argument(arg)?;
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

                            // `integer:64` on either side (integer widening,
                            // integer:64 → integer / number; type-system.md §5).
                            let arg_type = self.get_operand_mir_type(arg);
                            let involves_i64 = |t: &MirType| matches!(t, MirType::I64 | MirType::U64);
                            if let Some(src) = arg_type.as_ref() {
                                if involves_i64(&expected_param.param_type) || involves_i64(src) {
                                    self.emit_numeric_coercion(src, &expected_param.param_type);
                                }
                            }

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

                            // An `any` parameter takes a boxed value
                            // (BOXED_ANY_ABI). The MIR builder boxes `any`
                            // arguments; a float or `integer:64` that still
                            // arrives unboxed cannot be a box pointer, so it
                            // is boxed here as a number.
                            if matches!(expected_param.param_type, MirType::Any) {
                                if let Some(
                                    source @ (MirType::F64
                                    | MirType::F32
                                    | MirType::I64
                                    | MirType::U64),
                                ) = arg_type.as_ref()
                                {
                                    self.emit_box_value(AnyTypeTag::Number, source)?;
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

                        // An emitted `maps_to_helper` target wins over every other
                        // lookup (see the NamedFunction arm below for why).
                        let helper_index = self
                            .language_to_helper_map
                            .get(&function_name)
                            .and_then(|helper_name| {
                                self.wasm_generator.function_map.get(helper_name).copied()
                            });
                        let function_index = if helper_index.is_some() {
                            helper_index
                        } else if let Some(&idx) =
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

                    // An explicit `maps_to_helper` declaration whose helper was
                    // emitted in this build names the exact function the owning
                    // plugin generated for this language name, so it wins over a
                    // bridge alias of the same name from another plugin
                    // (frame.server's basic `auth.can` vs frame.auth's roles-aware
                    // helper) and over the name-shape fallbacks below
                    // ("auth.jwt.sign" stripped to "jwt.sign" would otherwise hit
                    // the raw `_jwt_sign` bridge wrapper).
                    let helper_index =
                        self.language_to_helper_map
                            .get(name)
                            .and_then(|helper_name| {
                                self.wasm_generator.function_map.get(helper_name).copied()
                            });
                    let function_index = if helper_index.is_some() {
                        helper_index
                    } else if let Some(&idx) = self.wasm_generator.function_map.get(name) {
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
                                    | Some("mem_release")
                                    | Some("mem_retain")
                                    | Some("mem_scope_push")
                                    | Some("mem_scope_pop")
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
                        | Some("mem_release")
                        | Some("mem_retain")
                        | Some("mem_scope_push")
                        | Some("mem_scope_pop")
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

        // The singleshot accumulator rewrite builds through the transient
        // arena (`__string_builder_append_transient` and
        // `__string_builder_finalize_to_main`); those are probed too, or a
        // program whose strings all go through the arena records nothing.
        let append_transient_idx = self
            .wasm_generator
            .function_map
            .get("__string_builder_append_transient")
            .copied();
        let finalize_to_main_idx = self
            .wasm_generator
            .function_map
            .get("__string_builder_finalize_to_main")
            .copied();

        let callee_label: &str = if Some(function_index) == append_idx {
            "string_builder_append"
        } else if Some(function_index) == finalize_idx {
            "string_builder_finalize"
        } else if Some(function_index) == append_transient_idx {
            "string_builder_append_transient"
        } else if Some(function_index) == finalize_to_main_idx {
            "string_builder_finalize_to_main"
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
        let scratch = self.alloc_temp_local(ValType::I32);

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
                let scratch = self.alloc_temp_local(ValType::I32);
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
            // No class in this build declares `can <Cap>`. The dispatch site
            // is provably unreachable at runtime (there is no receiver value
            // whose class-id would match any arm), so emit `unreachable`
            // rather than failing compilation. This matches the WASM spec's
            // stack-polymorphic semantics for `unreachable` and unblocks
            // callers that declare a capability-typed API without a concrete
            // implementer in-scope — a common shape for plugin-emitted
            // helpers (frame.data v3.0.12+ `Database.save(Persist target)`,
            // team-prompt d227e2be) and for library-style capabilities that
            // downstream code will conform to later.
            //
            // If the destination local exists, drop it from any preceding
            // load — `unreachable` is stack-polymorphic so no fixup is
            // needed for stack shape. The dest local stays unwritten;
            // subsequent reads of it will use whatever the local's default
            // is (0 for i32, 0.0 for f64), but they are also unreachable
            // per the same argument.
            //
            // Users get no compile-time warning today. A future Contract 5
            // Phase B `lint` bridge can flag empty-vtable dispatch as
            // FRAME-DATA-style informational so the friction stays low
            // while surfacing the issue at the call site.
            self.current_instructions.push(Instruction::Unreachable);
            return Ok(());
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
        let receiver_local = self.alloc_temp_local(ValType::I32);
        self.current_instructions
            .push(Instruction::LocalSet(receiver_local));

        // Emit an if/else-if chain over class_ids.
        // Structure per arm:
        //   load receiver header
        //   i32.const <CLASS_HEADER_MARK | class_id>
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
            // Load the receiver's object header (i32 at offset 0).
            self.current_instructions
                .push(Instruction::LocalGet(receiver_local));
            self.current_instructions
                .push(Instruction::I32Load(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2, // 4-byte aligned
                    memory_index: 0,
                }));
            self.current_instructions
                .push(Instruction::I32Const(crate::mir::class_header_word(
                    *class_id,
                )));
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

    /// Push a handler argument: the handler index of a function reference
    /// (exported as `handle_event_N`), or an integer handler index as is.
    pub(super) fn emit_handler_argument(&mut self, arg: &MirOperand) -> Result<(), CompilerError> {
        if let MirOperand::Value(value_id) = arg {
            if self.funcref_local_name(*value_id).is_none() {
                // Plugin-generated code like `_http_route("GET", "/", 0)`
                // passes the handler index as an integer value.
                let local_index = *self.value_to_local.get(value_id).ok_or_else(|| {
                    CompilerError::codegen_error(
                        format!("Handler argument Value({:?}) not found in locals", value_id),
                        None,
                        None,
                    )
                })?;
                self.current_instructions
                    .push(Instruction::LocalGet(local_index));
                return Ok(());
            }
        }
        let index = self.resolve_handler_index(arg)?;
        self.current_instructions
            .push(Instruction::I32Const(index as i32));
        Ok(())
    }

    /// The function name of a `funcref_<name>` local: the MIR builder emits
    /// function references as values held in such locals.
    fn funcref_local_name(&self, value_id: ValueId) -> Option<String> {
        self.current_function
            .as_ref()
            .and_then(|f| f.locals.get(&value_id))
            .and_then(|l| l.name.as_ref())
            .and_then(|name| name.strip_prefix("funcref_"))
            .map(str::to_string)
    }

    /// The handler index of a handler argument that names a function (or is
    /// an integer literal).
    fn resolve_handler_index(&mut self, arg: &MirOperand) -> Result<u32, CompilerError> {
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
            MirOperand::Value(value_id) => self.funcref_local_name(*value_id).ok_or_else(|| {
                CompilerError::codegen_error(
                    format!("Handler argument Value({:?}) is not a function reference", value_id),
                    None,
                    None,
                )
            })?,
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
            // Strings compare by content. Comparing the pointers would make two
            // equal strings built at runtime unequal, so a missing import is a
            // compiler error (the reachability scan must have kept it).
            let compare_idx = self
                .wasm_generator
                .get_function_index("string_compare")
                .ok_or_else(|| {
                    CompilerError::codegen_error(
                        "string equality comparison requires the string_compare import, \
                         which was not registered"
                            .to_string(),
                        None,
                        None,
                    )
                })?;
            // string_compare returns 0 for equal, non-zero otherwise.
            self.current_instructions
                .push(Instruction::Call(compare_idx));
            // `==` inverts (0 -> 1); `!=` uses the non-zero result as is.
            if matches!(op, MirBinaryOp::Eq) {
                self.current_instructions.push(Instruction::I32Eqz);
            }
            return Ok(());
        }

        // Relational operators on strings order them lexicographically:
        // `string_compare` returns -1 / 0 / 1 (function-registry.toml), which is
        // compared against 0. Comparing the raw pointers would order strings by
        // where they happen to live in memory.
        if is_string
            && matches!(
                op,
                MirBinaryOp::Lt | MirBinaryOp::Le | MirBinaryOp::Gt | MirBinaryOp::Ge
            )
        {
            let compare_idx = self
                .wasm_generator
                .get_function_index("string_compare")
                .ok_or_else(|| {
                    CompilerError::codegen_error(
                        "string ordering comparison requires the string_compare import, \
                         which was not registered"
                            .to_string(),
                        None,
                        None,
                    )
                })?;
            self.current_instructions
                .push(Instruction::Call(compare_idx));
            self.current_instructions.push(Instruction::I32Const(0));
            self.current_instructions.push(match op {
                MirBinaryOp::Lt => Instruction::I32LtS,
                MirBinaryOp::Le => Instruction::I32LeS,
                MirBinaryOp::Gt => Instruction::I32GtS,
                _ => Instruction::I32GeS,
            });
            return Ok(());
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
                    // WASM has no f64 remainder: a % b = a - trunc(a / b) * b
                    // (sign of the dividend, like the integer `rem_s`).
                    let divisor = self.alloc_temp_local(ValType::F64);
                    let dividend = self.alloc_temp_local(ValType::F64);
                    self.current_instructions.extend([
                        Instruction::LocalSet(divisor),
                        Instruction::LocalTee(dividend),
                        Instruction::LocalGet(dividend),
                        Instruction::LocalGet(divisor),
                        Instruction::F64Div,
                        Instruction::F64Trunc,
                        Instruction::LocalGet(divisor),
                        Instruction::F64Mul,
                    ]);
                    Instruction::F64Sub
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

    /// Emit the i64 form of an integer binary operation. Both operands are
    /// already on the stack as i64. Arithmetic and bitwise results are i64;
    /// comparisons produce an i32 boolean, as in the i32 path.
    pub(super) fn generate_i64_binary_operation(&mut self, op: &MirBinaryOp) {
        let instruction = match op {
            MirBinaryOp::Add => Instruction::I64Add,
            MirBinaryOp::Sub => Instruction::I64Sub,
            MirBinaryOp::Mul => Instruction::I64Mul,
            MirBinaryOp::Div => Instruction::I64DivS,
            MirBinaryOp::Rem => Instruction::I64RemS,
            MirBinaryOp::Eq => Instruction::I64Eq,
            MirBinaryOp::Ne => Instruction::I64Ne,
            MirBinaryOp::Lt => Instruction::I64LtS,
            MirBinaryOp::Le => Instruction::I64LeS,
            MirBinaryOp::Gt => Instruction::I64GtS,
            MirBinaryOp::Ge => Instruction::I64GeS,
            MirBinaryOp::And => Instruction::I64And,
            MirBinaryOp::Or => Instruction::I64Or,
            MirBinaryOp::Xor => Instruction::I64Xor,
            MirBinaryOp::Shl => Instruction::I64Shl,
            MirBinaryOp::Shr => Instruction::I64ShrS,
        };
        self.current_instructions.push(instruction);
    }

    /// Helper: Check if an operand is a 64-bit integer (`integer:64`).
    pub(super) fn is_i64_operand(&self, operand: &MirOperand) -> bool {
        matches!(
            self.get_operand_mir_type(operand),
            Some(MirType::I64 | MirType::U64)
        )
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
        let is_i64 = matches!(operand_type, Some(MirType::I64 | MirType::U64));
        match op {
            MirUnaryOp::Neg => {
                if is_f64 {
                    // F64Neg is a single-instruction float negation and
                    // preserves IEEE-754 sign correctly (e.g. -0.0, -NaN).
                    self.current_instructions.push(Instruction::F64Neg);
                } else if is_i64 {
                    // `integer:64`: the same two's complement, in i64.
                    self.current_instructions.extend([
                        Instruction::I64Const(-1),
                        Instruction::I64Xor,
                        Instruction::I64Const(1),
                        Instruction::I64Add,
                    ]);
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
                if is_i64 {
                    self.current_instructions.push(Instruction::I64Const(-1));
                    self.current_instructions.push(Instruction::I64Xor);
                } else {
                    self.current_instructions.push(Instruction::I32Const(-1));
                    self.current_instructions.push(Instruction::I32Xor);
                }
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

        // The box payload is an i32, an i64 or an f64. `integer:64` and
        // `number:32` values are stored as a `number` (tag 3), the same as a
        // JSON number — unless the caller asks for an `integer` box, which
        // holds the full i64 (BOXED_ANY_ABI §2; class JSON values).
        let widened = match source_type {
            MirType::I64 if tag == AnyTypeTag::Integer => None,
            MirType::I64 => Some(Instruction::F64ConvertI64S),
            MirType::U64 => Some(Instruction::F64ConvertI64U),
            MirType::F32 => Some(Instruction::F64PromoteF32),
            _ => None,
        };
        let (tag, source_type) = match widened {
            Some(instruction) => {
                self.current_instructions.push(instruction);
                (AnyTypeTag::Number, &MirType::F64)
            }
            None => (tag, source_type),
        };

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

        // Store the value to a temporary local
        let temp_local = self.alloc_temp_local(match source_type {
            MirType::F64 => ValType::F64,
            MirType::I64 => ValType::I64,
            _ => ValType::I32,
        });
        self.current_instructions
            .push(Instruction::LocalSet(temp_local));

        // Allocate 12 bytes for the boxed structure
        // mem_alloc(type_id=0, size=12)
        self.current_instructions.push(Instruction::I32Const(0)); // type_id
        self.current_instructions.push(Instruction::I32Const(12)); // size
        self.current_instructions
            .push(Instruction::Call(mem_alloc_idx));

        // Save the pointer to another temp local
        let ptr_local = self.alloc_temp_local(ValType::I32);
        self.current_instructions
            .push(Instruction::LocalTee(ptr_local));

        // Store the tag at offset 0
        // Stack: [ptr]
        self.current_instructions
            .push(Instruction::I32Const(tag.as_i32()));
        if tag == AnyTypeTag::Object && !matches!(source_type, MirType::F64 | MirType::I64) {
            // A null instance (`Post?` holding null) boxes as null: an
            // object payload of 0 is not a value (BOXED_ANY_ABI §6), and
            // `== null` reads the tag.
            self.current_instructions
                .push(Instruction::I32Const(AnyTypeTag::Null.as_i32()));
            self.current_instructions
                .push(Instruction::LocalGet(temp_local));
            self.current_instructions.push(Instruction::Select);
        }
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
            MirType::I64 => {
                // An `integer` box of an `integer:64`: the i64 spans offsets 4..12.
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions
                    .push(Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 4,
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

                // value2 at offset 8: an integer's payload is the full i64
                // (BOXED_ANY_ABI §2), so it holds the sign extension of a
                // signed i32; every other i32 payload leaves it 0.
                let unsigned = matches!(source_type, MirType::U8 | MirType::U16 | MirType::U32);
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                if tag == AnyTypeTag::Integer && !unsigned {
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(31));
                    self.current_instructions.push(Instruction::I32ShrS);
                } else {
                    self.current_instructions.push(Instruction::I32Const(0));
                }
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

    /// For `a == b` / `a != b` where one side is an `any` and the other the
    /// `null` literal, the `any` side; `None` for every other comparison.
    pub(super) fn any_null_comparison<'a>(
        &self,
        op: &MirBinaryOp,
        left: &'a MirOperand,
        right: &'a MirOperand,
    ) -> Option<&'a MirOperand> {
        if !matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne) {
            return None;
        }
        let is_null = |operand: &MirOperand| match operand {
            MirOperand::Constant(MirConstant::Null) => true,
            MirOperand::Value(_) => matches!(
                self.get_operand_mir_type(operand),
                Some(MirType::Ptr(inner)) if *inner == MirType::Void
            ),
            _ => false,
        };
        let is_any =
            |operand: &MirOperand| self.get_operand_mir_type(operand) == Some(MirType::Any);
        if is_any(left) && is_null(right) {
            Some(left)
        } else if is_any(right) && is_null(left) {
            Some(right)
        } else {
            None
        }
    }

    /// With a boxed `any` pointer on the stack, leave 1 when it is null — a
    /// null pointer or a null-tag box (BOXED_ANY_ABI §1) — and 0 otherwise.
    pub(super) fn emit_any_is_null(&mut self) {
        let ptr_local = self.alloc_temp_local(ValType::I32);
        self.current_instructions
            .push(Instruction::LocalTee(ptr_local));
        self.current_instructions.push(Instruction::I32Eqz);
        self.current_instructions
            .push(Instruction::If(BlockType::Result(ValType::I32)));
        self.current_instructions.push(Instruction::I32Const(1));
        self.current_instructions.push(Instruction::Else);
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions
            .push(Instruction::I32Const(AnyTypeTag::Null.as_i32()));
        self.current_instructions.push(Instruction::I32Eq);
        self.current_instructions.push(Instruction::End);
    }
}

/// The load that reads a value of type `ty` from linear memory. Sized
/// integers occupy their own width (`integer:8` is one byte, see
/// `get_type_byte_size`), so they are read with the matching narrow load and
/// sign- or zero-extended; `integer:64` and `number` use 8-byte loads.
fn memory_load_instruction(ty: &MirType) -> Instruction<'static> {
    let arg = |align| wasm_encoder::MemArg {
        offset: 0,
        align,
        memory_index: 0,
    };
    match ty {
        MirType::F64 => Instruction::F64Load(arg(3)),
        MirType::F32 => Instruction::F32Load(arg(2)),
        MirType::I64 | MirType::U64 => Instruction::I64Load(arg(3)),
        MirType::I8 => Instruction::I32Load8S(arg(0)),
        MirType::U8 => Instruction::I32Load8U(arg(0)),
        MirType::I16 => Instruction::I32Load16S(arg(1)),
        MirType::U16 => Instruction::I32Load16U(arg(1)),
        _ => Instruction::I32Load(arg(2)),
    }
}

/// The store that writes a value of type `ty` to linear memory, the
/// counterpart of [`memory_load_instruction`]. A narrow integer is written
/// with a narrow store so it does not overwrite the bytes of the next field.
fn memory_store_instruction(ty: &MirType) -> Instruction<'static> {
    let arg = |align| wasm_encoder::MemArg {
        offset: 0,
        align,
        memory_index: 0,
    };
    match ty {
        MirType::F64 => Instruction::F64Store(arg(3)),
        MirType::F32 => Instruction::F32Store(arg(2)),
        MirType::I64 | MirType::U64 => Instruction::I64Store(arg(3)),
        MirType::I8 | MirType::U8 => Instruction::I32Store8(arg(0)),
        MirType::I16 | MirType::U16 => Instruction::I32Store16(arg(1)),
        _ => Instruction::I32Store(arg(2)),
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
    if let Some(canonical) = crate::error::ErrorUtils::legacy_builtin_alias(target) {
        if function_map.contains_key(canonical) {
            return crate::error::ErrorUtils::legacy_builtin_hint(target);
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
