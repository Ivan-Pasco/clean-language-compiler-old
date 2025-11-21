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
        // list.allocate and list.push for array literal creation
        mir_program
            .symbol_name_map
            .insert(SymbolId(1003), "list.allocate".to_string());
        mir_program
            .symbol_name_map
            .insert(SymbolId(1004), "list.push".to_string());
        mir_program
            .symbol_name_map
            .insert(SymbolId(1005), "list.push_f64".to_string());

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
            "DEBUG MIR SYMBOLS: Added 5 synthetic SymbolIds (1000-1004) for MIR-generated builtins"
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
        // EXCEPT for static methods which don't need 'this'
        eprintln!(
            "DEBUG THIS PARAM: Function '{}' has_class_context={} is_static={}",
            tast_function.name,
            class_context.is_some(),
            tast_function.is_static
        );
        if let Some(_class_ctx) = class_context {
            if !tast_function.is_static {
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

        // DEBUG: Check all blocks for proper terminators
        if context.function.name == "test" {
            eprintln!(
                "DEBUG FINAL MIR: Function '{}' has {} blocks",
                context.function.name,
                context.function.blocks.len()
            );
            for (block_id, block) in &context.function.blocks {
                eprintln!("  Block {:?}: terminator={:?}", block_id, block.terminator);
            }
        }

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
                "DEBUG MIR BLOCK:   Statement {}: {:?}, current_block = {:?}",
                i,
                std::mem::discriminant(statement),
                self.current_block
            );
            self.build_statement(context, statement)?;
            eprintln!(
                "DEBUG MIR BLOCK:   After statement {}: current_block = {:?}",
                i, self.current_block
            );
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
                "DEBUG MIR BLOCK:   Statement {}: {:?}, current_block = {:?}",
                i,
                std::mem::discriminant(statement),
                self.current_block
            );
            self.build_statement(context, statement)?;
            eprintln!(
                "DEBUG MIR BLOCK:   After statement {}: current_block = {:?}",
                i, self.current_block
            );
        }

        // Exit scope
        context.scope_stack.pop();

        Ok(())
    }

    /// Helper function to check if a block effectively returns (has Return OR Branch where both branches return)
    fn block_effectively_returns(
        &self,
        context: &FunctionBuildContext,
        block_id: BasicBlockId,
    ) -> bool {
        let mut visited = std::collections::HashSet::new();
        self.block_effectively_returns_recursive(context, block_id, &mut visited)
    }

    fn block_effectively_returns_recursive(
        &self,
        context: &FunctionBuildContext,
        block_id: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
    ) -> bool {
        // Prevent infinite loops
        if visited.contains(&block_id) {
            return false;
        }
        visited.insert(block_id);

        let block = match context.function.blocks.get(&block_id) {
            Some(b) => b,
            None => return false,
        };

        match &block.terminator {
            MirTerminator::Return { .. } => true,
            MirTerminator::Branch {
                true_block,
                false_block,
                ..
            } => {
                // Both branches must effectively return
                self.block_effectively_returns_recursive(context, *true_block, visited)
                    && self.block_effectively_returns_recursive(context, *false_block, visited)
            }
            MirTerminator::Unreachable | MirTerminator::Jump { .. } => false,
        }
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
                        property_symbol,
                    } => {
                        // Handle field assignments like obj.field = value or this.field = value
                        // Build the object expression first
                        let object_id = self.build_expression(context, object)?;

                        // Get the class from the object's type
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
                                        target.location.clone(),
                                    )]
                                })? as i64
                        } else {
                            // Object doesn't have a class type - this shouldn't happen for field access
                            return Err(vec![CompilerError::validation_error(
                                &format!(
                                    "Cannot assign to field '{}' on non-class type: {:?}",
                                    property_name, object.expr_type
                                ),
                                target.location.clone(),
                            )]);
                        };

                        // Generate GetElementPtr to get the field address
                        let field_ptr_id = ValueId(context.function.next_value_id);
                        context.function.next_value_id += 1;

                        // Add GetElementPtr result to locals
                        let gep_local = MirLocal {
                            name: None,               // Temporary value
                            local_type: MirType::I32, // Pointer type
                            is_mutable: false,
                            location: target.location.clone(),
                        };
                        context.function.locals.insert(field_ptr_id, gep_local);

                        let field_index =
                            MirOperand::Constant(MirConstant::Integer(field_index_value));
                        let gep_instruction = MirInstruction {
                            dest: Some(field_ptr_id),
                            operation: MirOperation::GetElementPtr {
                                base: MirOperand::Value(object_id),
                                indices: vec![field_index],
                            },
                            location: target.location.clone(),
                        };
                        self.add_instruction(context, gep_instruction);

                        // Generate Store instruction to write the value to the field
                        let store_instruction = MirInstruction {
                            dest: None, // Store doesn't produce a value
                            operation: MirOperation::Store {
                                destination: MirOperand::Value(field_ptr_id),
                                value: MirOperand::Value(value_id),
                            },
                            location: target.location.clone(),
                        };
                        self.add_instruction(context, store_instruction);

                        // Also update the scope for simple field references (for optimization)
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
                // CRITICAL FIX: Debug logging for return statement processing
                eprintln!(
                    "DEBUG RETURN STATEMENT: function='{}', current_block={:?}",
                    context.function.name, self.current_block
                );

                // Return type validation already done in type checking phase
                let return_value = if let Some(expr) = value {
                    let value_id = self.build_expression(context, expr)?;
                    eprintln!(
                        "DEBUG RETURN: Built return expression, value_id={:?}",
                        value_id
                    );
                    Some(MirOperand::Value(value_id))
                } else {
                    eprintln!("DEBUG RETURN: No return value (void return)");
                    None
                };

                // Create return terminator
                let terminator = MirTerminator::Return {
                    value: return_value.clone(),
                };
                eprintln!(
                    "DEBUG RETURN: Created terminator={:?}, calling set_block_terminator",
                    terminator
                );
                self.set_block_terminator(context, terminator);
                eprintln!(
                    "DEBUG RETURN: After set_block_terminator, current_block={:?}",
                    self.current_block
                );
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
                        // Convert integer to string using int_to_string
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

                        let symbol_id = self.symbol_table.lookup_symbol("int_to_string")
                            .unwrap_or_else(|| {
                                eprintln!("WARNING: int_to_string not found in symbol table, using SymbolId(166)");
                                SymbolId(166)
                            });

                        let conversion_instruction = MirInstruction {
                            dest: Some(converted_id),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(symbol_id),
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    ConcreteType::Number => {
                        // Convert float to string using float_to_string
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

                        let symbol_id = self.symbol_table.lookup_symbol("float_to_string")
                            .unwrap_or_else(|| {
                                eprintln!("WARNING: float_to_string not found in symbol table, using SymbolId(167)");
                                SymbolId(167)
                            });

                        let conversion_instruction = MirInstruction {
                            dest: Some(converted_id),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(symbol_id),
                                arguments: vec![MirOperand::Value(value_id)],
                            },
                            location: location.clone(),
                        };
                        self.add_instruction(context, conversion_instruction);
                        converted_id
                    }
                    ConcreteType::Boolean => {
                        // Convert boolean to string using bool_to_string
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

                        let symbol_id = self.symbol_table.lookup_symbol("bool_to_string")
                            .unwrap_or_else(|| {
                                eprintln!("WARNING: bool_to_string not found in symbol table, using SymbolId(165)");
                                SymbolId(165)
                            });

                        let conversion_instruction = MirInstruction {
                            dest: Some(converted_id),
                            operation: MirOperation::Call {
                                function: MirOperand::Function(symbol_id),
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

                // CRITICAL FIX: Create basic blocks for then, else, AND continue upfront
                // This prevents block ID collisions when nested statements create new blocks
                let then_block_id = BasicBlockId(context.function.blocks.len());
                let else_block_id = if else_block.is_some() {
                    Some(BasicBlockId(context.function.blocks.len() + 1))
                } else {
                    None
                };
                let continue_block_id = BasicBlockId(
                    context.function.blocks.len() + if else_block.is_some() { 2 } else { 1 },
                );

                // Pre-allocate continue block to reserve its ID
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

                // CRITICAL FIX: Check the THEN BLOCK's terminator, not current_block
                // After processing nested statements (like nested If), current_block may point
                // to a different block. We need to check the then block we just built.
                let then_terminator = context
                    .function
                    .blocks
                    .get(&then_block_id)
                    .map(|b| &b.terminator);

                eprintln!(
                    "DEBUG THEN CHECK: then_block={:?}, terminator={:?}",
                    then_block_id, then_terminator
                );

                // FIX: Check if block effectively returns (direct Return OR Branch where both branches return)
                // Unreachable here is just a placeholder that should be replaced with Jump
                let has_return = self.block_effectively_returns(context, then_block_id);
                eprintln!(
                    "DEBUG THEN EFFECTIVE RETURN: then_block={:?}, has_return={}",
                    then_block_id, has_return
                );

                if !has_return {
                    eprintln!("DEBUG THEN: Adding Jump to continue block");
                    // Set the then block's terminator to jump to continuation
                    // This replaces the placeholder Unreachable or sets terminator if not set
                    let saved_current = self.current_block;
                    self.current_block = Some(then_block_id);
                    self.set_block_terminator(
                        context,
                        MirTerminator::Jump {
                            target: continue_block_id,
                        },
                    );
                    self.current_block = saved_current;
                } else {
                    eprintln!("DEBUG THEN: Already has return, skipping Jump");
                }

                // Track whether the else branch returns (all paths)
                let else_returns_all_paths = if let Some(else_stmt_block) = else_block {
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

                    // Save current_block before processing else block
                    let before_else = self.current_block;

                    // Process else block statements
                    for stmt in &else_stmt_block.statements {
                        self.build_statement(context, stmt)?;
                    }

                    // CRITICAL FIX: Check if current_block is None after processing else block
                    // If current_block is None, it means all paths in the else block returned
                    // This handles nested if-else-if chains correctly
                    let after_else = self.current_block;

                    // Check if all paths return:
                    // 1. current_block is None (nested if set it to None because both branches returned)
                    // 2. OR current_block points to a block with a Return terminator
                    // FIX: Only Return counts, Unreachable is just a placeholder
                    let else_returns = if after_else.is_none() {
                        true
                    } else if let Some(final_block_id) = after_else {
                        context
                            .function
                            .blocks
                            .get(&final_block_id)
                            .map(|b| matches!(b.terminator, MirTerminator::Return { .. }))
                            .unwrap_or(false)
                    } else {
                        false
                    };

                    eprintln!("DEBUG ELSE RETURN CHECK: before_else={:?}, after_else={:?}, else_returns={}", before_else, after_else, else_returns);

                    if !else_returns {
                        // At least one path doesn't return - add jump to continuation
                        let saved_current = self.current_block;
                        if let Some(curr) = saved_current {
                            self.current_block = Some(curr);
                            self.set_block_terminator(
                                context,
                                MirTerminator::Jump {
                                    target: continue_block_id,
                                },
                            );
                        }
                        self.current_block = saved_current;
                    } else {
                        eprintln!("DEBUG ELSE: All paths return");
                    }

                    else_returns
                } else {
                    false
                };

                // Continue block was already created above to reserve its ID
                // No need to create it again here

                // Check if both branches have return terminators
                let then_has_return = context
                    .function
                    .blocks
                    .get(&then_block_id)
                    .map(|b| matches!(b.terminator, MirTerminator::Return { .. }))
                    .unwrap_or(false);

                eprintln!("DEBUG IF FINAL: then_has_return={}, else_returns_all_paths={}, current_block before={:?}", then_has_return, else_returns_all_paths, self.current_block);

                // CRITICAL FIX: Handle continue block based on whether branches return
                // Use else_returns_all_paths instead of checking the else block's entry terminator
                if then_has_return && else_returns_all_paths && else_block.is_some() {
                    // Both branches return - continue block is truly unreachable
                    // Set current_block to None to prevent ensure_function_termination from adding a return
                    eprintln!("DEBUG IF FINAL: Both branches return, setting current_block to None (unreachable)");
                    self.current_block = None;
                } else {
                    // At least one branch doesn't return - continue block is reachable
                    // Set current_block to continue block so execution can proceed
                    eprintln!("DEBUG IF FINAL: At least one branch continues, setting current_block to continue_block={:?}", continue_block_id);
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
                // OPTIMIZATION: Check if iterable is a Range expression
                // If so, generate optimized loop code directly instead of building an array
                if let TastExpressionKind::Range {
                    start,
                    end,
                    inclusive,
                } = &iterable.kind
                {
                    eprintln!("DEBUG FOR RANGE: Detected Range expression in For loop, generating optimized code");

                    // Generate optimized range loop directly
                    return self.build_range_for_loop(
                        context,
                        iterator_name,
                        start,
                        end,
                        *inclusive,
                        body,
                        location,
                    );
                }

                // Build the iterable expression (e.g., array, range)
                let iterable_value = self.build_expression(context, iterable)?;

                // Create loop blocks
                // CRITICAL FIX: Add dedicated increment block to ensure counter
                // is always incremented, even when loop body contains IF statements
                let header_block_id = BasicBlockId(context.function.blocks.len());
                let body_block_id = BasicBlockId(context.function.blocks.len() + 1);
                let increment_block_id = BasicBlockId(context.function.blocks.len() + 2);
                let exit_block_id = BasicBlockId(context.function.blocks.len() + 3);

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

                // Save the init block ID (current block) for Phi node
                let init_block_id = self.current_block.expect("No current block for loop init");

                // We'll create current_index_value_id here so we can set it in init block
                // (before creating header block)
                let current_index_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    current_index_value_id,
                    MirType::I32,
                    location.clone(),
                );

                // Set current_index to initial value (0) in init block
                let init_current_instruction = MirInstruction {
                    dest: Some(current_index_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Value(index_value_id),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, init_current_instruction);

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

                // SSA FIX: Create Phi node to merge index values from different predecessors
                // - From init block: initial 0
                // - From body block: incremented value
                // current_index_value_id was already created and set in init block
                // Create Phi node with init block predecessor
                // Body block predecessor will be added later after we know the incremented ValueId
                let phi_instruction = MirInstruction {
                    dest: Some(current_index_value_id),
                    operation: MirOperation::Phi {
                        incoming: vec![(init_block_id, MirOperand::Value(index_value_id))],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, phi_instruction);

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

                // Compare index < length (use the reloaded current_index_value_id)
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
                        left: MirOperand::Value(current_index_value_id),
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

                // CRITICAL FIX: Get the address of the element, then LOAD the value
                // GetElementPtr returns a pointer, not the value itself!

                // Step 1: Get element pointer
                let element_ptr_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    element_ptr_value_id,
                    MirType::Ptr(Box::new(MirType::I32)),
                    location.clone(),
                );

                eprintln!(
                    "DEBUG ITERATE: current_block before GetElementPtr = {:?}",
                    self.current_block
                );
                let get_ptr_instruction = MirInstruction {
                    dest: Some(element_ptr_value_id),
                    operation: MirOperation::GetElementPtr {
                        base: MirOperand::Value(iterable_value),
                        indices: vec![MirOperand::Value(current_index_value_id)],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, get_ptr_instruction);
                eprintln!(
                    "DEBUG ITERATE: after adding GetElementPtr, block has {} instructions",
                    context
                        .function
                        .blocks
                        .get(&body_block_id)
                        .map(|b| b.instructions.len())
                        .unwrap_or(0)
                );

                // Step 2: Load the actual value from that pointer
                let load_element_instruction = MirInstruction {
                    dest: Some(iterator_value_id),
                    operation: MirOperation::Load {
                        source: MirOperand::Value(element_ptr_value_id),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, load_element_instruction);

                // DEBUG: Check instructions before processing statements
                eprintln!("DEBUG ITERATE (function={}): Before processing statements, body block has {} instructions",
                    context.function.name,
                    context.function.blocks.get(&body_block_id).map(|b| b.instructions.len()).unwrap_or(0));

                // Process loop body statements
                for stmt in &body.statements {
                    self.build_statement(context, stmt)?;
                }

                // DEBUG: Check instructions after processing statements
                eprintln!(
                    "DEBUG ITERATE: After processing statements, body block has {} instructions",
                    context
                        .function
                        .blocks
                        .get(&body_block_id)
                        .map(|b| b.instructions.len())
                        .unwrap_or(0)
                );

                // CRITICAL FIX: Check if body block already has a terminator
                // (IF statements may have set one). If not, jump to increment block.
                let body_has_terminator = context
                    .function
                    .blocks
                    .get(&body_block_id)
                    .map(|b| !matches!(b.terminator, MirTerminator::Unreachable))
                    .unwrap_or(false);

                if !body_has_terminator {
                    // Body block needs a terminator - jump to increment block
                    self.current_block = Some(body_block_id);
                    self.set_block_terminator(
                        context,
                        MirTerminator::Jump {
                            target: increment_block_id,
                        },
                    );
                } else {
                    // Body block already has terminator (from IF/etc)
                    // Need to redirect it to increment block instead of header
                    // For now, we'll handle this in the increment block by having
                    // continuation blocks jump to increment instead
                    eprintln!(
                        "DEBUG ITERATE: Body block already has terminator, current_block={:?}",
                        self.current_block
                    );

                    // Set current block to wherever we ended up after processing statements
                    // and make it jump to increment block
                    if let Some(_curr_block) = self.current_block {
                        self.set_block_terminator(
                            context,
                            MirTerminator::Jump {
                                target: increment_block_id,
                            },
                        );
                    }
                }

                // Create increment block
                context.function.blocks.insert(
                    increment_block_id,
                    MirBasicBlock {
                        id: increment_block_id,
                        label: Some("for_increment".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: location.clone(),
                    },
                );
                self.current_block = Some(increment_block_id);

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
                        left: MirOperand::Value(current_index_value_id),
                        right: MirOperand::Constant(MirConstant::Integer(1)),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, increment_instruction);

                // SSA FIX: Copy the incremented value to the Phi result local
                // This ensures the Phi node sees the updated value on the next iteration
                let update_phi_instruction = MirInstruction {
                    dest: Some(current_index_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Value(incremented_value_id),
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, update_phi_instruction);

                // SSA FIX: Update the Phi node in the header block with the INCREMENT BLOCK's incremented value
                // The Phi node merges:
                // - init_block → index_value_id (0)
                // - increment_block → incremented_value_id (updated counter)
                if let Some(header_block) = context.function.blocks.get_mut(&header_block_id) {
                    if let Some(first_instr) = header_block.instructions.first_mut() {
                        if let MirOperation::Phi { incoming } = &mut first_instr.operation {
                            incoming.push((
                                increment_block_id,
                                MirOperand::Value(incremented_value_id),
                            ));
                        }
                    }
                }

                // Jump back to header from increment block
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

                    // CRITICAL FIX: Use actual MIR types from built expressions, not TAST expr_type
                    // TAST expr_type may be Unknown for method calls like toNumber()
                    let left_mir_type = context
                        .function
                        .locals
                        .get(&left_id)
                        .map(|local| local.local_type.clone())
                        .unwrap_or_else(|| MirType::from_concrete_type(&left.expr_type));
                    let right_mir_type = context
                        .function
                        .locals
                        .get(&right_id)
                        .map(|local| local.local_type.clone())
                        .unwrap_or_else(|| MirType::from_concrete_type(&right.expr_type));

                    let left_concrete = Self::mir_type_to_concrete(&left_mir_type);
                    let right_concrete = Self::mir_type_to_concrete(&right_mir_type);

                    let result_type =
                        self.infer_binary_operation_type(&left_concrete, &right_concrete, operator);
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

                // CRITICAL FIX: For print/printl function calls, convert all arguments to strings
                // Check if this is a print/printl call
                let is_print_call = function_symbol_id == SymbolId(0) // print
                    || function_symbol_id == SymbolId(1) // printl
                    || function_symbol_id == SymbolId(162) // print (alternative)
                    || function_symbol_id == SymbolId(163); // printl (alternative)

                // Add user-provided arguments
                for arg in arguments {
                    let arg_id = self.build_expression(context, arg)?;

                    // For print calls, convert arguments to strings
                    let final_arg_id = if is_print_call {
                        self.convert_value_to_string(
                            context,
                            arg_id,
                            &arg.expr_type,
                            &arg.location,
                        )?
                    } else {
                        arg_id
                    };

                    mir_arguments.push(MirOperand::Value(final_arg_id));
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
                eprintln!("DEBUG IS_VOID CHECK: function_symbol_id={:?}, expression.expr_type={:?}, result_type={:?}",
                          function_symbol_id, expression.expr_type, result_type);
                let is_void = matches!(
                    expression.expr_type,
                    ConcreteType::Null | ConcreteType::Undefined
                ) || matches!(result_type, MirType::Void)
                    || matches!(&result_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void));
                eprintln!("DEBUG IS_VOID RESULT: is_void={}", is_void);

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

                // CRITICAL FIX: For namespace functions (SymbolId(0)), create NamedFunction operand
                // so codegen can look up the function by name instead of symbol ID
                let function_operand = if function_symbol_id.0 == 0 {
                    // Get function name from the Variable expression
                    let function_name = match &function.kind {
                        TastExpressionKind::Variable { name, .. } => name.clone(),
                        _ => String::from("unknown"),
                    };
                    eprintln!("DEBUG MIR FUNCTIONCALL: Creating NamedFunction for function_name='{}' with SymbolId(0)",
                              function_name);
                    MirOperand::NamedFunction {
                        name: function_name,
                        symbol_id: function_symbol_id,
                    }
                } else {
                    MirOperand::Function(function_symbol_id)
                };

                let instruction = MirInstruction {
                    dest: dest_opt,
                    operation: MirOperation::Call {
                        function: function_operand,
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

                // CRITICAL FIX: Get the ACTUAL type of the receiver
                // Priority: 1) Use TAST type if not Unknown, 2) Infer from locals map, 3) Use Unknown
                let receiver_actual_type = if !matches!(receiver.expr_type, ConcreteType::Unknown) {
                    // TAST has the type - use it
                    receiver.expr_type.clone()
                } else {
                    // TAST type is Unknown - try to infer from locals map
                    context
                        .function
                        .locals
                        .get(&receiver_id)
                        .map(|mir_local| Self::mir_type_to_concrete(&mir_local.local_type))
                        .unwrap_or(ConcreteType::Unknown)
                };

                eprintln!("DEBUG MIR METHODCALL RECEIVER: method='{}', receiver_id={:?}, tast_type={:?}, actual_type={:?}",
                    method_name, receiver_id, receiver.expr_type, receiver_actual_type);

                // SPECIAL CASE: String.toString() is identity operation - just return the receiver
                if method_symbol.0 == 0
                    && matches!(&receiver.expr_type, ConcreteType::String)
                    && method_name == "toString"
                {
                    return Ok(receiver_id);
                }

                // SPECIAL CASE: Type conversion methods - emit Cast instructions or builtin calls
                if method_symbol.0 == 0 {
                    let receiver_type = &receiver.expr_type;
                    match (receiver_type, method_name.as_str()) {
                        // Integer to String conversion - call int_to_string
                        (ConcreteType::Integer, "toString") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as i32 (string pointer)
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );

                            let symbol_id = self
                                .symbol_table
                                .lookup_symbol("int_to_string")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: int_to_string not found in symbol table, using SymbolId(166)");
                                    SymbolId(166)
                                });

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(symbol_id),
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Number to String conversion - call float_to_string
                        (ConcreteType::Number, "toString") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as i32 (string pointer)
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );

                            let symbol_id = self
                                .symbol_table
                                .lookup_symbol("float_to_string")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: float_to_string not found in symbol table, using SymbolId(167)");
                                    SymbolId(167)
                                });

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(symbol_id),
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
                        // Boolean to String conversion - call bool_to_string
                        (ConcreteType::Boolean, "toString") => {
                            let result_id = ValueId(context.function.next_value_id);
                            context.function.next_value_id += 1;

                            // Register the result as i32 (string pointer)
                            self.register_temp_local(
                                context,
                                result_id,
                                MirType::I32,
                                expression.location.clone(),
                            );

                            let symbol_id = self
                                .symbol_table
                                .lookup_symbol("bool_to_string")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: bool_to_string not found in symbol table, using SymbolId(165)");
                                    SymbolId(165)
                                });

                            let instruction = MirInstruction {
                                dest: Some(result_id),
                                operation: MirOperation::Call {
                                    function: MirOperand::Function(symbol_id),
                                    arguments: vec![MirOperand::Value(receiver_id)],
                                },
                                location: expression.location.clone(),
                            };

                            self.add_instruction(context, instruction);

                            return Ok(result_id);
                        }
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
                    // CRITICAL FIX: Use actual_type instead of TAST expr_type to handle inferred types correctly
                    let receiver_type = &receiver_actual_type;
                    match (receiver_type, method_name.as_str()) {
                        // Type conversion methods - look up correct SymbolIds from symbol table
                        (ConcreteType::Integer, "toString") => {
                            // Call int_to_string with the integer value
                            let symbol_id = self.symbol_table.lookup_symbol("int_to_string")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: int_to_string not found in symbol table, using SymbolId(166)");
                                    SymbolId(166)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Number, "toString") => {
                            // Call float_to_string with the float value
                            let symbol_id = self.symbol_table.lookup_symbol("float_to_string")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: float_to_string not found in symbol table, using SymbolId(167)");
                                    SymbolId(167)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Boolean, "toString") => {
                            // Call bool_to_string with the boolean value
                            let symbol_id = self.symbol_table.lookup_symbol("bool_to_string")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: bool_to_string not found in symbol table, using SymbolId(165)");
                                    SymbolId(165)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        // Generic type with toString() - infer from MIR type
                        (ConcreteType::Generic { .. } | ConcreteType::Unknown, "toString") => {
                            // Check the MIR type to determine which conversion function to use
                            let mir_type = context
                                .function
                                .locals
                                .get(&receiver_id)
                                .map(|l| l.local_type.clone())
                                .unwrap_or(MirType::I32);

                            eprintln!(
                                "DEBUG GENERIC toString: receiver_id={:?}, mir_type={:?}",
                                receiver_id, mir_type
                            );

                            match mir_type {
                                MirType::I32 => {
                                    // Call int_to_string
                                    let symbol_id = self.symbol_table.lookup_symbol("int_to_string")
                                        .unwrap_or_else(|| {
                                            eprintln!("WARNING: int_to_string not found in symbol table, using SymbolId(166)");
                                            SymbolId(166)
                                        });
                                    (symbol_id, vec![MirOperand::Value(receiver_id)])
                                }
                                MirType::F64 => {
                                    // Call float_to_string
                                    let symbol_id = self.symbol_table.lookup_symbol("float_to_string")
                                        .unwrap_or_else(|| {
                                            eprintln!("WARNING: float_to_string not found in symbol table, using SymbolId(167)");
                                            SymbolId(167)
                                        });
                                    (symbol_id, vec![MirOperand::Value(receiver_id)])
                                }
                                _ => {
                                    // Assume it's already a string or object with built-in toString
                                    eprintln!("WARNING: Unknown MIR type {:?} for Generic.toString(), treating as string", mir_type);
                                    (*method_symbol, vec![MirOperand::Value(receiver_id)])
                                }
                            }
                        }
                        // String methods - look up correct SymbolIds from symbol table
                        (ConcreteType::String, "length") => {
                            // Call string.length with the string value
                            let symbol_id = self.symbol_table.lookup_symbol("string.length")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: string.length not found in symbol table, using SymbolId(67)");
                                    SymbolId(67)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "toUpperCase") => {
                            // Call string.toUpperCase
                            let symbol_id = self.symbol_table.lookup_symbol("string.toUpperCase")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: string.toUpperCase not found in symbol table, using SymbolId(74)");
                                    SymbolId(74)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::String, "toLowerCase") => {
                            // Call string.toLowerCase
                            let symbol_id = self.symbol_table.lookup_symbol("string.toLowerCase")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: string.toLowerCase not found in symbol table, using SymbolId(75)");
                                    SymbolId(75)
                                });
                            (symbol_id, vec![MirOperand::Value(receiver_id)])
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
                            // Call list.size - look up from symbol table
                            let list_size_symbol = self.symbol_table.lookup_symbol("list.size")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: list.size not found in symbol table, using fallback");
                                    *method_symbol
                                });
                            (list_size_symbol, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Array(_), "add" | "push") => {
                            // Call list.push - look up from symbol table
                            let list_push_symbol = self.symbol_table.lookup_symbol("list.push")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: list.push not found in symbol table, using fallback");
                                    *method_symbol
                                });
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (list_push_symbol, args)
                        }
                        (ConcreteType::Array(_), "remove" | "pop") => {
                            // Call list_pop - look up from symbol table
                            let list_pop_symbol = self.symbol_table.lookup_symbol("list_pop")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: list_pop not found in symbol table, using fallback");
                                    *method_symbol
                                });
                            (list_pop_symbol, vec![MirOperand::Value(receiver_id)])
                        }
                        (ConcreteType::Array(_), "get") => {
                            // Call list_get - look up from symbol table
                            let list_get_symbol = self.symbol_table.lookup_symbol("list_get")
                                .unwrap_or_else(|| {
                                    eprintln!("WARNING: list_get not found in symbol table, using fallback");
                                    *method_symbol
                                });
                            let mut args = vec![MirOperand::Value(receiver_id)];
                            for arg in arguments {
                                let arg_id = self.build_expression(context, arg)?;
                                args.push(MirOperand::Value(arg_id));
                            }
                            (list_get_symbol, args)
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

                // CRITICAL FIX: For list.get and toString, infer return type from receiver instead of using Unknown
                // The typechecker returns Unknown for these methods, but we can infer from the receiver type
                let inferred_type = if method_name == "get" {
                    if let ConcreteType::Array(element_type) = &receiver.expr_type {
                        // Extract element type from Array<T> -> T
                        element_type.as_ref().clone()
                    } else {
                        expression.expr_type.clone()
                    }
                } else if method_name == "toString" {
                    // toString always returns String
                    ConcreteType::String
                } else {
                    expression.expr_type.clone()
                };

                // Convert the expression type to MIR type
                let result_type = self.convert_concrete_type(&inferred_type);

                // CRITICAL FIX: Check if this is a void method
                // Unknown types should NOT be treated as void - they represent unresolved return types
                // that likely return values. Only explicitly Null/Undefined should be treated as void.
                let is_void = !matches!(inferred_type, ConcreteType::Unknown)
                    && (matches!(inferred_type, ConcreteType::Null | ConcreteType::Undefined)
                        || matches!(result_type, MirType::Void)
                        || matches!(&result_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void)));

                eprintln!(
                    "DEBUG MIR METHODCALL: {:?} is_void={} inferred_type={:?} mir_type={:?}",
                    result_id, is_void, inferred_type, result_type
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

                // CRITICAL FIX: For namespace functions (string.*, list.*, etc), use NamedFunction operand
                // Check if this is a method call that should be converted to a namespace function call
                let function_operand = {
                    let receiver_type_name = match &receiver.expr_type {
                        ConcreteType::String => Some("string"),
                        ConcreteType::Array(_) => Some("list"),
                        ConcreteType::Integer => Some("integer"),
                        ConcreteType::Number => Some("number"),
                        ConcreteType::Boolean => Some("boolean"),
                        _ => None,
                    };

                    if let Some(type_name) = receiver_type_name {
                        // This is a method call on a known type - convert to namespace function
                        let namespace_function_name = format!("{}.{}", type_name, method_name);
                        eprintln!(
                            "DEBUG MIR METHODCALL: Creating NamedFunction '{}' for method call",
                            namespace_function_name
                        );
                        MirOperand::NamedFunction {
                            name: namespace_function_name,
                            symbol_id: function_symbol,
                        }
                    } else {
                        // Regular function call with symbol ID
                        MirOperand::Function(function_symbol)
                    }
                };

                let instruction = MirInstruction {
                    dest: dest_opt,
                    operation: MirOperation::Call {
                        function: function_operand,
                        arguments: mir_arguments,
                    },
                    location: expression.location.clone(),
                };

                self.add_instruction(context, instruction);
                Ok(result_id)
            }

            TastExpressionKind::StaticMethodCall {
                class_name,
                method_name,
                method_symbol,
                arguments,
                type_args: _,
            } => {
                // Build all arguments (NO 'this' parameter for static methods!)
                let mut mir_arguments = Vec::new();
                for arg in arguments {
                    let arg_id = self.build_expression(context, arg)?;
                    mir_arguments.push(MirOperand::Value(arg_id));
                }

                // Create result
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Get return type from expression
                let mir_return_type = self.convert_concrete_type(&expression.expr_type);

                // Register result local
                self.register_temp_local(
                    context,
                    result_id,
                    mir_return_type.clone(),
                    expression.location.clone(),
                );

                // Check if this is a namespace function (like math.pow, string.length)
                // These use SymbolId(0) and need to be looked up by name
                eprintln!("DEBUG MIR STATIC CALL: class_name='{}', method_name='{}', method_symbol=SymbolId({})",
                          class_name, method_name, method_symbol.0);
                let function_operand = if method_symbol.0 == 0 {
                    // Namespace function - use NamedFunction pattern
                    let full_name = format!("{}.{}", class_name, method_name);
                    eprintln!(
                        "DEBUG MIR STATIC CALL: Creating NamedFunction with name='{}'",
                        full_name
                    );
                    MirOperand::NamedFunction {
                        name: full_name,
                        symbol_id: *method_symbol,
                    }
                } else {
                    // Regular static method - use symbol ID directly
                    eprintln!(
                        "DEBUG MIR STATIC CALL: Creating Function(SymbolId({}))",
                        method_symbol.0
                    );
                    MirOperand::Function(*method_symbol)
                };

                // Emit Call instruction - NO 'this' parameter prepended!
                let instruction = MirInstruction {
                    dest: Some(result_id),
                    operation: MirOperation::Call {
                        function: function_operand,
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
                // Use the actual field type from the expression instead of hardcoding I32
                let field_type = self.convert_concrete_type(&expression.expr_type);
                let load_local = MirLocal {
                    name: Some(format!("field_{}", property_name)),
                    local_type: field_type,
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

            TastExpressionKind::ArrayLiteral {
                elements,
                element_type: _,
            } => {
                // CRITICAL FIX: Handle array literal creation properly
                // Array literals like [1, 2, 3] need to be materialized into actual memory
                eprintln!(
                    "DEBUG MIR ARRAYLITERAL: Creating list with {} elements",
                    elements.len()
                );

                // Strategy:
                // 1. Allocate empty list using list.allocate (synthetic SymbolId(1003))
                // 2. For each element, call list.push (synthetic SymbolId(1004)) to add it
                // 3. Return the list pointer

                // Allocate the result ValueId for the list
                let list_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the list as a Ptr(I32) local
                self.register_temp_local(
                    context,
                    list_value_id,
                    MirType::Ptr(Box::new(MirType::I32)),
                    expression.location.clone(),
                );

                // Call list.allocate(size) to create initial list
                // Use synthetic SymbolId(1003) for list.allocate
                let size_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                self.register_temp_local(
                    context,
                    size_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                // Create size constant
                let size_instruction = MirInstruction {
                    dest: Some(size_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::Integer(elements.len() as i64)),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, size_instruction);

                // Call list.allocate with synthetic SymbolId(1003)
                let alloc_instruction = MirInstruction {
                    dest: Some(list_value_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(SymbolId(1003)),
                        arguments: vec![MirOperand::Value(size_value_id)],
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, alloc_instruction);

                // Now add each element using list.push (synthetic SymbolId(1004))
                for (idx, element) in elements.iter().enumerate() {
                    eprintln!("DEBUG MIR ARRAYLITERAL: Adding element {}", idx);

                    // Build the element expression
                    let element_value_id = self.build_expression(context, element)?;

                    // CRITICAL FIX: Detect element type and use appropriate list.push function
                    // For F64 elements, use list.push_f64 (SymbolId(1005))
                    // For I32 elements, use list.push (SymbolId(1004))
                    let element_type = context
                        .function
                        .locals
                        .get(&element_value_id)
                        .map(|local| local.local_type.clone())
                        .unwrap_or(MirType::I32);

                    let push_symbol = match element_type {
                        MirType::F64 => {
                            eprintln!(
                                "DEBUG MIR ARRAYLITERAL: Element {} is F64, using list.push_f64",
                                idx
                            );
                            SymbolId(1005) // list.push_f64
                        }
                        _ => {
                            eprintln!(
                                "DEBUG MIR ARRAYLITERAL: Element {} is {:?}, using list.push",
                                idx, element_type
                            );
                            SymbolId(1004) // list.push
                        }
                    };

                    // Call list.push(list, element) or list.push_f64(list, element)
                    // Note: list.push returns the list pointer, so we need to capture it
                    let push_result_id = ValueId(context.function.next_value_id);
                    context.function.next_value_id += 1;

                    self.register_temp_local(
                        context,
                        push_result_id,
                        MirType::Ptr(Box::new(MirType::I32)),
                        expression.location.clone(),
                    );

                    let push_instruction = MirInstruction {
                        dest: Some(push_result_id),
                        operation: MirOperation::Call {
                            function: MirOperand::Function(push_symbol),
                            arguments: vec![
                                MirOperand::Value(list_value_id),
                                MirOperand::Value(element_value_id),
                            ],
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, push_instruction);

                    // Update list_value_id to point to the result of push
                    // (list.push returns the updated list pointer)
                    // We need to copy this back to list_value_id for the next iteration
                    let copy_instruction = MirInstruction {
                        dest: Some(list_value_id),
                        operation: MirOperation::Copy {
                            source: MirOperand::Value(push_result_id),
                        },
                        location: expression.location.clone(),
                    };
                    self.add_instruction(context, copy_instruction);
                }

                eprintln!(
                    "DEBUG MIR ARRAYLITERAL: Array literal created, returning ValueId({:?})",
                    list_value_id
                );
                Ok(list_value_id)
            }

            TastExpressionKind::Range {
                start,
                end,
                inclusive,
            } => {
                // Generate a range as an array of integers from start to end
                eprintln!(
                    "DEBUG MIR RANGE: Creating range array (inclusive: {})",
                    inclusive
                );

                // Evaluate start and end expressions
                let start_value_id = self.build_expression(context, start)?;
                let end_value_id = self.build_expression(context, end)?;

                // Calculate size: end - start + (1 if inclusive else 0)
                let size_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    size_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                // Subtract: size = end - start
                let diff_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    diff_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                let sub_instruction = MirInstruction {
                    dest: Some(diff_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Sub,
                        left: MirOperand::Value(end_value_id),
                        right: MirOperand::Value(start_value_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, sub_instruction);

                // Add 1 if inclusive: size = diff + 1
                let adjustment = if *inclusive { 1 } else { 0 };
                let adjustment_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    adjustment_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                let const_instruction = MirInstruction {
                    dest: Some(adjustment_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::Integer(adjustment)),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, const_instruction);

                let add_instruction = MirInstruction {
                    dest: Some(size_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Add,
                        left: MirOperand::Value(diff_value_id),
                        right: MirOperand::Value(adjustment_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, add_instruction);

                // Allocate the list using list.allocate (SymbolId(1003))
                let list_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    list_value_id,
                    MirType::Ptr(Box::new(MirType::I32)),
                    expression.location.clone(),
                );

                let alloc_instruction = MirInstruction {
                    dest: Some(list_value_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(SymbolId(1003)),
                        arguments: vec![MirOperand::Value(size_value_id)],
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, alloc_instruction);

                // Now populate the list with values from start to end
                // Use a counter variable to track current value
                let counter_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    counter_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                // Initialize counter to start value
                let init_instruction = MirInstruction {
                    dest: Some(counter_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Value(start_value_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, init_instruction);

                // Create loop blocks
                let loop_header = BasicBlockId(context.function.blocks.len());
                let loop_body = BasicBlockId(context.function.blocks.len() + 1);
                let loop_increment = BasicBlockId(context.function.blocks.len() + 2);
                let loop_exit = BasicBlockId(context.function.blocks.len() + 3);

                // Jump to loop header
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: loop_header,
                    },
                );

                // Loop header: check if counter <= end (or < end if not inclusive)
                context.function.blocks.insert(
                    loop_header,
                    MirBasicBlock {
                        id: loop_header,
                        label: Some("range_header".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: expression.location.clone(),
                    },
                );
                self.current_block = Some(loop_header);

                let condition_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    condition_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                let comparison_op = if *inclusive {
                    MirBinaryOp::Le
                } else {
                    MirBinaryOp::Lt
                };

                let cmp_instruction = MirInstruction {
                    dest: Some(condition_value_id),
                    operation: MirOperation::BinaryOp {
                        op: comparison_op,
                        left: MirOperand::Value(counter_value_id),
                        right: MirOperand::Value(end_value_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, cmp_instruction);

                self.set_block_terminator(
                    context,
                    MirTerminator::Branch {
                        condition: MirOperand::Value(condition_value_id),
                        true_block: loop_body,
                        false_block: loop_exit,
                    },
                );

                // Loop body: push counter value to list
                context.function.blocks.insert(
                    loop_body,
                    MirBasicBlock {
                        id: loop_body,
                        label: Some("range_body".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: expression.location.clone(),
                    },
                );
                self.current_block = Some(loop_body);

                let push_result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    push_result_id,
                    MirType::Ptr(Box::new(MirType::I32)),
                    expression.location.clone(),
                );

                let push_instruction = MirInstruction {
                    dest: Some(push_result_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(SymbolId(1004)), // list.push
                        arguments: vec![
                            MirOperand::Value(list_value_id),
                            MirOperand::Value(counter_value_id),
                        ],
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, push_instruction);

                // Update list pointer (list.push returns updated list)
                let copy_instruction = MirInstruction {
                    dest: Some(list_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Value(push_result_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, copy_instruction);

                // Jump to increment block
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: loop_increment,
                    },
                );

                // Increment block: counter = counter + 1
                context.function.blocks.insert(
                    loop_increment,
                    MirBasicBlock {
                        id: loop_increment,
                        label: Some("range_increment".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: expression.location.clone(),
                    },
                );
                self.current_block = Some(loop_increment);

                let one_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(
                    context,
                    one_value_id,
                    MirType::I32,
                    expression.location.clone(),
                );

                let one_instruction = MirInstruction {
                    dest: Some(one_value_id),
                    operation: MirOperation::Copy {
                        source: MirOperand::Constant(MirConstant::Integer(1)),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, one_instruction);

                let inc_instruction = MirInstruction {
                    dest: Some(counter_value_id),
                    operation: MirOperation::BinaryOp {
                        op: MirBinaryOp::Add,
                        left: MirOperand::Value(counter_value_id),
                        right: MirOperand::Value(one_value_id),
                    },
                    location: expression.location.clone(),
                };
                self.add_instruction(context, inc_instruction);

                // Jump back to loop header
                self.set_block_terminator(
                    context,
                    MirTerminator::Jump {
                        target: loop_header,
                    },
                );

                // Loop exit: continue with the list
                context.function.blocks.insert(
                    loop_exit,
                    MirBasicBlock {
                        id: loop_exit,
                        label: Some("range_exit".to_string()),
                        instructions: Vec::new(),
                        terminator: MirTerminator::Unreachable,
                        predecessors: HashSet::new(),
                        successors: HashSet::new(),
                        location: expression.location.clone(),
                    },
                );
                self.current_block = Some(loop_exit);

                eprintln!("DEBUG MIR RANGE: Range array created");
                Ok(list_value_id)
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

    /// Build optimized range-based for loop
    /// Generates a loop that iterates directly from start to end without creating an intermediate array
    fn build_range_for_loop(
        &mut self,
        context: &mut FunctionBuildContext,
        iterator_name: &str,
        start: &TastExpression,
        end: &TastExpression,
        inclusive: bool,
        body: &TastBlock,
        location: &SourceLocation,
    ) -> Result<(), Vec<CompilerError>> {
        eprintln!(
            "DEBUG RANGE FOR: Building optimized range for loop: {} in {:?}..{:?}{}",
            iterator_name,
            start.kind,
            end.kind,
            if inclusive { "=" } else { "" }
        );

        // Evaluate start and end expressions
        let start_value_id = self.build_expression(context, start)?;
        let end_value_id = self.build_expression(context, end)?;

        // Create loop blocks with dedicated increment block
        // CRITICAL: Only calculate header and body IDs now
        // increment and exit IDs will be calculated AFTER processing body
        // because body may create additional blocks (e.g., for IF statements)
        let header_block_id = BasicBlockId(context.function.blocks.len());
        let body_block_id = BasicBlockId(context.function.blocks.len() + 1);

        // Create iterator variable (this is the loop counter, starts at start value)
        let iterator_value_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;

        // Register iterator as local
        self.register_temp_local(context, iterator_value_id, MirType::I32, location.clone());

        // Initialize iterator to start value
        let init_instruction = MirInstruction {
            dest: Some(iterator_value_id),
            operation: MirOperation::Copy {
                source: MirOperand::Value(start_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, init_instruction);

        // Add iterator variable to scope so it's accessible in the loop body
        if let Some(current_scope) = context.scope_stack.last_mut() {
            current_scope.insert(iterator_name.to_string(), iterator_value_id);
        }

        // Create local for iterator variable
        let iterator_local = MirLocal {
            name: Some(iterator_name.to_string()),
            local_type: MirType::I32,
            is_mutable: true, // Mutable because it gets incremented
            location: location.clone(),
        };
        context
            .function
            .locals
            .insert(iterator_value_id, iterator_local);

        // Save init block ID for Phi node
        let init_block_id = self
            .current_block
            .expect("No current block for range loop init");

        // Create current_iterator for SSA Phi node
        let current_iterator_value_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(
            context,
            current_iterator_value_id,
            MirType::I32,
            location.clone(),
        );

        // Set current_iterator to initial value in init block
        let init_current_instruction = MirInstruction {
            dest: Some(current_iterator_value_id),
            operation: MirOperation::Copy {
                source: MirOperand::Value(iterator_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, init_current_instruction);

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
                label: Some("range_for_header".to_string()),
                instructions: Vec::new(),
                terminator: MirTerminator::Unreachable,
                predecessors: HashSet::new(),
                successors: HashSet::new(),
                location: location.clone(),
            },
        );
        self.current_block = Some(header_block_id);

        // SSA Phi node to merge iterator values
        let phi_instruction = MirInstruction {
            dest: Some(current_iterator_value_id),
            operation: MirOperation::Phi {
                incoming: vec![(init_block_id, MirOperand::Value(iterator_value_id))],
            },
            location: location.clone(),
        };
        self.add_instruction(context, phi_instruction);

        // Compare: iterator <= end (or < end if not inclusive)
        let condition_value_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, condition_value_id, MirType::I32, location.clone());

        let comparison_op = if inclusive {
            MirBinaryOp::Le
        } else {
            MirBinaryOp::Lt
        };

        let compare_instruction = MirInstruction {
            dest: Some(condition_value_id),
            operation: MirOperation::BinaryOp {
                op: comparison_op,
                left: MirOperand::Value(current_iterator_value_id),
                right: MirOperand::Value(end_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, compare_instruction);

        // NOTE: We'll set the header's terminator later after calculating exit_block_id
        // For now, leave it as Unreachable

        // Create body block
        context.function.blocks.insert(
            body_block_id,
            MirBasicBlock {
                id: body_block_id,
                label: Some("range_for_body".to_string()),
                instructions: Vec::new(),
                terminator: MirTerminator::Unreachable,
                predecessors: HashSet::new(),
                successors: HashSet::new(),
                location: location.clone(),
            },
        );
        self.current_block = Some(body_block_id);

        // Push a new scope for the loop body
        context.scope_stack.push(HashMap::new());

        // Update the scope's iterator variable to point to current_iterator_value_id
        // so the body uses the Phi-merged value
        if let Some(current_scope) = context.scope_stack.last_mut() {
            current_scope.insert(iterator_name.to_string(), current_iterator_value_id);
        }

        // Build loop body
        eprintln!(
            "DEBUG RANGE FOR: Building loop body with {} statements",
            body.statements.len()
        );
        eprintln!(
            "DEBUG RANGE FOR: current_block before body = {:?}",
            self.current_block
        );
        for (idx, stmt) in body.statements.iter().enumerate() {
            eprintln!("DEBUG RANGE FOR: Processing body statement {}", idx);
            eprintln!(
                "DEBUG RANGE FOR:   current_block = {:?}",
                self.current_block
            );
            self.build_statement(context, stmt)?;
            eprintln!(
                "DEBUG RANGE FOR:   current_block after = {:?}",
                self.current_block
            );
        }
        eprintln!(
            "DEBUG RANGE FOR: current_block after all body statements = {:?}",
            self.current_block
        );

        // Pop the loop body scope
        context.scope_stack.pop();

        // CRITICAL FIX: Calculate increment and exit block IDs AFTER processing body
        // Body may have created additional blocks (e.g., IF statement blocks)
        let increment_block_id = BasicBlockId(context.function.blocks.len());
        let exit_block_id = BasicBlockId(context.function.blocks.len() + 1);
        eprintln!("DEBUG RANGE FOR: Calculated increment_block_id = {:?}, exit_block_id = {:?} (blocks.len = {})",
            increment_block_id, exit_block_id, context.function.blocks.len());

        // Now set the header block's terminator with the correct exit_block_id
        if let Some(header_block) = context.function.blocks.get_mut(&header_block_id) {
            header_block.terminator = MirTerminator::Branch {
                condition: MirOperand::Value(condition_value_id),
                true_block: body_block_id,
                false_block: exit_block_id,
            };
        }

        // Check if current block has a terminator (from return/break/continue)
        // After processing the body statements, current_block might have changed
        // (e.g., to an IF statement's continue block)
        let current_has_terminator = if let Some(current_block_id) = self.current_block {
            if let Some(current_block) = context.function.blocks.get(&current_block_id) {
                !matches!(current_block.terminator, MirTerminator::Unreachable)
            } else {
                false
            }
        } else {
            false
        };

        if !current_has_terminator {
            // Jump to increment block
            self.set_block_terminator(
                context,
                MirTerminator::Jump {
                    target: increment_block_id,
                },
            );
        } else {
            eprintln!(
                "DEBUG RANGE FOR: Current block already has terminator, skipping jump to increment"
            );
        }

        // Create increment block
        context.function.blocks.insert(
            increment_block_id,
            MirBasicBlock {
                id: increment_block_id,
                label: Some("range_for_increment".to_string()),
                instructions: Vec::new(),
                terminator: MirTerminator::Unreachable,
                predecessors: HashSet::new(),
                successors: HashSet::new(),
                location: location.clone(),
            },
        );
        self.current_block = Some(increment_block_id);

        // Increment: iterator = iterator + 1
        let one_value_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, one_value_id, MirType::I32, location.clone());

        let one_instruction = MirInstruction {
            dest: Some(one_value_id),
            operation: MirOperation::Copy {
                source: MirOperand::Constant(MirConstant::Integer(1)),
            },
            location: location.clone(),
        };
        self.add_instruction(context, one_instruction);

        let inc_instruction = MirInstruction {
            dest: Some(iterator_value_id),
            operation: MirOperation::BinaryOp {
                op: MirBinaryOp::Add,
                left: MirOperand::Value(current_iterator_value_id),
                right: MirOperand::Value(one_value_id),
            },
            location: location.clone(),
        };
        self.add_instruction(context, inc_instruction);

        // Update Phi node with increment block predecessor
        if let Some(header_block) = context.function.blocks.get_mut(&header_block_id) {
            if let Some(phi_instr) = header_block.instructions.first_mut() {
                if let MirOperation::Phi { incoming } = &mut phi_instr.operation {
                    incoming.push((increment_block_id, MirOperand::Value(iterator_value_id)));
                }
            }
        }

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
                label: Some("range_for_exit".to_string()),
                instructions: Vec::new(),
                terminator: MirTerminator::Unreachable,
                predecessors: HashSet::new(),
                successors: HashSet::new(),
                location: location.clone(),
            },
        );
        eprintln!(
            "DEBUG RANGE FOR: Setting current_block to exit_block {:?}",
            exit_block_id
        );
        self.current_block = Some(exit_block_id);

        eprintln!(
            "DEBUG RANGE FOR: Range for loop completed successfully, current_block = {:?}",
            self.current_block
        );
        Ok(())
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

    /// Convert a value to string for print() calls
    /// Returns the ValueId of the string result
    fn convert_value_to_string(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        value_type: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        use crate::typechecker::tast::ConcreteType;

        match value_type {
            ConcreteType::String => {
                // Already a string, use directly
                Ok(value_id)
            }
            ConcreteType::Integer => {
                // Convert integer to string using int_to_string
                let converted_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the converted_id in function.locals
                self.register_temp_local(context, converted_id, MirType::I32, location.clone());

                let symbol_id = self
                    .symbol_table
                    .lookup_symbol("int_to_string")
                    .unwrap_or_else(|| {
                        eprintln!(
                            "WARNING: int_to_string not found in symbol table, using SymbolId(166)"
                        );
                        SymbolId(166)
                    });

                let conversion_instruction = MirInstruction {
                    dest: Some(converted_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(symbol_id),
                        arguments: vec![MirOperand::Value(value_id)],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, conversion_instruction);
                Ok(converted_id)
            }
            ConcreteType::Number => {
                // Convert float to string using float_to_string
                let converted_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the converted_id in function.locals
                self.register_temp_local(context, converted_id, MirType::I32, location.clone());

                let symbol_id = self
                    .symbol_table
                    .lookup_symbol("float_to_string")
                    .unwrap_or_else(|| {
                        eprintln!("WARNING: float_to_string not found in symbol table, using SymbolId(167)");
                        SymbolId(167)
                    });

                let conversion_instruction = MirInstruction {
                    dest: Some(converted_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(symbol_id),
                        arguments: vec![MirOperand::Value(value_id)],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, conversion_instruction);
                Ok(converted_id)
            }
            ConcreteType::Boolean => {
                // Convert boolean to string using bool_to_string
                let converted_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Register the converted_id in function.locals
                self.register_temp_local(context, converted_id, MirType::I32, location.clone());

                let symbol_id = self
                    .symbol_table
                    .lookup_symbol("bool_to_string")
                    .unwrap_or_else(|| {
                        eprintln!("WARNING: bool_to_string not found in symbol table, using SymbolId(165)");
                        SymbolId(165)
                    });

                let conversion_instruction = MirInstruction {
                    dest: Some(converted_id),
                    operation: MirOperation::Call {
                        function: MirOperand::Function(symbol_id),
                        arguments: vec![MirOperand::Value(value_id)],
                    },
                    location: location.clone(),
                };
                self.add_instruction(context, conversion_instruction);
                Ok(converted_id)
            }
            _ => {
                // For other types (objects, arrays, etc.), use the value as-is for now
                // In a complete implementation, these would also have toString() methods
                Ok(value_id)
            }
        }
    }

    /// Convert MirType back to ConcreteType for type inference
    /// This is the inverse of MirType::from_concrete_type()
    fn mir_type_to_concrete(mir_type: &MirType) -> ConcreteType {
        match mir_type {
            MirType::I32 => ConcreteType::Integer,
            MirType::F64 => ConcreteType::Number,
            MirType::Bool => ConcreteType::Boolean,
            MirType::Void => ConcreteType::Undefined,
            MirType::Ptr(inner) => {
                match **inner {
                    MirType::I8 => ConcreteType::String,
                    MirType::Void => ConcreteType::Null,
                    _ => ConcreteType::Null, // Fallback for other pointer types
                }
            }
            MirType::StringTuple => ConcreteType::String,
            MirType::Function {
                parameters,
                return_type,
            } => ConcreteType::Function {
                parameters: parameters.iter().map(Self::mir_type_to_concrete).collect(),
                return_type: Box::new(Self::mir_type_to_concrete(return_type)),
                is_async: false,
            },
            // For types that can't be precisely converted back, use safe defaults
            MirType::I8 | MirType::I16 | MirType::I64 => ConcreteType::Integer,
            MirType::U8 | MirType::U16 | MirType::U32 | MirType::U64 => ConcreteType::Integer,
            MirType::F32 => ConcreteType::Number,
            MirType::Array(_, _) => ConcreteType::Array(Box::new(ConcreteType::Integer)),
            MirType::Struct(_) => ConcreteType::Null,
        }
    }

    /// Infer the result type of a binary operation
    fn infer_binary_operation_type(
        &self,
        left_type: &ConcreteType,
        right_type: &ConcreteType,
        operator: &BinaryOperator,
    ) -> MirType {
        // DEBUG: Log type inference for binary operations
        eprintln!(
            "DEBUG TYPE INFER: Binary {:?}: left={:?}, right={:?}",
            operator, left_type, right_type
        );

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
                eprintln!("DEBUG TYPE INFER: Comparison/logical op -> I32");
                return MirType::I32; // Boolean result
            }
            _ => {}
        }

        // For arithmetic and other operations, infer from operand types
        let result = match (left_type, right_type) {
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
        };
        eprintln!("DEBUG TYPE INFER: Result type -> {:?}", result);
        result
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
        // DEBUG: Track GetElementPtr/Load instructions
        if matches!(instruction.operation, MirOperation::GetElementPtr { .. }) {
            eprintln!(
                "DEBUG ADD_INSTR: Adding GetElementPtr to block {:?}",
                self.current_block
            );
        }
        if matches!(instruction.operation, MirOperation::Load { .. })
            && self.current_block == Some(BasicBlockId(2))
        {
            eprintln!(
                "DEBUG ADD_INSTR: Adding Load to body block {:?}",
                self.current_block
            );
        }

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
                if block_id == BasicBlockId(3) && context.function.name == "test" {
                    eprintln!(
                        "DEBUG SET_TERM BasicBlockId(3): old={:?}, new={:?}",
                        block.terminator, terminator
                    );
                }
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
