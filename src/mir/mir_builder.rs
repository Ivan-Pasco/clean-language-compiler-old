//! MIR Builder - Lowering from TAST to MIR
//!
//! This module implements the transformation from TAST (Typed Abstract Syntax Tree)
//! to MIR (Medium-level Intermediate Representation). The builder converts high-level
//! typed constructs into optimization-friendly SSA form with explicit control flow.

#![allow(dead_code)]

use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::mir::mir_types::{
    BasicBlockId, MirBasicBlock, MirBinaryOp, MirConstant, MirFunction, MirFunctionAttributes,
    MirInstruction, MirLocal, MirOperand, MirOperation, MirParameter, MirProgram, MirTerminator,
    MirType, MirUnaryOp, ValueId,
};
use crate::resolver::SymbolId;
use crate::typechecker::tast::{
    BinaryOperator, ConcreteType, TastBlock, TastClass, TastExpression, TastExpressionKind,
    TastFunction, TastLiteral, TastProgram, TastStatement, UnaryOperator,
};
use std::collections::{HashMap, HashSet};

/// Result of MIR building process
#[derive(Debug)]
pub struct MirBuildResult {
    /// The generated MIR program
    pub program: MirProgram,

    /// Warnings generated during lowering
    pub warnings: Vec<CompilerError>,

    /// Statistics about the lowering process
    pub stats: MirBuildStats,
}

/// Statistics about the MIR building process
#[derive(Debug, Default)]
pub struct MirBuildStats {
    /// Number of functions lowered
    pub functions_lowered: usize,

    /// Number of basic blocks created
    pub basic_blocks_created: usize,

    /// Number of SSA values created
    pub ssa_values_created: usize,

    /// Number of phi nodes inserted
    pub phi_nodes_inserted: usize,

    /// Build time in microseconds
    pub build_time_us: u64,
}

/// MIR builder - converts TAST to MIR
#[derive(Debug)]
pub struct MirBuilder {
    /// Current function being built
    current_function: Option<MirFunction>,

    /// Current basic block being built
    current_block: Option<BasicBlockId>,

    /// SSA value counter
    next_value_id: usize,

    /// Basic block counter
    next_block_id: usize,

    /// Symbol ID to value ID mapping
    symbol_values: HashMap<SymbolId, ValueId>,

    /// Variable name to value ID mapping for current scope
    variable_values: HashMap<String, ValueId>,

    /// String literals pool
    string_pool: Vec<String>,

    /// String literal to pool index mapping
    string_indices: HashMap<String, usize>,

    /// Collected warnings
    warnings: Vec<CompilerError>,

    /// Build statistics
    stats: MirBuildStats,

    /// All classes in the program for inheritance lookups
    all_classes: Vec<TastClass>,
}

/// Context for building a single function
#[derive(Debug)]
struct FunctionBuildContext {
    /// Function being built
    function: MirFunction,

    /// Stack of scopes for variable resolution
    scope_stack: Vec<HashMap<String, ValueId>>,

    /// Pending phi nodes to resolve
    pending_phis: Vec<PendingPhi>,

    /// Loop context stack for break/continue
    loop_stack: Vec<LoopContext>,

    /// Class context for field access (if this is a class method)
    class_context: Option<TastClass>,

    /// All classes in the program for inheritance lookups
    all_classes: Vec<TastClass>,
}

/// Pending phi node that needs to be resolved
#[derive(Debug)]
struct PendingPhi {
    /// The phi instruction value ID
    value_id: ValueId,

    /// Variable name this phi represents
    variable_name: String,

    /// Basic block where phi is located
    block_id: BasicBlockId,

    /// Predecessor blocks and their values
    incoming: Vec<(BasicBlockId, Option<ValueId>)>,
}

/// Loop context for break/continue statements
#[derive(Debug)]
struct LoopContext {
    /// Basic block to jump to on continue
    continue_block: BasicBlockId,

    /// Basic block to jump to on break
    break_block: BasicBlockId,
}

impl MirBuilder {
    /// Create a new MIR builder
    pub fn new() -> Self {
        Self {
            current_function: None,
            current_block: None,
            next_value_id: 0,
            next_block_id: 0,
            symbol_values: HashMap::new(),
            variable_values: HashMap::new(),
            string_pool: Vec::new(),
            string_indices: HashMap::new(),
            warnings: Vec::new(),
            stats: MirBuildStats::default(),
            all_classes: Vec::new(),
        }
    }

