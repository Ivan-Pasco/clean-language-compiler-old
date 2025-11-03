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

    /// Symbol table for looking up constructors, methods, etc.
    symbol_table: std::sync::Arc<crate::resolver::GlobalSymbolTable>,

    /// Collected warnings
    warnings: Vec<CompilerError>,

    /// Build statistics
    stats: MirBuildStats,

    /// All classes in the program for inheritance lookups
    all_classes: Vec<TastClass>,

    /// All functions in the program for default parameter lookups
    all_functions: Vec<TastFunction>,
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

    /// All functions in the program for default parameter lookups
    all_functions: Vec<TastFunction>,
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
    pub fn new(symbol_table: std::sync::Arc<crate::resolver::GlobalSymbolTable>) -> Self {
        Self {
            current_function: None,
            current_block: None,
            next_value_id: 0,
            next_block_id: 0,
            symbol_values: HashMap::new(),
            variable_values: HashMap::new(),
            string_pool: Vec::new(),
            string_indices: HashMap::new(),
            symbol_table,
            warnings: Vec::new(),
            stats: MirBuildStats::default(),
            all_classes: Vec::new(),
            all_functions: Vec::new(),
        }
    }

    /// Build MIR from TAST program
    pub fn build(&mut self, tast: TastProgram) -> Result<MirBuildResult, Vec<CompilerError>> {
        let start_time = std::time::Instant::now();

        // Store all classes for inheritance lookups
        self.all_classes = tast.classes.clone();

        // Store all functions for default parameter lookups
        self.all_functions = tast.functions.clone();

        // Initialize program structure
        let mut mir_program = MirProgram {
            functions: HashMap::new(),
            globals: HashMap::new(),
            string_pool: Vec::new(),
            entry_point: None,
            debug_info: None,
            symbol_name_map: HashMap::new(),
        };

        // CRITICAL FIX: Populate symbol_name_map from SymbolTable for dynamic resolution
        // This captures ALL symbols: builtins (print, math.*, string.*, etc.) AND user-defined
        eprintln!(
            "DEBUG MIR SYMBOLS: Populating symbol_name_map from SymbolTable with {} symbols",
            tast.symbol_table.all_symbols().len()
        );
        for (symbol_id, symbol) in tast.symbol_table.all_symbols() {
            // CRITICAL FIX: For Method symbols, construct fully qualified name (e.g., "math.min")
            let full_name =
                if let crate::resolver::SymbolKind::Method { class_id, .. } = &symbol.kind {
                    // Get the class name for this method
                    if let Some(class_symbol) = tast.symbol_table.all_symbols().get(class_id) {
                        format!("{}.{}", class_symbol.name.to_lowercase(), symbol.name)
                    } else {
                        symbol.name.clone()
                    }
                } else {
                    symbol.name.clone()
                };

            mir_program
                .symbol_name_map
                .insert(*symbol_id, full_name.clone());
            eprintln!(
                "DEBUG MIR SYMBOLS: SymbolId({}) -> '{}'",
                symbol_id.0, full_name
            );
        }
        eprintln!(
            "DEBUG MIR SYMBOLS: symbol_name_map populated with {} entries",
            mir_program.symbol_name_map.len()
        );

        // CRITICAL FIX: Add synthetic SymbolIds for MIR-generated built-in functions
        // These are created during MIR building for string concatenation and power operations
        // Map them to existing WASM builtin functions
        mir_program
            .symbol_name_map
            .insert(SymbolId(1000), "string_concat".to_string());
        // pow_f64 and pow_i32 should map to the existing math.pow function
        mir_program
            .symbol_name_map
            .insert(SymbolId(1001), "math.pow".to_string());
        mir_program
            .symbol_name_map
            .insert(SymbolId(1002), "math.pow".to_string());

        // CRITICAL FIX: Add common name variations for builtin functions
        // Check symbol_name_map for variations and add correct WASM names
        let mut fixes_needed = Vec::new();
        for (symbol_id, name) in mir_program.symbol_name_map.iter() {
            match name.as_str() {
                "println" => fixes_needed.push((*symbol_id, "printl".to_string())),
                _ => {}
            }
        }
        for (symbol_id, corrected_name) in fixes_needed {
            eprintln!(
                "DEBUG MIR SYMBOLS: Correcting SymbolId({}) from '{}' to '{}'",
                symbol_id.0,
                mir_program.symbol_name_map.get(&symbol_id).unwrap(),
                corrected_name
            );
            mir_program
                .symbol_name_map
                .insert(symbol_id, corrected_name);
        }

        eprintln!(
            "DEBUG MIR SYMBOLS: Added 3 synthetic SymbolIds (1000-1002) for MIR-generated builtins"
        );
        eprintln!(
            "DEBUG MIR SYMBOLS: Final symbol_name_map has {} entries",
            mir_program.symbol_name_map.len()
        );

        // Lower all functions
        for function in tast.functions {
            let symbol_id = function.symbol_id;
            let name = function.name.clone();

            match self.build_function(function) {
                Ok(mir_function) => {
                    if mir_function.attributes.entry_point {
                        mir_program.entry_point = Some(mir_function.symbol_id);
                    }
                    // CRITICAL FIX: Add function name to symbol map for dynamic resolution
                    mir_program.symbol_name_map.insert(symbol_id, name);
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
            let symbol_id = start_function.symbol_id;
            let name = start_function.name.clone();

            match self.build_function(start_function) {
                Ok(mir_function) => {
                    // Mark the start function as the entry point
                    mir_program.entry_point = Some(mir_function.symbol_id);
                    // CRITICAL FIX: Add start function name to symbol map
                    mir_program.symbol_name_map.insert(symbol_id, name);
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
                        // CRITICAL FIX: Add class method/constructor names to symbol map
                        mir_program
                            .symbol_name_map
                            .insert(function.symbol_id, function.name.clone());
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
        // DEBUG: Show parameter count for functions with default parameters
        eprintln!(
            "DEBUG MIR PARAMS: Function '{}' has {} TAST parameters",
            tast_function.name,
            tast_function.parameters.len()
        );
        for (i, param) in tast_function.parameters.iter().enumerate() {
            eprintln!(
                "  TAST Param[{}]: name='{}' type={:?} has_default={}",
                i,
                param.name,
                param.param_type,
                param.default_value.is_some()
            );
        }

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
            all_functions: self.all_functions.clone(),
        };

        // For class methods and constructors, add implicit 'this' parameter as the first parameter
        if let Some(_class_ctx) = class_context {
            let this_value_id = ValueId(context.function.next_value_id);
            context.function.next_value_id += 1;

            // Create 'this' parameter with class type
            let this_param = MirParameter {
                value_id: this_value_id,
                name: "this".to_string(),
                param_type: MirType::I32, // Instance pointer is i32 in WASM
                location: tast_function.location.clone(),
            };

            context.function.parameters.push(this_param);

            // Add 'this' to scope
            if let Some(current_scope) = context.scope_stack.last_mut() {
                current_scope.insert("this".to_string(), this_value_id);
            }
        }

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

        // DEBUG: Show final MIR parameter count
        eprintln!(
            "DEBUG MIR PARAMS: Function '{}' created {} MIR parameters",
            context.function.name,
            context.function.parameters.len()
        );
        for (i, mir_param) in context.function.parameters.iter().enumerate() {
            eprintln!(
                "  MIR Param[{}]: name='{}' type={:?}",
                i, mir_param.name, mir_param.param_type
            );
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
        eprintln!(
            "DEBUG MIR FUNC: Building function '{}' with {} statements",
            tast_function.name,
            tast_function.body.statements.len()
        );
        eprintln!(
            "DEBUG MIR FUNC:   Has class_context: {}",
            context.class_context.is_some()
        );
        if let Some(ref class) = context.class_context {
            eprintln!("DEBUG MIR FUNC:   Class has {} fields", class.fields.len());
        }

        // Handle automatic return for last expression in non-void functions
        let has_non_void_return = !matches!(tast_function.return_type, ConcreteType::Undefined);
        self.build_function_body(&mut context, &tast_function.body, has_non_void_return)?;

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

        // Build all constructors with class context (constructors can use 'this')
        for constructor in tast_class.constructors {
            match self.build_function_with_class_context(constructor, Some(&class_for_methods)) {
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

    /// Build function body with automatic return handling
    fn build_function_body(
        &mut self,
        context: &mut FunctionBuildContext,
        block: &TastBlock,
        has_non_void_return: bool,
    ) -> Result<(), Vec<CompilerError>> {
        // Enter new scope
        context.scope_stack.push(HashMap::new());

        let num_statements = block.statements.len();

        // Check if the last statement is an expression that should be auto-returned
        let last_is_auto_return = if has_non_void_return && num_statements > 0 {
            matches!(
                block.statements.last(),
                Some(TastStatement::Expression { .. })
            )
        } else {
            false
        };

        // Process all statements except possibly the last
        let statements_to_process = if last_is_auto_return {
            num_statements - 1
        } else {
            num_statements
        };

        for (i, statement) in block
            .statements
            .iter()
            .take(statements_to_process)
            .enumerate()
        {
            eprintln!(
                "DEBUG MIR BLOCK:   Statement {}: {:?}",
                i,
                std::mem::discriminant(statement)
            );
            self.build_statement(context, statement)?;
        }

        // Handle last statement if it should be auto-returned
        if last_is_auto_return {
            if let Some(TastStatement::Expression {
                expression,
                location: _,
            }) = block.statements.last()
            {
                eprintln!("DEBUG MIR FUNC:   Auto-returning last expression");
                // Build expression and convert to return
                let value_id = self.build_expression(context, expression)?;

                // Add return instruction
                if let Some(block_id) = self.current_block {
                    if let Some(current_block) = context.function.blocks.get_mut(&block_id) {
                        current_block.terminator = MirTerminator::Return {
                            value: Some(MirOperand::Value(value_id)),
                        };
                    }
                }
            }
        }

        // Exit scope
        context.scope_stack.pop();

        Ok(())
    }

    /// Build basic block from TAST block
    fn build_block(
        &mut self,
        context: &mut FunctionBuildContext,
        block: &TastBlock,
    ) -> Result<(), Vec<CompilerError>> {
        // Enter new scope
        context.scope_stack.push(HashMap::new());

        eprintln!(
            "DEBUG MIR BLOCK: Processing block with {} statements",
            block.statements.len()
        );

        // Lower all statements
        for (i, statement) in block.statements.iter().enumerate() {
            eprintln!(
                "DEBUG MIR BLOCK:   Statement {}: {:?}",
                i,
                std::mem::discriminant(statement)
            );
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
                // For void expressions, the ValueId won't be used, but we still need to
                // generate the instructions for side effects (like method calls)

                let _value_id = self.build_expression(context, expression)?;

                // Note: For void expressions (ConcreteType::Null), the ValueId is created
                // but never registered in function.locals or used. This is intentional -
                // the expression is executed for its side effects only.
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
                        let converted_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Register the converted_id BEFORE creating the instruction to ensure
                        // it's in the function.locals map for codegen
                        // CRITICAL FIX: Strings are i32 pointers to [len|content] structure in memory
                        self.register_temp_local(
                            context,
                            converted_id,
                            MirType::I32,
                            location.clone(),
                        );

                        let conversion_instruction = MirInstruction {
                            dest: Some(converted_id),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(SymbolId(5)), // int_to_string
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    ConcreteType::Number => {
                        // Convert float to string using float_to_string (SymbolId(6))
                        let converted_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Register the converted_id BEFORE creating the instruction to ensure
                        // it's in the function.locals map for codegen
                        // CRITICAL FIX: Strings are i32 pointers to [len|content] structure in memory
                        self.register_temp_local(
                            context,
                            converted_id,
                            MirType::I32,
                            location.clone(),
                        );

                        let conversion_instruction = MirInstruction {
                            dest: Some(converted_id),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(SymbolId(6)), // float_to_string
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    ConcreteType::Boolean => {
                        // Convert boolean to string using bool_to_string (SymbolId(7))
                        let converted_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Register the converted_id BEFORE creating the instruction to ensure
                        // it's in the function.locals map for codegen
                        // CRITICAL FIX: Strings are i32 pointers to [len|content] structure in memory
                        self.register_temp_local(
                            context,
                            converted_id,
                            MirType::I32,
                            location.clone(),
                        );

                        let conversion_instruction = MirInstruction {
                            dest: Some(converted_id),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(SymbolId(7)), // bool_to_string
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
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

                // Only jump to continue block if the block doesn't already have a Return terminator
                // (return statements set Return terminators, which should not be overwritten)
                let current_terminator = self
                    .current_block
                    .and_then(|bid| context.function.blocks.get(&bid))
                    .map(|b| &b.terminator);

                let has_return = matches!(current_terminator, Some(MirTerminator::Return { .. }));

                if !has_return {
                    self.set_block_terminator(
                        context,
                        MirTerminator::Jump {
                            target: continue_block_id,
                        },
                    );
                }

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

                    // Only jump to continue block if the block doesn't already have a Return terminator
                    // (return statements set Return terminators, which should not be overwritten)
                    let current_terminator = self
                        .current_block
                        .and_then(|bid| context.function.blocks.get(&bid))
                        .map(|b| &b.terminator);

                    let has_return =
                        matches!(current_terminator, Some(MirTerminator::Return { .. }));

                    if !has_return {
                        self.set_block_terminator(
                            context,
                            MirTerminator::Jump {
                                target: continue_block_id,
                            },
                        );
                    }
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

                // Check if both branches have return terminators
                let then_has_return = context
                    .function
                    .blocks
                    .get(&then_block_id)
                    .map(|b| matches!(b.terminator, MirTerminator::Return { .. }))
                    .unwrap_or(false);

                let else_has_return = else_block_id
                    .and_then(|id| context.function.blocks.get(&id))
                    .map(|b| matches!(b.terminator, MirTerminator::Return { .. }))
                    .unwrap_or(false);

                // Only set continue block as current if at least one branch doesn't return
                // (if both branches return, the continue block is unreachable)
                if !(then_has_return && else_has_return) {
                    self.current_block = Some(continue_block_id);
                }
            }

            TastStatement::For {
                iterator: _,
                iterator_name,
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

                // CRITICAL FIX: Register index ValueId as a local
                self.register_temp_local(context, index_value_id, MirType::I32, location.clone());

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
                // Use the actual iterator name from the TAST so variable lookups work correctly
                if let Some(current_scope) = context.scope_stack.last_mut() {
                    current_scope.insert(iterator_name.clone(), iterator_value_id);
                }

                // Create local for iterator variable
                let iterator_local = MirLocal {
                    name: Some(iterator_name.clone()),
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

                // CRITICAL FIX: Register length ValueId as a local
                self.register_temp_local(context, length_value_id, MirType::I32, location.clone());

                // Load array length directly from memory (length is at offset 0)
                let length_instruction = MirInstruction {
                    dest: Some(length_value_id),
                    operation: MirOperation::Load {
                        source: MirOperand::Value(iterable_value),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, length_instruction);

                // Compare index < length
                let condition_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // CRITICAL FIX: Register condition ValueId as a local
                self.register_temp_local(
                    context,
                    condition_value_id,
                    MirType::I32, // Boolean represented as I32
                    location.clone(),
                );

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

                // CRITICAL FIX: Register incremented ValueId as a local
                self.register_temp_local(
                    context,
                    incremented_value_id,
                    MirType::I32,
                    location.clone(),
                );

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
        eprintln!(
            "DEBUG MIR EXPR: Processing expression: {:?}",
            std::mem::discriminant(&expression.kind)
        );
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
                eprintln!("DEBUG MIR VAR: Processing variable '{}'", name);
                eprintln!(
                    "DEBUG MIR VAR:   Has class_context: {}",
                    context.class_context.is_some()
                );
                eprintln!(
                    "DEBUG MIR VAR:   Scope stack depth: {}",
                    context.scope_stack.len()
                );

                // Special case for 'this' - in class methods, 'this' refers to the first parameter
                if name == "this" && context.class_context.is_some() {
                    // In class methods, 'this' is the first parameter
                    // Get the actual ValueId of the first parameter
                    if let Some(first_param) = context.function.parameters.first() {
                        // Return the parameter's ValueId directly - no need to copy
                        return Ok(first_param.value_id);
                    } else {
                        return Err(vec![CompilerError::validation_error(
                            "'this' used in method without instance parameter",
                            expression.location.clone(),
                        )]);
                    }
                }

                // Look up variable in scope stack
                for scope in context.scope_stack.iter().rev() {
                    if let Some(&value_id) = scope.get(name) {
                        return Ok(value_id);
                    }
                }

                // If not found in scope and we're in a class method, check class fields
                // Extract field index before any mutable borrows to avoid borrow checker issues
                let field_index_opt = if let Some(ref class) = context.class_context {
                    eprintln!(
                        "DEBUG MIR: Looking for field '{}' in class with {} fields",
                        name,
                        class.fields.len()
                    );
                    let result = class
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(_, f)| f.name == *name)
                        .map(|(idx, _)| idx);
                    eprintln!("DEBUG MIR: Field search result: {:?}", result);
                    result
                } else {
                    eprintln!("DEBUG MIR: No class context for variable '{}'", name);
                    None
                };

                if let Some(field_index) = field_index_opt {
                    eprintln!(
                        "DEBUG MIR: Found field '{}' at index {}, generating load instructions",
                        name, field_index
                    );
                    // This is an implicit field access - treat as this.field
                    // Need to load 'this', get field pointer, and load field value

                    // Get 'this' from the first parameter
                    let this_value_id =
                        if let Some(first_param) = context.function.parameters.first() {
                            first_param.value_id
                        } else {
                            return Err(vec![CompilerError::validation_error(
                                "Implicit field access in method without instance parameter",
                                expression.location.clone(),
                            )]);
                        };

                    // Create GetElementPtr to get field address
                    let field_ptr_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    let gep_instruction = MirInstruction {
                        dest: Some(field_ptr_id),
                        operation: MirOperation::GetElementPtr {
                            base: MirOperand::Value(this_value_id),
                            indices: vec![MirOperand::Constant(MirConstant::Integer(
                                field_index as i64,
                            ))],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, gep_instruction);

                    // Load the field value
                    let value_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    let load_instruction = MirInstruction {
                        dest: Some(value_id),
                        operation: MirOperation::Load {
                            source: MirOperand::Value(field_ptr_id),
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, load_instruction);

                    return Ok(value_id);
                }

                // TODO: Implement parent class field access with proper load instructions
                // For now, only current class fields are supported

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

                    // Result is a single i32 pointer to [len|content] structure in memory
                    // CRITICAL FIX: Strings are i32 pointers to [len|content] structure in memory
                    self.register_temp_local(
                        context,
                        result_id,
                        MirType::I32,
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

                    // CRITICAL FIX: Pass operator to infer correct result type
                    // Comparison operations always return i32 (boolean), not the operand type
                    let result_type = self.infer_binary_operation_type(
                        &left.expr_type,
                        &right.expr_type,
                        operator,
                    );
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

                // Check if this is a constructor call by examining the return type
                // Constructor calls return a Class type
                if let ConcreteType::Class {
                    symbol_id: class_symbol_id,
                    ..
                } = &expression.expr_type
                {
                    // This is a constructor call - we need to allocate instance memory
                    // and pass the instance pointer as the first argument

                    // Find the class definition to get field count
                    let class_def = context
                        .all_classes
                        .iter()
                        .find(|c| c.symbol_id == *class_symbol_id)
                        .ok_or_else(|| {
                            vec![CompilerError::validation_error(
                                &format!(
                                    "Class definition not found for symbol {:?}",
                                    class_symbol_id
                                ),
                                expression.location.clone(),
                            )]
                        })?;

                    // Calculate instance size: 4 bytes per field (all i32/pointer for now)
                    let instance_size = class_def.fields.len() * 4;

                    // Generate Alloca instruction to allocate instance memory
                    // Note: For heap allocation, this should be converted to mem_alloc during codegen
                    let alloc_result = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    // Register the allocation result as a temporary local
                    self.register_temp_local(
                        context,
                        alloc_result,
                        MirType::I32,
                        expression.location.clone(),
                    );

                    // Create allocation instruction
                    let alloc_instr = MirInstruction {
                        dest: Some(alloc_result),
                        operation: MirOperation::Alloca {
                            size: MirOperand::Constant(MirConstant::Integer(instance_size as i64)),
                            alignment: 4,
                        },
                        location: expression.location.clone(),
                    };

                    self.add_instruction(context, alloc_instr);

                    // Prepend instance pointer as first argument to constructor
                    mir_arguments.push(MirOperand::Value(alloc_result));
                }

                // Add user-provided arguments
                for arg in arguments {
                    let arg_id = self.build_expression(context, arg)?;
                    mir_arguments.push(MirOperand::Value(arg_id));
                }

                // Fill in default parameters for missing arguments
                // CRITICAL FIX: Skip default parameter handling for builtin/stdlib functions
                // Stdlib functions don't have default parameters and aren't in the all_functions list
                // Check if this function is actually in all_functions before trying to look it up
                let function_in_all_functions = context
                    .all_functions
                    .iter()
                    .any(|f| f.symbol_id == function_symbol_id);

                // Look up the function definition and clone the default values to avoid borrow conflicts
                let default_params: Vec<(String, Option<TastExpression>)> =
                    if !function_in_all_functions {
                        // Stdlib/builtin functions don't have default parameters, skip lookup
                        Vec::new()
                    } else {
                        let function_def = context
                            .all_functions
                            .iter()
                            .find(|f| f.symbol_id == function_symbol_id)
                            .expect("Function must exist - we just checked with any()");

                        // Clone the default values for parameters beyond what was provided
                        function_def
                            .parameters
                            .iter()
                            .skip(arguments.len())
                            .map(|p| (p.name.clone(), p.default_value.clone()))
                            .collect()
                    };

                // For each missing parameter, check if it has a default value
                for (param_name, default_value_opt) in default_params {
                    if let Some(default_expr) = default_value_opt {
                        // Build the default value expression
                        let default_id = self.build_expression(context, &default_expr)?;
                        mir_arguments.push(MirOperand::Value(default_id));
                    } else {
                        return Err(vec![CompilerError::validation_error(
                            &format!("Missing required argument for parameter '{}'", param_name),
                            expression.location.clone(),
                        )]);
                    }
                }

                // Always allocate a result ValueId for consistency in MIR SSA form
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Convert the expression type to MIR type
                let result_type = self.convert_concrete_type(&expression.expr_type);

                // CRITICAL FIX: Check if this is a void function
                // Void functions have Null or Undefined types, which convert to void-related MIR types
                let is_void = matches!(
                    expression.expr_type,
                    ConcreteType::Null | ConcreteType::Undefined
                ) || matches!(result_type, MirType::Void)
                    || matches!(&result_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void));

                // ALWAYS register the local to maintain SSA invariant (learned from Context7)
                // This ensures every ValueId has a corresponding entry in the locals map
                self.register_temp_local(
                    context,
                    result_id,
                    result_type,
                    expression.location.clone(),
                );

                // For void functions, set dest = None so codegen knows not to store the result
                let dest_opt = if is_void { None } else { Some(result_id) };

                let instruction = MirInstruction {
                    dest: dest_opt,
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

                // SPECIAL CASE: String.toString() is identity operation - just return the receiver
                if method_symbol.0 == 0
                    && matches!(&receiver.expr_type, ConcreteType::String)
                    && method_name == "toString"
                {
                    return Ok(receiver_id);
                }

                // SPECIAL CASE: Type conversion methods - emit Cast instructions
                if method_symbol.0 == 0 {
                    let receiver_type = &receiver.expr_type;
                    match (receiver_type, method_name.as_str()) {
                        // Number to Integer conversion (f64 -> i32)
                        (ConcreteType::Number, "toInteger") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as i32
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );

                            // Emit Cast instruction from F64 to I32
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Cast {
                                    value: MirOperand::Value(receiver_id),
                                    target_type: MirType::I32,
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Integer to Number conversion (i32 -> f64)
                        (ConcreteType::Integer, "toNumber") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as f64
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::F64,
                                expression.location.clone(),
                            );

                            // Emit Cast instruction from I32 to F64
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Cast {
                                    value: MirOperand::Value(receiver_id),
                                    target_type: MirType::F64,
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Integer to Boolean conversion (i32 -> i32, non-zero = true)
                        (ConcreteType::Integer, "toBoolean") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as i32 (booleans are i32 in WASM)
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );

                            // Emit comparison: receiver != 0
                            let zero_const = MirConstant::Integer(0);
                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::BinaryOp {
                                    op: MirBinaryOp::Ne,
                                    left: MirOperand::Value(receiver_id),
                                    right: MirOperand::Constant(zero_const),
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        _ => {
                            // Not a type conversion method, continue to built-in method handling
                        }
                    }
                }

                // Check if this is a built-in method that should be mapped to a conversion function
                // SymbolId(0) is used as a placeholder for built-in methods that don't have real symbols
                let (function_symbol, mir_arguments) = if method_symbol.0 == 0 {
                    // This is a built-in method - determine the correct function based on receiver type and method name
                    let receiver_type = &receiver.expr_type;
                    match (receiver_type, method_name.as_str()) {
                        // Type conversion methods
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
                        // String methods
                        (ConcreteType::String, "length") => {
                            // Call string_length (SymbolId 48) with the string value
                            (SymbolId(48), vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "toUpperCase") => {
                            // Call string_toUpperCase (SymbolId 50)
                            (SymbolId(50), vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "toLowerCase") => {
                            // Call string_toLowerCase (SymbolId 51)
                            (SymbolId(51), vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "substring") => {
                            // Call string_substring (SymbolId 49)
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (SymbolId(49), args)
                        }
                        (ConcreteType::String, "contains") => {
                            // Call string_contains (SymbolId 52)
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (SymbolId(52), args)
                        }
                        // Array/List methods
                        (ConcreteType::Array(_), "size" | "length") => {
                            // Call list_size (SymbolId 53) with the array value
                            (SymbolId(53), vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Array(_), "add" | "push") => {
                            // Call list_push (SymbolId 54) with array and element
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (SymbolId(54), args)
                        }
                        (ConcreteType::Array(_), "remove" | "pop") => {
                            // Call list_pop (SymbolId 55) with the array value
                            (SymbolId(55), vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Array(_), "get") => {
                            // Call list_get (SymbolId 56)
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (SymbolId(56), args)
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

                // Always allocate a result ValueId for consistency in MIR SSA form
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                eprintln!(
                    "DEBUG MIR METHODCALL: Allocated {:?} for method '{}'",
                    result_id, method_name
                );

                // Convert the expression type to MIR type
                let result_type = self.convert_concrete_type(&expression.expr_type);

                // CRITICAL FIX: Check if this is a void method
                // Void methods have Null or Undefined types, which convert to void-related MIR types
                let is_void = matches!(
                    expression.expr_type,
                    ConcreteType::Null | ConcreteType::Undefined
                ) || matches!(result_type, MirType::Void)
                    || matches!(&result_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void));

                eprintln!(
                    "DEBUG MIR METHODCALL: {:?} is_void={} expr_type={:?} mir_type={:?}",
                    result_id, is_void, expression.expr_type, result_type
                );

                // ALWAYS register the local to maintain SSA invariant (learned from Context7)
                // This ensures every ValueId has a corresponding entry in the locals map
                self.register_temp_local(
                    context,
                    result_id,
                    result_type.clone(),
                    expression.location.clone(),
                );

                eprintln!(
                    "DEBUG MIR METHODCALL: Registered {:?} in locals (void={})",
                    result_id, is_void
                );

                // For void methods, set dest = None so codegen knows not to store the result
                let dest_opt = if is_void { None } else { Some(result_id) };

                let instruction = MirInstruction {
                    dest: dest_opt,
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
                property_name,
                property_symbol,
            } => {
                // Build the object expression first
                let object_id = self.build_expression(context, object)?;

                // Use GetElementPtr for field access - this will be handled by codegen
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Add GetElementPtr result to locals
                let gep_local = MirLocal {
                    name: None,               // Temporary value
                    local_type: MirType::I32, // Pointer type
                    is_mutable: false,
                    location: expression.location.clone(),
                };
                context.function.locals.insert(result_id, gep_local);

                // CRITICAL FIX: Get the class from the object's type, not from class_context
                // This allows field access from any context (e.g., start() function)
                let object_class_symbol = match &object.expr_type {
                    ConcreteType::Class { symbol_id, .. } => Some(*symbol_id),
                    _ => None,
                };

                // Find the actual field index in the class hierarchy (including inherited fields)
                let field_index_value = if let Some(class_symbol) = object_class_symbol {
                    // Search for the field in the object's class and all parent classes
                    self.find_field_index_for_class(context, class_symbol, property_symbol)
                        .ok_or_else(|| {
                            vec![CompilerError::validation_error(
                                &format!(
                                    "Field '{}' not found in class or parent classes",
                                    property_name
                                ),
                                expression.location.clone(),
                            )]
                        })? as i64
                } else {
                    // Object doesn't have a class type - this shouldn't happen for field access
                    return Err(vec![CompilerError::validation_error(
                        &format!(
                            "Cannot access field '{}' on non-class type: {:?}",
                            property_name, object.expr_type
                        ),
                        expression.location.clone(),
                    )]);
                };

                let field_index = MirOperand::Constant(MirConstant::Integer(field_index_value));
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

                // Add Load result to locals
                let load_local = MirLocal {
                    name: Some(format!("field_{}", property_name)),
                    local_type: MirType::I32, // Field value type - TODO: Get actual field type
                    is_mutable: false,
                    location: expression.location.clone(),
                };
                context.function.locals.insert(load_result_id, load_local);

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

                // CRITICAL FIX: Register the pointer result as a local
                // GetElementPtr returns a pointer to the array element
                self.register_temp_local(
                    context,
                    result_id,
                    MirType::Ptr(Box::new(MirType::I32)), // Pointer to element
                    expression.location.clone(),
                );

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

                // CRITICAL FIX: Register the loaded value as a local
                // Determine the type from the array expression type
                let element_type = match &array.expr_type {
                    ConcreteType::Array(elem_type) => self.convert_concrete_type(elem_type),
                    ConcreteType::Matrix(elem_type) => {
                        // Matrix is 2D array, so element is 1D array
                        MirType::Ptr(Box::new(self.convert_concrete_type(elem_type)))
                    }
                    _ => MirType::I32, // Default fallback
                };

                self.register_temp_local(
                    context,
                    load_result_id,
                    element_type,
                    expression.location.clone(),
                );

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

            TastExpressionKind::BaseCall {
                parent_class_symbol_id,
                arguments,
            } => {
                // Base constructor call: base(args...)
                // This is called from a derived class constructor to initialize the base class fields
                // The derived class constructor has already allocated the instance, so we pass
                // the 'this' pointer as the first argument to the base constructor

                eprintln!(
                    "DEBUG MIR BASECALL: Processing base() call to parent class {:?}",
                    parent_class_symbol_id
                );

                // Build argument operands
                let mut mir_arguments = Vec::new();

                // Get 'this' from the first parameter of the current constructor
                // In class constructors, 'this' is always the first parameter
                let this_value_id = if let Some(first_param) = context.function.parameters.first() {
                    first_param.value_id
                } else {
                    return Err(vec![CompilerError::validation_error(
                        "base() call outside of constructor or constructor has no 'this' parameter",
                        expression.location.clone(),
                    )]);
                };

                eprintln!(
                    "DEBUG MIR BASECALL: Got 'this' ValueId = {:?}",
                    this_value_id
                );

                // Prepend 'this' as first argument to base constructor
                mir_arguments.push(MirOperand::Value(this_value_id));

                // Add user-provided arguments
                for (i, arg) in arguments.iter().enumerate() {
                    eprintln!(
                        "DEBUG MIR BASECALL: Processing argument {} of {}",
                        i + 1,
                        arguments.len()
                    );
                    let arg_id = self.build_expression(context, arg)?;
                    mir_arguments.push(MirOperand::Value(arg_id));
                }

                eprintln!(
                    "DEBUG MIR BASECALL: Total arguments (including this): {}",
                    mir_arguments.len()
                );

                // Allocate a result ValueId even though base constructors return void
                // This maintains SSA form consistency
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Base constructors always return void
                let result_type = MirType::Void;

                // Register the ValueId as a local (even for void)
                self.register_temp_local(
                    context,
                    result_id,
                    result_type,
                    expression.location.clone(),
                );

                // CRITICAL FIX: Find the parent class constructor SymbolId
                // parent_class_symbol_id is the CLASS, we need to find its CONSTRUCTOR
                let parent_constructor_symbol_id = if let Some(parent_symbol) =
                    self.symbol_table.all_symbols().get(parent_class_symbol_id)
                {
                    // For classes, look for a child symbol that is a Constructor for this class
                    let constructor_id = self.symbol_table.all_symbols().iter()
                        .find(|(_, symbol)| {
                            matches!(symbol.kind, crate::resolver::SymbolKind::Constructor { class_id, .. } if class_id == *parent_class_symbol_id)
                        })
                        .map(|(id, _)| *id);

                    if let Some(constructor_id) = constructor_id {
                        eprintln!(
                            "DEBUG MIR BASECALL: Found parent constructor SymbolId({}) for class '{}'",
                            constructor_id.0, parent_symbol.name
                        );
                        constructor_id
                    } else {
                        eprintln!(
                            "DEBUG MIR BASECALL: WARNING - No constructor found for parent class '{}' (SymbolId({})), using class SymbolId",
                            parent_symbol.name, parent_class_symbol_id.0
                        );
                        *parent_class_symbol_id
                    }
                } else {
                    eprintln!(
                        "DEBUG MIR BASECALL: WARNING - Parent class SymbolId({}) not found in symbol table",
                        parent_class_symbol_id.0
                    );
                    *parent_class_symbol_id
                };

                // Create function call instruction
                // Base constructors are void, so dest = None
                let call_instruction = MirInstruction {
                    dest: None, // Void return
                    operation: MirOperation::Call {
                        function: MirOperand::Function(parent_constructor_symbol_id),
                        arguments: mir_arguments,
                    },
                    location: expression.location.clone(),
                };

                eprintln!(
                    "DEBUG MIR BASECALL: Generated call instruction: {:?}",
                    call_instruction
                );

                self.add_instruction(context, call_instruction);

                // Return the result_id for consistency (even though it represents void)
                Ok(result_id)
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
        operator: &BinaryOperator,
    ) -> MirType {
        // CRITICAL FIX: Comparison and logical operations always return i32 (boolean)
        match operator {
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::LessThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::LessThanOrEqual
            | BinaryOperator::GreaterThanOrEqual
            | BinaryOperator::And
            | BinaryOperator::Or => {
                return MirType::I32; // Boolean result
            }
            _ => {}
        }

        // For arithmetic and other operations, infer from operand types
        match (left_type, right_type) {
            // Arithmetic operations between numeric types
            (ConcreteType::Integer, ConcreteType::Integer) => MirType::I32,
            (ConcreteType::Number, ConcreteType::Number) => MirType::F64,
            (ConcreteType::Number, ConcreteType::Integer) => MirType::F64,
            (ConcreteType::Integer, ConcreteType::Number) => MirType::F64,

            // Boolean operations
            (ConcreteType::Boolean, ConcreteType::Boolean) => MirType::Bool,

            // String operations (concatenation) - result is string
            // CRITICAL FIX: Strings are i32 pointers to [len|content] structure in memory
            (ConcreteType::String, ConcreteType::String) => MirType::I32,
            (ConcreteType::String, _) => MirType::I32, // String + any = String
            (_, ConcreteType::String) => MirType::I32, // Any + String = String

            // Array operations (if supported) - result is array pointer
            (ConcreteType::Array(elem_type), ConcreteType::Array(_)) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Same type operations - use the type's MIR representation
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
            // CRITICAL FIX: Strings are i32 pointers to [len|content] structure in memory
            ConcreteType::String => MirType::I32,

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

    /// Find field index in class hierarchy, searching through parent classes if needed
    ///
    /// Fields are laid out in memory starting with the most distant ancestor's fields first.
    /// For example, if Cat extends Animal:
    /// - Animal fields: [name, age]
    /// - Cat fields: [isIndoor]
    /// - Memory layout: [name(0), age(1), isIndoor(2)]
    fn find_field_index_in_hierarchy(
        &self,
        context: &FunctionBuildContext,
        property_symbol: &SymbolId,
    ) -> Option<usize> {
        // Collect all classes in the hierarchy from current to root
        let mut hierarchy = Vec::new();
        let mut current_class_opt = context.class_context.as_ref();

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

        // Now search through hierarchy and count field offsets
        let mut field_offset = 0usize;

        for class in &hierarchy {
            if let Some(position) = class
                .fields
                .iter()
                .position(|f| f.symbol_id == *property_symbol)
            {
                return Some(field_offset + position);
            }
            // Move offset past this class's fields
            field_offset += class.fields.len();
        }

        None
    }

    /// Find field index for a specific class in its hierarchy, searching through parent classes if needed
    ///
    /// This is similar to find_field_index_in_hierarchy, but takes an explicit class_symbol instead
    /// of using context.class_context. This allows field access from any context (e.g., start() function).
    ///
    /// Fields are laid out in memory starting with the most distant ancestor's fields first.
    /// For example, if Child extends Base:
    /// - Base fields: [name]
    /// - Child fields: [flag]
    /// - Memory layout: [name(0), flag(1)]
    fn find_field_index_for_class(
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

        // Now search through hierarchy and count field offsets
        let mut field_offset = 0usize;

        for class in &hierarchy {
            if let Some(position) = class
                .fields
                .iter()
                .position(|f| f.symbol_id == *property_symbol)
            {
                return Some(field_offset + position);
            }
            // Move offset past this class's fields
            field_offset += class.fields.len();
        }

        None
    }
}

impl Default for MirBuilder {
    fn default() -> Self {
        // Create empty symbol table for default initialization
        let empty_symbol_table = std::sync::Arc::new(crate::resolver::GlobalSymbolTable::new());
        Self::new(empty_symbol_table)
    }
}
