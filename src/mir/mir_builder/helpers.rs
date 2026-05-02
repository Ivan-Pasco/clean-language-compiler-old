//! Helper utilities — instruction emission, block management, field/class helpers.

use super::*;

impl MirBuilder {
    /// Add instruction to current basic block
    pub(super) fn add_instruction(
        &mut self,
        context: &mut FunctionBuildContext,
        instruction: MirInstruction,
    ) {
        if matches!(instruction.operation, MirOperation::GetElementPtr { .. }) {
            trace!(current_block = ?self.current_block, "Adding GetElementPtr");
        }
        if matches!(instruction.operation, MirOperation::Load { .. })
            && self.current_block == Some(BasicBlockId(2))
        {
            trace!(current_block = ?self.current_block, "Adding Load to body block");
        }

        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                block.instructions.push(instruction);
            }
        }
    }

    /// Set terminator for current basic block
    pub(super) fn set_block_terminator(
        &mut self,
        context: &mut FunctionBuildContext,
        terminator: MirTerminator,
    ) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                if block_id == BasicBlockId(3) && context.function.name == "test" {
                    trace!(
                        old_terminator = ?block.terminator,
                        new_terminator = ?terminator,
                        "Setting terminator for BasicBlockId(3)"
                    );
                }
                block.terminator = terminator;
            }
        }
    }

    /// Inject state rules checking at the end of operation boundary functions (start, frame)
    /// Each rule must evaluate to true; if any is false, execution traps
    pub(super) fn inject_rules_checking(
        &mut self,
        context: &mut FunctionBuildContext,
    ) -> Result<(), Vec<CompilerError>> {
        // Clone rules to avoid borrow issues
        let rules = self.state_rules.clone();

        for (rule_idx, rule_expr) in rules.iter().enumerate() {
            trace!(rule_index = rule_idx, "Building state rule check");

            // Build the rule expression - evaluate it to get a boolean
            let condition_value = self.build_expression(context, rule_expr)?;

            // Reserve block IDs for trap and continue blocks
            let base_block_id = context.function.next_block_id;
            let trap_block_id = BasicBlockId(base_block_id);
            let continue_block_id = BasicBlockId(base_block_id + 1);
            context.function.next_block_id = base_block_id + 2;

            // Trap block: executes unreachable if rule is false
            context.function.blocks.insert(
                trap_block_id,
                MirBasicBlock {
                    id: trap_block_id,
                    label: Some(format!("rule_{}_trap", rule_idx)),
                    instructions: Vec::new(),
                    terminator: MirTerminator::Trap, // WASM unreachable trap for contract violation
                    predecessors: HashSet::new(),
                    successors: HashSet::new(),
                    location: rule_expr.location.clone(),
                },
            );

            // Continue block: normal execution if rule is true
            context.function.blocks.insert(
                continue_block_id,
                MirBasicBlock {
                    id: continue_block_id,
                    label: Some(format!("rule_{}_continue", rule_idx)),
                    instructions: Vec::new(),
                    terminator: MirTerminator::Unreachable, // Will be replaced
                    predecessors: HashSet::new(),
                    successors: HashSet::new(),
                    location: rule_expr.location.clone(),
                },
            );

            // Set terminator: branch based on condition
            // If condition is true, continue; if false, trap
            self.set_block_terminator(
                context,
                MirTerminator::Branch {
                    condition: MirOperand::Value(condition_value),
                    true_block: continue_block_id, // Condition true -> continue
                    false_block: trap_block_id,    // Condition false -> trap
                },
            );

            // Switch to continue block for next rule or function termination
            self.current_block = Some(continue_block_id);
        }

        trace!(rule_count = rules.len(), "State rules checking injected");

        Ok(())
    }

    /// Ensure function has proper termination
    pub(super) fn ensure_function_termination(
        &mut self,
        context: &mut FunctionBuildContext,
        return_type: &ConcreteType,
    ) -> Result<(), Vec<CompilerError>> {
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                // Check if block already has a terminator other than Unreachable
                if matches!(block.terminator, MirTerminator::Unreachable) {
                    // If this block is the current_block at function termination time,
                    // it means it's reachable (execution reaches here), so add implicit return.
                    // Add implicit return
                    let return_value = if matches!(return_type, ConcreteType::Undefined) {
                        None
                    } else if context.class_context.is_some()
                        && matches!(return_type, ConcreteType::Class { .. })
                    {
                        // Constructor: return 'this' (first parameter - instance pointer)
                        if let Some(first_param) = context.function.parameters.first() {
                            Some(MirOperand::Value(first_param.value_id))
                        } else {
                            // Fallback to undefined if no parameters (shouldn't happen)
                            Some(MirOperand::Constant(MirConstant::Undefined))
                        }
                    } else {
                        // Return undefined for non-void functions without explicit return
                        Some(MirOperand::Constant(MirConstant::Undefined))
                    };

                    block.terminator = MirTerminator::Return {
                        value: return_value,
                    };
                }
            }
        }

        Ok(())
    }

    /// Resolve pending phi nodes
    pub(super) fn resolve_phi_nodes(
        &mut self,
        _context: &mut FunctionBuildContext,
    ) -> Result<(), Vec<CompilerError>> {
        Ok(())
    }

    /// Count total number of fields in a class including all inherited fields
    ///
    /// This traverses the class hierarchy from the root (most distant ancestor) to the leaf
    /// and sums up all field counts.
    pub(super) fn count_all_fields_in_hierarchy(
        &self,
        context: &FunctionBuildContext,
        class_symbol: SymbolId,
    ) -> usize {
        // Find the starting class
        let mut current_class_opt = context
            .all_classes
            .iter()
            .find(|c| c.symbol_id == class_symbol);

        if current_class_opt.is_none() {
            return 0;
        }

        // Collect all classes in the hierarchy from current to root
        let mut hierarchy = Vec::new();

        while let Some(current_class) = current_class_opt {
            hierarchy.push(current_class.clone());

            // Move to parent
            if let Some(ref parent_symbol) = current_class.parent_class {
                current_class_opt = context
                    .all_classes
                    .iter()
                    .find(|c| c.symbol_id == *parent_symbol);
            } else {
                break;
            }
        }

        // Sum up all field counts from all classes in the hierarchy
        let total_fields: usize = hierarchy.iter().map(|c| c.fields.len()).sum();
        tracing::debug!(
            "count_all_fields_in_hierarchy: class {:?} has {} classes in hierarchy, total {} fields",
            class_symbol,
            hierarchy.len(),
            total_fields
        );
        total_fields
    }

    /// Calculate the total byte size of all fields in a class hierarchy
    ///
    /// This traverses the class hierarchy and sums up the byte sizes of all fields,
    /// accounting for different type sizes (i32=4, i64/f64=8, etc.)
    pub(super) fn calculate_instance_byte_size(
        &self,
        context: &FunctionBuildContext,
        class_symbol: SymbolId,
    ) -> usize {
        // Find the starting class
        let mut current_class_opt = context
            .all_classes
            .iter()
            .find(|c| c.symbol_id == class_symbol);

        if current_class_opt.is_none() {
            return 0;
        }

        // Collect all classes in the hierarchy from current to root
        let mut hierarchy = Vec::new();

        while let Some(current_class) = current_class_opt {
            hierarchy.push(current_class.clone());

            // Move to parent
            if let Some(ref parent_symbol) = current_class.parent_class {
                current_class_opt = context
                    .all_classes
                    .iter()
                    .find(|c| c.symbol_id == *parent_symbol);
            } else {
                break;
            }
        }

        // Reverse to get root-to-leaf order
        hierarchy.reverse();

        // Sum up byte sizes for all fields
        let mut total_bytes = 0usize;
        for class in &hierarchy {
            for field in &class.fields {
                total_bytes += self.get_type_byte_size(&field.field_type);
            }
        }

        tracing::debug!(
            "calculate_instance_byte_size: class {:?} needs {} bytes",
            class_symbol,
            total_bytes
        );
        total_bytes
    }

    /// Get the byte size of a ConcreteType
    pub(super) fn get_type_byte_size(&self, concrete_type: &ConcreteType) -> usize {
        match concrete_type {
            ConcreteType::Integer => 4,
            ConcreteType::Number => 8, // f64
            ConcreteType::IntegerSized { bits: 8, .. } => 1,
            ConcreteType::IntegerSized { bits: 16, .. } => 2,
            ConcreteType::IntegerSized { bits: 32, .. } => 4,
            ConcreteType::IntegerSized { bits: 64, .. } => 8,
            ConcreteType::IntegerSized { .. } => 4, // fallback
            ConcreteType::NumberSized { bits: 32 } => 4,
            ConcreteType::NumberSized { bits: 64 } => 8,
            ConcreteType::NumberSized { .. } => 8, // fallback
            ConcreteType::Boolean => 4,            // Stored as i32
            ConcreteType::String => 4,             // Pointer (i32)
            ConcreteType::Null => 4,               // Stored as i32
            ConcreteType::Undefined => 4,          // Stored as i32
            ConcreteType::Array(_) => 4,           // Pointer
            ConcreteType::Matrix(_) => 4,          // Pointer
            ConcreteType::Pairs(_, _) => 4,        // Pointer
            ConcreteType::Function { .. } => 4,    // Pointer
            ConcreteType::Class { .. } => 4,       // Pointer
            ConcreteType::Interface { .. } => 4,   // Pointer
            ConcreteType::Tuple(_) => 4,           // Pointer
            ConcreteType::Union(_) => 4,           // Pointer (boxed)
            ConcreteType::Intersection(_) => 4,    // Pointer (boxed)
            ConcreteType::Generic { .. } => 4,     // Treat as pointer
            ConcreteType::Unknown => 4,            // Default
            ConcreteType::Never => 0,              // Never returns
            ConcreteType::Namespace => 4,          // Not really used in memory
            ConcreteType::Any => 12,               // Boxed: [tag:i32][value1:i32][value2:i32]
            // Optional<T> has the same runtime representation as T (none = 0/null)
            ConcreteType::Optional(inner) => self.get_type_byte_size(inner),
        }
    }

    /// Calculate the byte offset of a field within a class hierarchy
    ///
    /// This returns the byte offset from the start of the object to the field,
    /// accounting for different field sizes.
    pub(super) fn calculate_field_byte_offset(
        &self,
        context: &FunctionBuildContext,
        class_symbol: SymbolId,
        property_symbol: &SymbolId,
    ) -> Option<usize> {
        // Find the starting class
        let mut current_class_opt = context
            .all_classes
            .iter()
            .find(|c| c.symbol_id == class_symbol);

        if current_class_opt.is_none() {
            return None;
        }

        // Collect all classes in the hierarchy from current to root
        let mut hierarchy = Vec::new();

        while let Some(current_class) = current_class_opt {
            hierarchy.push(current_class.clone());

            // Move to parent
            if let Some(ref parent_symbol) = current_class.parent_class {
                current_class_opt = context
                    .all_classes
                    .iter()
                    .find(|c| c.symbol_id == *parent_symbol);
            } else {
                break;
            }
        }

        // Reverse to get root-to-leaf order
        hierarchy.reverse();

        // Calculate byte offset
        let mut byte_offset = 0usize;

        for class in &hierarchy {
            for field in &class.fields {
                if field.symbol_id == *property_symbol {
                    tracing::debug!(
                        "calculate_field_byte_offset: field {:?} at byte offset {}",
                        property_symbol,
                        byte_offset
                    );
                    return Some(byte_offset);
                }
                byte_offset += self.get_type_byte_size(&field.field_type);
            }
        }

        tracing::debug!(
            "calculate_field_byte_offset: field {:?} NOT FOUND",
            property_symbol
        );
        None
    }
}