    /// Build MIR from TAST program
    pub fn build(&mut self, tast: TastProgram) -> Result<MirBuildResult, Vec<CompilerError>> {
        let start_time = std::time::Instant::now();

        // Store all classes for inheritance lookups
        self.all_classes = tast.classes.clone();

        // Initialize program structure
        let mut mir_program = MirProgram {
            functions: HashMap::new(),
            globals: HashMap::new(),
            string_pool: Vec::new(),
            entry_point: None,
            debug_info: None,
        };

        // Lower all functions
        for function in tast.functions {
            match self.build_function(function) {
                Ok(mir_function) => {
                    if mir_function.attributes.entry_point {
                        mir_program.entry_point = Some(mir_function.symbol_id);
                    }
                    mir_program
                        .functions
                        .insert(mir_function.symbol_id, mir_function);
                    self.stats.functions_lowered += 1;
                }
                Err(errors) => {
                    self.warnings.extend(errors);
                }
            }
        }

        // CRITICAL FIX: Lower the start function if it exists
        if let Some(start_function) = tast.start_function {
            match self.build_function(start_function) {
                Ok(mir_function) => {
                    // Mark the start function as the entry point
                    mir_program.entry_point = Some(mir_function.symbol_id);
                    mir_program
                        .functions
                        .insert(mir_function.symbol_id, mir_function);
                    self.stats.functions_lowered += 1;
                }
                Err(errors) => {
                    self.warnings.extend(errors);
                }
            }
        }

        // Lower all classes (as constructor functions and method functions)
        for class in tast.classes {
            match self.build_class(class) {
                Ok(class_functions) => {
                    for function in class_functions {
                        mir_program.functions.insert(function.symbol_id, function);
                        self.stats.functions_lowered += 1;
                    }
                }
                Err(errors) => {
                    self.warnings.extend(errors);
                }
            }
        }

        // Transfer string pool
        mir_program.string_pool = self.string_pool.clone();

        // Record build time
        self.stats.build_time_us = start_time.elapsed().as_micros() as u64;

        // Return early if there are critical errors
        let critical_errors: Vec<_> = self
            .warnings
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    CompilerError::Type { .. } | CompilerError::Validation { .. }
                )
            })
            .cloned()
            .collect();

        if !critical_errors.is_empty() {
            return Err(critical_errors);
        }

        Ok(MirBuildResult {
            program: mir_program,
            warnings: std::mem::take(&mut self.warnings),
            stats: std::mem::take(&mut self.stats),
        })
    }

    /// Build MIR function from TAST function
    fn build_function(
        &mut self,
        tast_function: TastFunction,
    ) -> Result<MirFunction, Vec<CompilerError>> {
        self.build_function_with_class_context(tast_function, None)
    }

    /// Build MIR function from TAST function with optional class context for field access
    fn build_function_with_class_context(
        &mut self,
        tast_function: TastFunction,
        class_context: Option<&TastClass>,
    ) -> Result<MirFunction, Vec<CompilerError>> {
        // Create function structure
        let mir_function = MirFunction {
            symbol_id: tast_function.symbol_id,
            name: tast_function.name.clone(),
            parameters: Vec::new(),
            return_type: self.convert_concrete_type(&tast_function.return_type),
            blocks: HashMap::new(),
            entry_block: BasicBlockId(0),
            locals: HashMap::new(),
            next_value_id: 0,
            next_block_id: 1, // Block 0 is entry block
            attributes: MirFunctionAttributes {
                inline: false,
                pure: false,
                entry_point: tast_function.name == "start",
                exported: tast_function.name == "start",
            },
            location: tast_function.location.clone(),
        };

        // Create function build context
        let mut context = FunctionBuildContext {
            function: mir_function,
            scope_stack: vec![HashMap::new()],
            pending_phis: Vec::new(),
            loop_stack: Vec::new(),
            class_context: class_context.cloned(),
            all_classes: self.all_classes.clone(),
        };

        // Process parameters
        for (_i, param) in tast_function.parameters.iter().enumerate() {
            let value_id = ValueId(context.function.next_value_id);
            context.function.next_value_id += 1;

            let mir_param = MirParameter {
                value_id,
                name: param.name.clone(),
                param_type: self.convert_concrete_type(&param.param_type),
                location: param.location.clone(),
            };

            context.function.parameters.push(mir_param);

            // Add parameter to current scope
            if let Some(current_scope) = context.scope_stack.last_mut() {
                current_scope.insert(param.name.clone(), value_id);
            }
        }

        // Create entry basic block
        let entry_block = MirBasicBlock {
            id: BasicBlockId(0),
            label: Some("entry".to_string()),
            instructions: Vec::new(),
            terminator: MirTerminator::Unreachable, // Will be replaced
            predecessors: HashSet::new(),
            successors: HashSet::new(),
            location: tast_function.location.clone(),
        };

        context.function.blocks.insert(BasicBlockId(0), entry_block);
        self.current_block = Some(BasicBlockId(0));

        // Lower function body
        self.build_block(&mut context, &tast_function.body)?;

        // Ensure function has proper termination
        self.ensure_function_termination(&mut context, &tast_function.return_type)?;

        // Resolve pending phi nodes
        self.resolve_phi_nodes(&mut context)?;

        self.stats.basic_blocks_created += context.function.blocks.len();
        self.stats.ssa_values_created += context.function.next_value_id;
        self.stats.phi_nodes_inserted += context.pending_phis.len();

        Ok(context.function)
    }

    /// Build MIR class functions (constructor and methods)
    fn build_class(
        &mut self,
        tast_class: TastClass,
    ) -> Result<Vec<MirFunction>, Vec<CompilerError>> {
        let mut functions = Vec::new();
        let mut errors = Vec::new();

        // Clone class for passing to methods (needed to avoid borrow checker issues)
        let class_for_methods = tast_class.clone();

        // Build all constructors
        for constructor in tast_class.constructors {
            match self.build_function(constructor) {
                Ok(ctor_function) => functions.push(ctor_function),
                Err(ctor_errors) => errors.extend(ctor_errors),
            }
        }

        // Build all methods with class context
        for method in tast_class.methods {
            match self.build_function_with_class_context(method, Some(&class_for_methods)) {
                Ok(method_function) => functions.push(method_function),
                Err(method_errors) => errors.extend(method_errors),
            }
        }

        if errors.is_empty() {
            Ok(functions)
        } else {
            Err(errors)
        }
    }

    /// Build basic block from TAST block
    fn build_block(
        &mut self,
        context: &mut FunctionBuildContext,
        block: &TastBlock,
    ) -> Result<(), Vec<CompilerError>> {
        // Enter new scope
        context.scope_stack.push(HashMap::new());

        // Lower all statements
        for statement in &block.statements {
            self.build_statement(context, statement)?;
        }

        // Exit scope
        context.scope_stack.pop();

        Ok(())
    }

    /// Build statement
    fn build_statement(
        &mut self,
        context: &mut FunctionBuildContext,
        statement: &TastStatement,
    ) -> Result<(), Vec<CompilerError>> {
        match statement {
            TastStatement::VariableDeclaration {
                symbol_id: _,
                name,
                var_type,
                initializer,
                is_mutable: _,
                location,
            } => {
                let value_id = if let Some(init_expr) = initializer {
                    // Build initializer expression
                    self.build_expression(context, init_expr)?
                } else {
                    // Create uninitialized value
                    let value_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Add undefined constant instruction
                    let instruction = MirInstruction {
                        dest: Some(value_id),
                        operation: MirOperation::Copy {
                            source: MirOperand::Constant(MirConstant::Undefined),
                        },
                        location: location.clone(),
                    };

                    self.add_instruction(context, instruction);
                    value_id
                };

                // Add to current scope
                if let Some(current_scope) = context.scope_stack.last_mut() {
                    current_scope.insert(name.clone(), value_id);
                }

                // Create local variable entry
                let local = MirLocal {
                    name: Some(name.clone()),
                    local_type: self.convert_concrete_type(var_type),
                    is_mutable: true, // TODO: Track mutability from TAST
                    location: location.clone(),
                };

                context.function.locals.insert(value_id, local);
            }

            TastStatement::Assignment {
                target,
                value,
                location: _,
            } => {
                // Build value expression
                let value_id = self.build_expression(context, value)?;

                // Handle assignment target - for now, only support simple variable assignments
                match &target.kind {
                    TastExpressionKind::Variable { symbol_id: _, name } => {
                        // Update variable in current scope
                        if let Some(current_scope) = context.scope_stack.last_mut() {
                            current_scope.insert(name.clone(), value_id);
                        }
                    }
                    TastExpressionKind::PropertyAccess {
                        object,
                        property_name,
                        property_symbol: _,
                    } => {
                        // Handle field assignments like obj.field = value or this.field = value
                        let _object_value = self.build_expression(context, object)?;

                        // For now, treat field assignments as simple variable assignments
                        // In a class context, this.field = value becomes field = value
                        // TODO: Implement proper field assignment with object context

                        // Store the value for field assignment
                        if let Some(current_scope) = context.scope_stack.last_mut() {
                            current_scope.insert(property_name.clone(), value_id);
                        }
                    }
                    TastExpressionKind::ArrayAccess { array, index } => {
                        // Handle array index assignments like arr[i] = value
                        let _array_value = self.build_expression(context, array)?;
                        let _index_value = self.build_expression(context, index)?;

                        // TODO: Implement proper array element assignment
                        // For now, just ignore this assignment
                        tracing::warn!("Array index assignment not fully implemented");
                    }
                    _ => {
                        // Handle any other complex assignment targets
                        tracing::warn!(
                            target_kind = ?target.kind,
                            "Complex assignment target not implemented"
                        );
                        // Don't error out, just ignore the assignment for now
                    }
                }
            }

            TastStatement::Expression { expression, .. } => {
                // Build expression for side effects
                self.build_expression(context, expression)?;
            }

            TastStatement::Return {
                value,
                return_type: _,
                location: _,
            } => {
                // Return type validation already done in type checking phase
                let return_value = if let Some(expr) = value {
                    Some(MirOperand::Value(self.build_expression(context, expr)?))
                } else {
                    None
                };

                // Create return terminator
                let terminator = MirTerminator::Return {
                    value: return_value,
                };
                self.set_block_terminator(context, terminator);
            }

            TastStatement::Print {
                expression,
                newline,
                location,
            } => {
                // Build the expression to print
                let value_id = self.build_expression(context, expression)?;

                // AUTO-CONVERSION: Convert non-string types to string before printing
                use crate::typechecker::tast::ConcreteType;
                let string_value_id = match &expression.expr_type {
                    ConcreteType::String => {
                        // Already a string, use directly
                        value_id
                    }
                    ConcreteType::Integer => {
                        // Convert integer to string using int_to_string (SymbolId(5))
                        let conversion_instruction = MirInstruction {
                            dest: Some(ValueId(context.function.next_value_id)),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(SymbolId(5)), // int_to_string
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        let converted_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    ConcreteType::Number => {
                        // Convert float to string using float_to_string (SymbolId(6))
                        let conversion_instruction = MirInstruction {
                            dest: Some(ValueId(context.function.next_value_id)),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(SymbolId(6)), // float_to_string
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        let converted_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    ConcreteType::Boolean => {
                        // Convert boolean to string using bool_to_string (SymbolId(7))
                        let conversion_instruction = MirInstruction {
                            dest: Some(ValueId(context.function.next_value_id)),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(SymbolId(7)), // bool_to_string
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        let converted_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    _ => {
                        // For other types (objects, arrays, etc.), use the value as-is for now
                        // In a complete implementation, these would also have toString() methods
                        value_id
                    }
                };

                // CRITICAL FIX: Use correct function based on newline flag
                // SymbolId(0) = print (no newline), SymbolId(1) = printl (with newline)
                let function_symbol = if *newline {
                    SymbolId(1) // printl
                } else {
                    SymbolId(0) // print
                };

                let instruction = MirInstruction {
                    dest: None, // Print doesn't return a value
                    operation: MirOperation::Call {
                        function: MirOperand::Function(function_symbol),
                        arguments: vec![MirOperand::Value(string_value_id)],
                    },
                    location: location.clone(),
                };

                self.add_instruction(context, instruction);
            }

            TastStatement::If {
                condition,
                then_block,
                else_block,
                result_type: _,
                location,
            } => {
                // Build condition expression
                let condition_id = self.build_expression(context, condition)?;

                // Create basic blocks for then, else (if present), and continuation
                let then_block_id = BasicBlockId(context.function.blocks.len());
                let else_block_id = if else_block.is_some() {
                    Some(BasicBlockId(context.function.blocks.len() + 1))
                } else {
                    None
                };
                let continue_block_id = BasicBlockId(
                    context.function.blocks.len() + if else_block.is_some() { 2 } else { 1 },
                );

                // Create conditional branch in current block
                let branch = if let Some(else_id) = else_block_id {
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_id),
                        true_block: then_block_id,
                        false_block: else_id,
                    }
                } else {
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_id),
                        true_block: then_block_id,
                        false_block: continue_block_id,
                    }
                };
                self.set_block_terminator(context, branch);

                // Build then block
                context.function.blocks.insert(
                    then_block_id,
                    MirBasicBlock {
                        id: then_block_id,
                        label: Some("then".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable, // Will be replaced
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(then_block_id);

                // Process then block statements
                for stmt in &then_block.statements {
                    self.build_statement(context, stmt)?;
                }

                // Jump to continue block from then
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: continue_block_id,
                    },
                );

                // Build else block if present
                if let Some(else_stmt_block) = else_block {
                    let else_id = else_block_id.unwrap();
                    context.function.blocks.insert(
                        else_id,
                        MirBasicBlock {
                            id: else_id,
                            label: Some("else".to_string()),
                            instructions: Vec::new(),
                            terminator: MirTerminator::Unreachable, // Will be replaced
                            predecessors: HashSet::new(),
                            successors: HashSet::new(),
                            location: location.clone(),
                        },
                    );
                    self.current_block = Some(else_id);

                    // Process else block statements
                    for stmt in &else_stmt_block.statements {
                        self.build_statement(context, stmt)?;
                    }

                    // Jump to continue block from else
                    self.set_block_terminator(
                        context,
                        MirTerminator::Jump {
                            target: continue_block_id,
                        },
                    );
                }

                // Create continue block
                context.function.blocks.insert(
                    continue_block_id,
                    MirBasicBlock {
                        id: continue_block_id,
                        label: Some("continue".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable, // Will be replaced
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(continue_block_id);
            }

            TastStatement::For {
                iterator,
                iterable,
                body,
                location,
            } => {
                // Build the iterable expression (e.g., array, range)
                let iterable_value = self.build_expression(context, iterable)?;

                // Create loop blocks
                let header_block_id = BasicBlockId(context.function.blocks.len());
                let body_block_id = BasicBlockId(context.function.blocks.len() + 1);
                let exit_block_id = BasicBlockId(context.function.blocks.len() + 2);

                // Create iterator index variable (starts at 0)
                let index_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Initialize index to 0
                let init_instruction = MirInstruction {
                    dest: Some(index_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::Integer(0)),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, init_instruction);

                // Create iterator value variable for current element
                let iterator_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Add iterator variable to scope (this becomes the loop variable)
                if let Some(current_scope) = context.scope_stack.last_mut() {
                    // The iterator symbol should be resolvable to get its name
                    let iterator_name = format!("loop_var_{}", iterator.0); // Fallback name
                    current_scope.insert(iterator_name.clone(), iterator_value_id);
                }

                // Create local for iterator variable
                let iterator_local = MirLocal {
                    name: Some(format!("loop_var_{}", iterator.0)),
                    local_type: MirType::I32, // TODO: Infer from iterable element type
                    is_mutable: false,
                    location: location.clone(),
                };
                context
                    .function
                    .locals
                    .insert(iterator_value_id, iterator_local);

                // Jump to header block
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: header_block_id,
                    },
                );

                // Create header block (loop condition check)
                context.function.blocks.insert(
                    header_block_id,
                    MirBasicBlock {
                        id: header_block_id,
                        label: Some("for_header".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(header_block_id);

                // Get array length (for bounds checking)
                let length_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Array length instruction (simplified - assume array has length property)
                let length_instruction = MirInstruction {
                    dest: Some(length_value_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(SymbolId(999)), // Built-in array length
                        arguments: vec![MirOperand::Value(iterable_value)],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, length_instruction);

                // Compare index < length
                let condition_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                let compare_instruction = MirInstruction {
                    dest: Some(condition_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Lt,
                        left: MirOperand::Value(index_value_id),
                        right: MirOperand::Value(length_value_id),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, compare_instruction);

                // Conditional branch: if index < length goto body else goto exit
                self.set_block_terminator(
                    context,
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_value_id),
                        true_block: body_block_id,
                        false_block: exit_block_id,
                    },
                );

                // Create body block
                context.function.blocks.insert(
                    body_block_id,
                    MirBasicBlock {
                        id: body_block_id,
                        label: Some("for_body".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(body_block_id);

                // Load current array element: iterator_value = iterable[index]
                let element_instruction = MirInstruction {
                    dest: Some(iterator_value_id),
                    operation: MirOperation::GetElementPtr {
                        base: MirOperand::Value(iterable_value),
                        indices: vec![MirOperand::Value(index_value_id)],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, element_instruction);

                // Process loop body statements
                for stmt in &body.statements {
                    self.build_statement(context, stmt)?;
                }

                // Increment index: index = index + 1
                let incremented_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                let increment_instruction = MirInstruction {
                    dest: Some(incremented_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Add,
                        left: MirOperand::Value(index_value_id),
                        right: MirOperand::Constant(MirConstant::Integer(1)),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, increment_instruction);

                // Update index variable (this is a bit tricky in SSA form)
                // For simplicity, we'll create a new value ID for the incremented index
                // In a real SSA implementation, we'd use phi nodes
                // TODO: Implement proper SSA phi nodes for loop variables

                // Jump back to header
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: header_block_id,
                    },
                );

                // Create exit block
                context.function.blocks.insert(
                    exit_block_id,
                    MirBasicBlock {
                        id: exit_block_id,
                        label: Some("for_exit".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(exit_block_id);
            }

            TastStatement::While {
                condition,
                body,
                location,
            } => {
                // Create basic blocks for loop header, body, and exit
                let header_block_id = BasicBlockId(context.function.blocks.len());
                let body_block_id = BasicBlockId(context.function.blocks.len() + 1);
                let exit_block_id = BasicBlockId(context.function.blocks.len() + 2);

                // Jump to header from current block
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: header_block_id,
                    },
                );

                // Create header block - evaluates condition
                context.function.blocks.insert(
                    header_block_id,
                    MirBasicBlock {
                        id: header_block_id,
                        label: Some("while_header".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable, // Will be replaced
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(header_block_id);

                // Build condition expression in header block
                let condition_id = self.build_expression(context, condition)?;

                // Create conditional branch: if condition then body else exit
                self.set_block_terminator(
                    context,
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_id),
                        true_block: body_block_id,
                        false_block: exit_block_id,
                    },
                );

                // Create body block
                context.function.blocks.insert(
                    body_block_id,
                    MirBasicBlock {
                        id: body_block_id,
                        label: Some("while_body".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable, // Will be replaced
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(body_block_id);

                // Process body statements
                for stmt in &body.statements {
                    self.build_statement(context, stmt)?;
                }

                // Jump back to header at end of body (creating the loop)
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: header_block_id,
                    },
                );

                // Create exit block
                context.function.blocks.insert(
                    exit_block_id,
                    MirBasicBlock {
                        id: exit_block_id,
                        label: Some("while_exit".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable, // Will be replaced later
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(exit_block_id);
            }

            TastStatement::LaterAssignment {
                variable,
                symbol_id: _,
                expression,
                location,
            } => {
                // For async later assignments, we create a deferred execution context
                // The expression is evaluated asynchronously and stored for later use
                let value_id = self.build_expression(context, expression)?;

                // Create a value ID for the variable
                let variable_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Store the async result in a local variable
                let mir_type = self.convert_concrete_type(&expression.expr_type);
                self.register_temp_local(context, variable_value_id, mir_type, location.clone());

                // Create an async assignment instruction that defers the value
                let async_instruction = MirInstruction {
                    dest: Some(variable_value_id),
                    operation: MirOperation::AsyncAssign {
                        source: MirOperand::Value(value_id),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, async_instruction);

                // Add variable to current scope
                if let Some(current_scope) = context.scope_stack.last_mut() {
                    current_scope.insert(variable.clone(), variable_value_id);
                }
            }

            _ => {
                // TODO: Implement other statement types
                return Err(vec![CompilerError::validation_error(
                    &format!("Statement type not yet implemented: {:?}", statement),
                    SourceLocation::default(),
                )]);
            }
        }

        Ok(())
    }

    /// Build expression and return its value ID
    fn build_expression(
        &mut self,
        context: &mut FunctionBuildContext,
        expression: &TastExpression,
    ) -> Result<ValueId, Vec<CompilerError>> {
        match &expression.kind {
            TastExpressionKind::Literal { value } => {
                let constant = self.convert_literal(value);
                let value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the ValueId as a temporary local for codegen
                let mir_type = self.convert_literal_type(value);
                self.register_temp_local(context, value_id, mir_type, expression.location.clone());

                let instruction = MirInstruction {
                    dest: Some(value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(constant),
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(value_id)
            }

            TastExpressionKind::Variable { symbol_id: _, name } => {
                // Special case for 'this' - in class methods, 'this' refers to the first parameter
                if name == "this" && context.class_context.is_some() {
                    // In class methods, 'this' is the first parameter (parameter 0)
                    // Create a value ID that refers to the first parameter
                    let value_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // For now, create a copy instruction that loads 'this' from parameter 0
                    let instruction = MirInstruction {
                        dest: Some(value_id),
                        operation: MirOperation::Copy {
                            source: MirOperand::Value(ValueId(0)), // Parameter 0 is 'this'
                        },
                        location: expression.location.clone(),
                    };

                    self.add_instruction(context, instruction);
                    return Ok(value_id);
                }

                // Look up variable in scope stack
                for scope in context.scope_stack.iter().rev() {
                    if let Some(&value_id) = scope.get(name) {
                        return Ok(value_id);
                    }
                }

                // If not found in scope and we're in a class method, check class fields
                if let Some(ref class) = context.class_context {
                    // Check current class fields
                    if let Some(_field) = class.fields.iter().find(|f| f.name == *name) {
                        // This is an implicit field access - treat as self.field
                        // For now, we'll create a simple variable access to the field
                        // TODO: Implement proper self.field access with object context

                        // Create a temporary value for the field access
                        let value_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        return Ok(value_id);
                    }

                    // Check parent class fields if inheritance is involved
                    if let Some(parent_symbol_id) = class.parent_class {
                        // Look up parent class in all classes
                        if let Some(parent_class) = context
                            .all_classes
                            .iter()
                            .find(|c| c.symbol_id == parent_symbol_id)
                        {
                            // Check if field exists in parent class
                            if let Some(_parent_field) =
                                parent_class.fields.iter().find(|f| f.name == *name)
                            {
                                // Found field in parent class - treat as self.field access to inherited field
                                // Create a temporary value for the field access
                                let value_id = ValueId(context.function.next_value_id);
                                context.function.next_value_id += 1;

                                return Ok(value_id);
                            }

                            // Recursively check grandparent classes
                            let mut current_class = parent_class;
                            loop {
                                if let Some(grandparent_symbol_id) = current_class.parent_class {
                                    if let Some(grandparent_class) = context
                                        .all_classes
                                        .iter()
                                        .find(|c| c.symbol_id == grandparent_symbol_id)
                                    {
                                        if let Some(_grandparent_field) = grandparent_class
                                            .fields
                                            .iter()
                                            .find(|f| f.name == *name)
                                        {
                                            // Found field in ancestor class
                                            let value_id = ValueId(context.function.next_value_id);
                                            context.function.next_value_id += 1;
                                            return Ok(value_id);
                                        }
                                        current_class = grandparent_class;
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }

                Err(vec![CompilerError::type_error(
                    &format!("Undefined variable: {}", name),
                    None,
                    Some(expression.location.clone()),
                )])
            }

            TastExpressionKind::BinaryOperation {
                operator,
                left,
                right,
            } => {
                // CRITICAL FIX: Handle Power operator as runtime function call
                if matches!(operator, BinaryOperator::Power) {
                    // Power operation requires runtime pow function
                    // For integers: pow_i32(base, exponent) -> i32
                    // For floats: pow_f64(base, exponent) -> f64
                    let left_id = self.build_expression(context, left)?;
                    let right_id = self.build_expression(context, right)?;
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Determine result type based on operand types
                    let result_type = if matches!(left.expr_type, ConcreteType::Number)
                        || matches!(right.expr_type, ConcreteType::Number)
                    {
                        MirType::F64 // Float power
                    } else {
                        MirType::I32 // Integer power
                    };

                    self.register_temp_local(
                        context,
                        result_id,
                        result_type.clone(),
                        expression.location.clone(),
                    );

                    // Use SymbolId(1001) for pow_f64, SymbolId(1002) for pow_i32
                    let pow_function = if matches!(result_type, MirType::F64) {
                        SymbolId(1001) // pow_f64
                    } else {
                        SymbolId(1002) // pow_i32
                    };

                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::Function(pow_function),
                            arguments: vec![
                                MirOperand::Value(left_id),
                                MirOperand::Value(right_id),
                            ],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    return Ok(result_id);
                }

                // Check if this is string concatenation (String + String or String + other)
                let is_string_concat = matches!(operator, BinaryOperator::Add)
                    && (matches!(left.expr_type, ConcreteType::String)
                        || matches!(right.expr_type, ConcreteType::String));

                if is_string_concat {
                    // String concatenation uses runtime string_concat function
                    // string_concat(str1_ptr, str1_len, str2_ptr, str2_len) -> (result_ptr, result_len)
                    let left_id = self.build_expression(context, left)?;
                    let right_id = self.build_expression(context, right)?;
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Result is a StringTuple (ptr, len)
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::StringTuple,
                        expression.location.clone(),
                    );

                    // Generate call to string_concat runtime function
                    // Use SymbolId(1000) as a fixed ID for string_concat built-in
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::Function(SymbolId(1000)),
                            arguments: vec![
                                MirOperand::Value(left_id),
                                MirOperand::Value(right_id),
                            ],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, instruction);
                    Ok(result_id)
                } else {
                    // Regular binary operation (arithmetic, comparison, etc.)
                    let left_id = self.build_expression(context, left)?;
                    let right_id = self.build_expression(context, right)?;
                    let result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Register the result as a temporary local for codegen
                    let result_type =
                        self.infer_binary_operation_type(&left.expr_type, &right.expr_type);
                    self.register_temp_local(
                        context,
                        result_id,
                        result_type,
                        expression.location.clone(),
                    );

                    let mir_op = self.convert_binary_op(operator);
                    let instruction = MirInstruction {
                        dest: Some(result_id),
                        operation: MirOperation::BinaryOp {
                            op: mir_op,
                            left: MirOperand::Value(left_id),
                            right: MirOperand::Value(right_id),
                        },
                        location: expression.location.clone(),
                    };

                    self.add_instruction(context, instruction);
                    Ok(result_id)
                }
            }

            TastExpressionKind::UnaryOperation { operator, operand } => {
                // CRITICAL FIX: Handle unary Plus as a no-op (identity operation)
                if matches!(operator, UnaryOperator::Plus) {
                    // Unary plus is a no-op - just return the operand value unchanged
                    return self.build_expression(context, operand);
                }

                // CRITICAL FIX: Increment/Decrement operators need special desugaring
                // These operators should have been desugared by the TAST phase, but if they reach here,
                // we need to handle them properly with proper state mutation
                match operator {
                    UnaryOperator::PreIncrement
                    | UnaryOperator::PostIncrement
                    | UnaryOperator::PreDecrement
                    | UnaryOperator::PostDecrement => {
                        return Err(vec![CompilerError::validation_error(
                            &format!(
                                "Increment/decrement operators must be desugared before MIR lowering. \
                                 Operator {:?} cannot be directly lowered to MIR.",
                                operator
                            ),
                            expression.location.clone(),
                        )]);
                    }
                    _ => {}
                }

                let operand_id = self.build_expression(context, operand)?;
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the result as a temporary local for codegen
                let result_type = self.infer_unary_operation_type(&operand.expr_type);
                self.register_temp_local(
                    context,
                    result_id,
                    result_type,
                    expression.location.clone(),
                );

                let mir_op = self.convert_unary_op(operator);
                let instruction = MirInstruction {
                    dest: Some(result_id),
                    operation: MirOperation::UnaryOp {
                        op: mir_op,
                        operand: MirOperand::Value(operand_id),
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(result_id)
            }

            TastExpressionKind::FunctionCall {
                function,
                arguments,
                type_args: _,
            } => {
                // Extract function symbol ID from the function expression
                let function_symbol_id = match &function.kind {
                    TastExpressionKind::Variable { symbol_id, .. } => *symbol_id,
                    _ => {
                        return Err(vec![CompilerError::validation_error(
                            "Function calls to non-simple function names not yet supported",
                            function.location.clone(),
                        )])
                    }
                };

                // Build argument operands
                let mut mir_arguments = Vec::new();
                for arg in arguments {
                    let arg_id = self.build_expression(context, arg)?;
                    mir_arguments.push(MirOperand::Value(arg_id));
                }

                // Create result value for function return
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // CRITICAL FIX: Register the result ValueId as a temporary local
                // This ensures constructor calls and function calls have a local to store the result
                let result_type = self.convert_concrete_type(&expression.expr_type);
                self.register_temp_local(
                    context,
                    result_id,
                    result_type,
                    expression.location.clone(),
                );

                let instruction = MirInstruction {
                    dest: Some(result_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(function_symbol_id),
                        arguments: mir_arguments,
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(result_id)
            }

            TastExpressionKind::MethodCall {
                receiver,
                method_name,
                method_symbol,
                arguments,
                type_args: _,
            } => {
                // Build the receiver (object) first
                let receiver_id = self.build_expression(context, receiver)?;

                // Check if this is a built-in method that should be mapped to a conversion function
                // SymbolId(0) is used as a placeholder for built-in methods that don't have real symbols
                let (function_symbol, mir_arguments) = if method_symbol.0 == 0 {
                    // This is a built-in method - determine the correct function based on receiver type and method name
                    let receiver_type = &receiver.expr_type;
                    match (receiver_type, method_name.as_str()) {
                        (ConcreteType::Integer, "toString") => {
                            // Call int_to_string (SymbolId 5) with the integer value
                            (SymbolId(5), vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Number, "toString") => {
                            // Call float_to_string (SymbolId 6) with the float value
                            (SymbolId(6), vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Boolean, "toString") => {
                            // Call bool_to_string (SymbolId 7) with the boolean value
                            (SymbolId(7), vec![MirOperand::Value(receiver_id)])
                        }
                        // For other built-in methods, fall back to treating as instance method
                        _ => {
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (*method_symbol, args)
                        }
                    }
                } else {
                    // This is a user-defined method - receiver becomes first argument
                    let mut args = vec![MirOperand::Value(receiver_id)];
                    for arg in arguments {
                        let arg_id = self.build_expression(context, arg)?;
                        args.push(MirOperand::Value(arg_id));
                    }
                    (*method_symbol, args)
                };

                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // CRITICAL FIX: Register the result ValueId as a temporary local
                let result_type = self.convert_concrete_type(&expression.expr_type);
                self.register_temp_local(
                    context,
                    result_id,
                    result_type,
                    expression.location.clone(),
                );

                let instruction = MirInstruction {
                    dest: Some(result_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(function_symbol),
                        arguments: mir_arguments,
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(result_id)
            }

            TastExpressionKind::PropertyAccess {
                object,
                property_name: _,
                property_symbol,
            } => {
                // Build the object expression first
                let object_id = self.build_expression(context, object)?;

                // Use GetElementPtr for field access - this will be handled by codegen
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Create a field access using GetElementPtr with field index
                // For simplicity, use symbol ID as field index
                let field_index =
                    MirOperand::Constant(MirConstant::Integer(property_symbol.0 as i64));
                let instruction = MirInstruction {
                    dest: Some(result_id),
                    operation: MirOperation::GetElementPtr {
                        base: MirOperand::Value(object_id),
                        indices: vec![field_index],
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);

                // Now load the value from the field pointer
                let load_result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                let load_instruction = MirInstruction {
                    dest: Some(load_result_id),
                    operation: MirOperation::Load {
                        source: MirOperand::Value(result_id),
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, load_instruction);
                Ok(load_result_id)
            }

            TastExpressionKind::ArrayAccess { array, index } => {
                // Build array and index expressions
                let array_id = self.build_expression(context, array)?;
                let index_id = self.build_expression(context, index)?;

                // Use GetElementPtr for array access
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                let instruction = MirInstruction {
                    dest: Some(result_id),
                    operation: MirOperation::GetElementPtr {
                        base: MirOperand::Value(array_id),
                        indices: vec![MirOperand::Value(index_id)],
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);

                // Load the value from the array element pointer
                let load_result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                let load_instruction = MirInstruction {
                    dest: Some(load_result_id),
                    operation: MirOperation::Load {
                        source: MirOperand::Value(result_id),
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, load_instruction);
                Ok(load_result_id)
            }

            TastExpressionKind::OnError {
                expression: expr,
                fallback: _,
            } => {
                // For now, just evaluate the expression
                // TODO: Implement proper error handling with fallback
                // This requires runtime error handling support
                self.build_expression(context, expr)
            }

            TastExpressionKind::Conditional {
                condition: _,
                then_expr,
                else_expr: _,
            } => {
                // TODO: Implement proper conditional with branching
                // For now, just evaluate the then branch
                // This requires proper control flow support
                self.build_expression(context, then_expr)
            }

            _ => {
                // TODO: Implement other expression types
                Err(vec![CompilerError::validation_error(
                    &format!("Expression type not yet implemented: {:?}", expression.kind),
                    expression.location.clone(),
                )])
            }
        }
    }

    /// Convert ConcreteType to MirType
    fn convert_concrete_type(&self, concrete_type: &ConcreteType) -> MirType {
        MirType::from_concrete_type(concrete_type)
    }

    /// Convert TAST literal to MIR constant
    fn convert_literal(&mut self, literal: &TastLiteral) -> MirConstant {
        match literal {
            TastLiteral::Integer(i) => MirConstant::Integer(*i),
            TastLiteral::Number(f) => MirConstant::Float(*f),
            TastLiteral::String(s) => {
                let index = self.get_string_index(s.clone());
                MirConstant::String(index)
            }
            TastLiteral::Boolean(b) => MirConstant::Boolean(*b),
            TastLiteral::Null => MirConstant::Null,
            TastLiteral::Undefined => MirConstant::Undefined,
        }
    }

    /// Convert TAST literal to its corresponding MIR type
    fn convert_literal_type(&self, literal: &TastLiteral) -> MirType {
        match literal {
            TastLiteral::Integer(_) => MirType::I32, // Default integer type
            TastLiteral::Number(_) => MirType::F64,  // Default float type
            TastLiteral::String(_) => MirType::Ptr(Box::new(MirType::I8)), // String as i8 pointer
            TastLiteral::Boolean(_) => MirType::Bool,
            TastLiteral::Null => MirType::Ptr(Box::new(MirType::Void)),
            TastLiteral::Undefined => MirType::Void,
        }
    }

    /// Register a ValueId as a temporary local for codegen
    fn register_temp_local(
        &self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        mir_type: MirType,
        location: SourceLocation,
    ) {
        let local = MirLocal {
            name: None, // Temporary values don't need names
            local_type: mir_type,
            is_mutable: false, // Temporary results are immutable
            location,
        };
        context.function.locals.insert(value_id, local);
    }

    /// Infer the result type of a binary operation
    fn infer_binary_operation_type(
        &self,
        left_type: &ConcreteType,
        right_type: &ConcreteType,
    ) -> MirType {
        match (left_type, right_type) {
            // Arithmetic operations between numeric types
            (ConcreteType::Integer, ConcreteType::Integer) => MirType::I32,
            (ConcreteType::Number, ConcreteType::Number) => MirType::F64,
            (ConcreteType::Number, ConcreteType::Integer) => MirType::F64,
            (ConcreteType::Integer, ConcreteType::Number) => MirType::F64,

            // Boolean operations
            (ConcreteType::Boolean, ConcreteType::Boolean) => MirType::Bool,

            // String operations (concatenation) - result is string
            (ConcreteType::String, ConcreteType::String) => MirType::StringTuple,
            (ConcreteType::String, _) => MirType::StringTuple, // String + any = String
            (_, ConcreteType::String) => MirType::StringTuple, // Any + String = String

            // Array operations (if supported) - result is array pointer
            (ConcreteType::Array(elem_type), ConcreteType::Array(_)) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Comparison operations always produce boolean
            // This is a simplified heuristic - ideally we'd know the operator type here
            // For now, if both operands are the same type and not numeric/bool, assume comparison
            (left, right) if left == right => {
                // Same type operations - use the type's MIR representation
                MirType::from_concrete_type(left)
            }

            // Mixed types or unknown - use left operand type as fallback
            // This handles cases like Class operations, Function operations, etc.
            (left, _) => MirType::from_concrete_type(left),
        }
    }

    /// Infer the result type of a unary operation
    fn infer_unary_operation_type(&self, operand_type: &ConcreteType) -> MirType {
        match operand_type {
            // Numeric operations preserve type
            ConcreteType::Integer => MirType::I32,
            ConcreteType::Number => MirType::F64,

            // Boolean operations preserve type
            ConcreteType::Boolean => MirType::Bool,

            // String operations preserve type
            ConcreteType::String => MirType::StringTuple,

            // Array operations preserve pointer type
            ConcreteType::Array(elem_type) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Matrix operations preserve pointer type
            ConcreteType::Matrix(elem_type) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Function, Class, and other complex types - use from_concrete_type
            // This handles all remaining ConcreteType variants properly
            other => MirType::from_concrete_type(other),
        }
    }

    /// Convert TAST binary operator to MIR binary operator
    ///
    /// Note: Some operators cannot be directly represented in MIR and should be
    /// handled specially in build_expression (Power, Concatenate).
    fn convert_binary_op(&self, op: &BinaryOperator) -> MirBinaryOp {
        match op {
            // Arithmetic operators
            BinaryOperator::Add => MirBinaryOp::Add,
            BinaryOperator::Subtract => MirBinaryOp::Sub,
            BinaryOperator::Multiply => MirBinaryOp::Mul,
            BinaryOperator::Divide => MirBinaryOp::Div,
            BinaryOperator::Modulo => MirBinaryOp::Rem,

            // Comparison operators
            BinaryOperator::Equal => MirBinaryOp::Eq,
            BinaryOperator::NotEqual => MirBinaryOp::Ne,
            BinaryOperator::LessThan => MirBinaryOp::Lt,
            BinaryOperator::GreaterThan => MirBinaryOp::Gt,
            BinaryOperator::LessThanOrEqual => MirBinaryOp::Le,
            BinaryOperator::GreaterThanOrEqual => MirBinaryOp::Ge,

            // Logical operators (And/Or work on booleans, lowered to i32.and/i32.or in WASM)
            // The type system ensures these are used on boolean types
            BinaryOperator::And => MirBinaryOp::And,
            BinaryOperator::Or => MirBinaryOp::Or,

            // Bitwise operators (And/Or work on integers, lowered to i32.and/i32.or in WASM)
            // The type system ensures these are used on integer types
            BinaryOperator::BitwiseAnd => MirBinaryOp::And,
            BinaryOperator::BitwiseOr => MirBinaryOp::Or,
            BinaryOperator::BitwiseXor => MirBinaryOp::Xor,
            BinaryOperator::LeftShift => MirBinaryOp::Shl,
            BinaryOperator::RightShift => MirBinaryOp::Shr,

            // CRITICAL: These operators should NEVER reach here - they must be handled in build_expression
            BinaryOperator::Power => {
                panic!("BUG: Power operator should be handled in build_expression as runtime function call, not converted to MIR operator")
            }
            BinaryOperator::Concatenate => {
                panic!("BUG: String concatenation should be handled in build_expression as string_concat call, not converted to MIR operator")
            }
        }
    }

    /// Convert TAST unary operator to MIR unary operator
    ///
    /// Note: Some unary operators (Plus, Increment, Decrement) cannot be directly
    /// represented in MIR and should be handled specially in build_expression.
    fn convert_unary_op(&self, op: &UnaryOperator) -> MirUnaryOp {
        match op {
            // Direct unary operators
            UnaryOperator::Negate => MirUnaryOp::Neg,
            UnaryOperator::Not => MirUnaryOp::Not,
            UnaryOperator::BitwiseNot => MirUnaryOp::BitNot,

            // CRITICAL: These operators should NEVER reach here - they must be handled in build_expression
            UnaryOperator::Plus => {
                panic!("BUG: Unary plus should be handled in build_expression as no-op, not converted to MIR operator")
            }
            UnaryOperator::PreIncrement => {
                panic!("BUG: Pre-increment should be desugared to assignment (x = x + 1) in build_expression, not converted to MIR operator")
            }
            UnaryOperator::PostIncrement => {
                panic!("BUG: Post-increment should be desugared to assignment (temp = x; x = x + 1; temp) in build_expression, not converted to MIR operator")
            }
            UnaryOperator::PreDecrement => {
                panic!("BUG: Pre-decrement should be desugared to assignment (x = x - 1) in build_expression, not converted to MIR operator")
            }
            UnaryOperator::PostDecrement => {
                panic!("BUG: Post-decrement should be desugared to assignment (temp = x; x = x - 1; temp) in build_expression, not converted to MIR operator")
            }
        }
    }

    /// Get or create string pool index
    fn get_string_index(&mut self, string: String) -> usize {
        if let Some(&index) = self.string_indices.get(&string) {
            index
        } else {
            let index = self.string_pool.len();
            self.string_pool.push(string.clone());
            self.string_indices.insert(string, index);
            index
        }
    }

    /// Add instruction to current basic block
    fn add_instruction(&mut self, context: &mut FunctionBuildContext, instruction: MirInstruction) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                block.instructions.push(instruction);
            }
        }
    }

    /// Set terminator for current basic block
    fn set_block_terminator(
        &mut self,
        context: &mut FunctionBuildContext,
        terminator: MirTerminator,
    ) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                block.terminator = terminator;
            }
        }
    }

    /// Ensure function has proper termination
    fn ensure_function_termination(
        &mut self,
        context: &mut FunctionBuildContext,
        return_type: &ConcreteType,
    ) -> Result<(), Vec<CompilerError>> {
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                // Check if block already has a terminator other than Unreachable
                if matches!(block.terminator, MirTerminator::Unreachable) {
                    // Add implicit return
                    let return_value = if matches!(return_type, ConcreteType::Undefined) {
                        None
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
    fn resolve_phi_nodes(
        &mut self,
        context: &mut FunctionBuildContext,
    ) -> Result<(), Vec<CompilerError>> {
        // TODO: Implement phi node resolution for SSA form
        // This is complex and requires control flow analysis

        if !context.pending_phis.is_empty() {
            self.warnings.push(CompilerError::validation_error(
                "Phi node resolution not yet implemented",
                SourceLocation::default(),
            ));
        }

        Ok(())
    }
}

impl Default for MirBuilder {
    fn default() -> Self {
        Self::new()
    }
}
