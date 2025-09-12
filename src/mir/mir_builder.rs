//! MIR Builder - Lowering from TAST to MIR
//!
//! This module implements the transformation from TAST (Typed Abstract Syntax Tree)
//! to MIR (Medium-level Intermediate Representation). The builder converts high-level
//! typed constructs into optimization-friendly SSA form with explicit control flow.

use crate::typechecker::tast::{
    TastProgram, TastFunction, TastClass, TastExpression, TastStatement, TastBlock,
    ConcreteType, TastExpressionKind, TastLiteral, BinaryOperator
};
use crate::mir::mir_types::{
    MirProgram, MirFunction, MirBasicBlock, MirInstruction, MirOperation, MirTerminator,
    MirOperand, MirConstant, MirType, MirParameter, MirLocal, MirGlobal,
    MirBinaryOp, MirUnaryOp, MirFunctionAttributes, MirDebugInfo,
    BasicBlockId, ValueId, RegisterId
};
use crate::resolver::SymbolId;
use crate::ast::{SourceLocation, Value};
use crate::error::CompilerError;
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
        }
    }
    
    /// Build MIR from TAST program
    pub fn build(&mut self, tast: TastProgram) -> Result<MirBuildResult, Vec<CompilerError>> {
        let start_time = std::time::Instant::now();
        
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
                    mir_program.functions.insert(mir_function.symbol_id, mir_function);
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
        let critical_errors: Vec<_> = self.warnings.iter()
            .filter(|e| matches!(e, CompilerError::Type { .. } | CompilerError::Validation { .. }))
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
    fn build_function(&mut self, tast_function: TastFunction) -> Result<MirFunction, Vec<CompilerError>> {
        // Create function structure
        let mut mir_function = MirFunction {
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
        };
        
        // Process parameters
        for (i, param) in tast_function.parameters.iter().enumerate() {
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
    fn build_class(&mut self, tast_class: TastClass) -> Result<Vec<MirFunction>, Vec<CompilerError>> {
        let mut functions = Vec::new();
        let mut errors = Vec::new();
        
        // Build all constructors
        for constructor in tast_class.constructors {
            match self.build_function(constructor) {
                Ok(ctor_function) => functions.push(ctor_function),
                Err(ctor_errors) => errors.extend(ctor_errors),
            }
        }
        
        // Build all methods
        for method in tast_class.methods {
            match self.build_function(method) {
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
    fn build_block(&mut self, context: &mut FunctionBuildContext, block: &TastBlock) -> Result<(), Vec<CompilerError>> {
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
    fn build_statement(&mut self, context: &mut FunctionBuildContext, statement: &TastStatement) -> Result<(), Vec<CompilerError>> {
        match statement {
            TastStatement::VariableDeclaration { symbol_id, name, var_type, initializer, is_mutable, location } => {
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
            
            TastStatement::Assignment { target, value, location } => {
                // Build value expression
                let value_id = self.build_expression(context, value)?;
                
                // Handle assignment target - for now, only support simple variable assignments
                match &target.kind {
                    TastExpressionKind::Variable { symbol_id, name } => {
                        // Update variable in current scope
                        if let Some(current_scope) = context.scope_stack.last_mut() {
                            current_scope.insert(name.clone(), value_id);
                        }
                    }
                    _ => {
                        // TODO: Handle field access and array index assignments
                        return Err(vec![CompilerError::validation_error(
                            "Complex assignment targets not yet implemented",
                            location.clone(),
                        )]);
                    }
                }
            }
            
            TastStatement::Expression { expression, .. } => {
                // Build expression for side effects
                self.build_expression(context, expression)?;
            }
            
            TastStatement::Return { value, return_type: _, location: _ } => {
                // Return type validation already done in type checking phase
                let return_value = if let Some(expr) = value {
                    Some(MirOperand::Value(self.build_expression(context, expr)?))
                } else {
                    None
                };
                
                // Create return terminator
                let terminator = MirTerminator::Return { value: return_value };
                self.set_block_terminator(context, terminator);
            }
            
            TastStatement::Print { expression, newline: _, location } => {
                // Build the expression to print
                let value_id = self.build_expression(context, expression)?;
                
                // For now, treat print as a built-in operation
                // TODO: Implement proper function calls to built-in functions
                let instruction = MirInstruction {
                    dest: None, // Print doesn't return a value
                    operation: MirOperation::Copy {
                        source: MirOperand::Value(value_id), // Just copy the value for now
                    },
                    location: location.clone(),
                };
                
                self.add_instruction(context, instruction);
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
    fn build_expression(&mut self, context: &mut FunctionBuildContext, expression: &TastExpression) -> Result<ValueId, Vec<CompilerError>> {
        match &expression.kind {
            TastExpressionKind::Literal { value } => {
                let constant = self.convert_literal(value);
                let value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                
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
            
            TastExpressionKind::Variable { symbol_id, name } => {
                // Look up variable in scope stack
                for scope in context.scope_stack.iter().rev() {
                    if let Some(&value_id) = scope.get(name) {
                        return Ok(value_id);
                    }
                }
                
                Err(vec![CompilerError::type_error(
                    &format!("Undefined variable: {}", name),
                    None,
                    Some(expression.location.clone()),
                )])
            }
            
            TastExpressionKind::BinaryOperation { operator, left, right } => {
                let left_id = self.build_expression(context, left)?;
                let right_id = self.build_expression(context, right)?;
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                
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
            
            TastExpressionKind::FunctionCall { function, arguments, type_args: _ } => {
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
    
    /// Convert TAST binary operator to MIR binary operator
    fn convert_binary_op(&self, op: &BinaryOperator) -> MirBinaryOp {
        match op {
            BinaryOperator::Add => MirBinaryOp::Add,
            BinaryOperator::Subtract => MirBinaryOp::Sub,
            BinaryOperator::Multiply => MirBinaryOp::Mul,
            BinaryOperator::Divide => MirBinaryOp::Div,
            BinaryOperator::Modulo => MirBinaryOp::Rem,
            BinaryOperator::Equal => MirBinaryOp::Eq,
            BinaryOperator::NotEqual => MirBinaryOp::Ne,
            BinaryOperator::LessThan => MirBinaryOp::Lt,
            BinaryOperator::GreaterThan => MirBinaryOp::Gt,
            BinaryOperator::LessThanOrEqual => MirBinaryOp::Le,
            BinaryOperator::GreaterThanOrEqual => MirBinaryOp::Ge,
            BinaryOperator::And => MirBinaryOp::And,
            BinaryOperator::Or => MirBinaryOp::Or,
            BinaryOperator::Power => MirBinaryOp::Mul, // TODO: Implement proper power operation
            BinaryOperator::BitwiseAnd => MirBinaryOp::And, // TODO: Distinguish from logical and
            BinaryOperator::BitwiseOr => MirBinaryOp::Or, // TODO: Distinguish from logical or
            BinaryOperator::BitwiseXor => MirBinaryOp::Add, // TODO: Implement XOR
            BinaryOperator::LeftShift => MirBinaryOp::Mul, // TODO: Implement left shift
            BinaryOperator::RightShift => MirBinaryOp::Div, // TODO: Implement right shift
            BinaryOperator::Concatenate => MirBinaryOp::Add, // TODO: Implement string concat
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
    fn set_block_terminator(&mut self, context: &mut FunctionBuildContext, terminator: MirTerminator) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = context.function.blocks.get_mut(&block_id) {
                block.terminator = terminator;
            }
        }
    }
    
    /// Ensure function has proper termination
    fn ensure_function_termination(&mut self, context: &mut FunctionBuildContext, return_type: &ConcreteType) -> Result<(), Vec<CompilerError>> {
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
                    
                    block.terminator = MirTerminator::Return { value: return_value };
                }
            }
        }
        
        Ok(())
    }
    
    /// Resolve pending phi nodes
    fn resolve_phi_nodes(&mut self, context: &mut FunctionBuildContext) -> Result<(), Vec<CompilerError>> {
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