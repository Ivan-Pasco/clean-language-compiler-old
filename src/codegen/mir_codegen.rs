//! MIR to WebAssembly Code Generation
//!
//! This module implements code generation from MIR (Medium-level Intermediate Representation)
//! to WebAssembly bytecode. It provides a cleaner, more optimized path from typed code
//! to WASM compared to the direct AST-to-WASM generation.

use crate::mir::mir_types::{
    MirProgram, MirFunction, MirBasicBlock, MirInstruction, MirOperation, MirTerminator,
    MirOperand, MirConstant, MirType, MirBinaryOp, MirUnaryOp,
    BasicBlockId, ValueId
};
use crate::error::CompilerError;
use crate::codegen::CodeGenerator;
use wasm_encoder::{
    ValType, Instruction, Function as WasmFunction, BlockType
};
use std::collections::HashMap;

/// MIR to WASM code generator
pub struct MirCodeGenerator<'a> {
    /// The underlying WASM code generator
    wasm_generator: CodeGenerator,
    
    /// Mapping from MIR ValueId to WASM local indices
    value_to_local: HashMap<ValueId, u32>,
    
    /// Mapping from MIR BasicBlockId to WASM block indices
    block_labels: HashMap<BasicBlockId, u32>,
    
    /// Current WASM local index counter
    next_local_index: u32,
    
    /// Current WASM block label counter
    next_block_label: u32,
    
    /// Stack of WASM instructions for current function
    current_instructions: Vec<Instruction<'a>>,
    
    /// Current function being generated
    current_function: Option<MirFunction>,
}

/// Result of MIR code generation
#[derive(Debug)]
pub struct MirCodegenResult {
    /// Generated WASM bytecode
    pub wasm_bytes: Vec<u8>,
    
    /// Generation statistics
    pub stats: MirCodegenStats,
    
    /// Warnings during generation
    pub warnings: Vec<CompilerError>,
}

/// Statistics about MIR code generation
#[derive(Debug, Default)]
pub struct MirCodegenStats {
    /// Number of functions generated
    pub functions_generated: usize,
    
    /// Number of basic blocks generated
    pub blocks_generated: usize,
    
    /// Number of instructions generated
    pub instructions_generated: usize,
    
    /// Generation time in microseconds
    pub generation_time_us: u64,
}

impl<'a> MirCodeGenerator<'a> {
    /// Create a new MIR code generator
    pub fn new() -> Self {
        Self {
            wasm_generator: CodeGenerator::new(),
            value_to_local: HashMap::new(),
            block_labels: HashMap::new(),
            next_local_index: 0,
            next_block_label: 0,
            current_instructions: Vec::new(),
            current_function: None,
        }
    }
    
    /// Create a new MIR code generator for testing (without runtime imports)
    pub fn new_minimal() -> Self {
        Self {
            wasm_generator: CodeGenerator::new_minimal(),
            value_to_local: HashMap::new(),
            block_labels: HashMap::new(),
            next_local_index: 0,
            next_block_label: 0,
            current_instructions: Vec::new(),
            current_function: None,
        }
    }
    
    /// Generate WASM from MIR program
    pub fn generate(&mut self, mir_program: MirProgram) -> Result<MirCodegenResult, Vec<CompilerError>> {
        let start_time = std::time::Instant::now();
        let mut stats = MirCodegenStats::default();
        let mut warnings = Vec::new();
        
        // Set up memory section
        self.setup_memory_section().map_err(|e| vec![e])?;
        
        // Generate all functions
        for (symbol_id, function) in mir_program.functions {
            match self.generate_function(function) {
                Ok(function_stats) => {
                    stats.functions_generated += 1;
                    stats.blocks_generated += function_stats.blocks_generated;
                    stats.instructions_generated += function_stats.instructions_generated;
                }
                Err(error) => {
                    warnings.push(error);
                }
            }
        }
        
        // Transfer string pool to WASM module
        self.setup_string_pool(&mir_program.string_pool).map_err(|e| vec![e])?;
        
        // Finalize WASM module
        let wasm_bytes = self.finalize_module().map_err(|e| vec![e])?;
        
        stats.generation_time_us = start_time.elapsed().as_micros() as u64;
        
        Ok(MirCodegenResult {
            wasm_bytes,
            stats,
            warnings,
        })
    }
    
    /// Generate WASM function from MIR function
    fn generate_function(&mut self, function: MirFunction) -> Result<FunctionStats, CompilerError> {
        let mut stats = FunctionStats::default();
        
        // Reset per-function state
        self.value_to_local.clear();
        self.block_labels.clear();
        self.next_local_index = 0;
        self.next_block_label = 0;
        self.current_instructions.clear();
        self.current_function = Some(function.clone());
        
        // Convert MIR function signature to WASM
        let wasm_signature = self.convert_function_signature(&function)?;
        
        // Allocate locals for function parameters
        for param in &function.parameters {
            let local_index = self.next_local_index;
            self.value_to_local.insert(param.value_id, local_index);
            self.next_local_index += 1;
        }
        
        // Allocate locals for function local variables
        for (value_id, local) in &function.locals {
            let local_index = self.next_local_index;
            self.value_to_local.insert(*value_id, local_index);
            self.next_local_index += 1;
        }
        
        // Pre-assign block labels
        for &block_id in function.blocks.keys() {
            self.block_labels.insert(block_id, self.next_block_label);
            self.next_block_label += 1;
        }
        
        // Generate basic blocks in post-order for proper control flow
        let block_order = self.compute_block_order(&function);
        
        for block_id in block_order {
            if let Some(block) = function.blocks.get(&block_id) {
                self.generate_basic_block(block)?;
                stats.blocks_generated += 1;
                stats.instructions_generated += block.instructions.len();
            }
        }
        
        // Create WASM function with generated instructions
        let local_types = self.compute_local_types(&function);
        let mut wasm_function = WasmFunction::new(local_types);
        for instruction in &self.current_instructions {
            wasm_function.instruction(instruction);
        }
        
        // Add function to WASM module
        self.add_function_to_module(function.name.clone(), wasm_function, wasm_signature)?;
        
        Ok(stats)
    }
    
    /// Generate WASM instructions for a basic block
    fn generate_basic_block(&mut self, block: &MirBasicBlock) -> Result<(), CompilerError> {
        // Start block if it has predecessors (not entry block)
        if !block.predecessors.is_empty() {
            if let Some(&label) = self.block_labels.get(&block.id) {
                self.current_instructions.push(Instruction::Block(BlockType::Empty));
            }
        }
        
        // Generate instructions
        for instruction in &block.instructions {
            self.generate_instruction(instruction)?;
        }
        
        // Generate terminator
        self.generate_terminator(&block.terminator)?;
        
        // End block if it was started
        if !block.predecessors.is_empty() {
            self.current_instructions.push(Instruction::End);
        }
        
        Ok(())
    }
    
    /// Generate WASM instruction from MIR instruction
    fn generate_instruction(&mut self, instruction: &MirInstruction) -> Result<(), CompilerError> {
        match &instruction.operation {
            MirOperation::Copy { source } => {
                // Load source operand and store to destination
                self.load_operand(source)?;
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                }
            }
            
            MirOperation::BinaryOp { op, left, right } => {
                // Load operands and perform operation
                self.load_operand(left)?;
                self.load_operand(right)?;
                self.generate_binary_operation(op)?;
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                }
            }
            
            MirOperation::UnaryOp { op, operand } => {
                // Load operand and perform operation
                self.load_operand(operand)?;
                self.generate_unary_operation(op)?;
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                }
            }
            
            MirOperation::Load { source } => {
                // Load from memory
                self.load_operand(source)?;
                // Add memory load instruction based on type
                self.current_instructions.push(Instruction::I32Load(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                }
            }
            
            MirOperation::Store { destination, value } => {
                // Store to memory
                self.load_operand(destination)?;
                self.load_operand(value)?;
                self.current_instructions.push(Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
            }
            
            MirOperation::Call { function, arguments } => {
                // Load arguments
                for arg in arguments {
                    self.load_operand(arg)?;
                }
                
                // Generate function call
                match function {
                    MirOperand::Function(symbol_id) => {
                        // TODO: Map symbol_id to function index
                        // For now, generate a placeholder call
                        self.current_instructions.push(Instruction::Call(0));
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
                
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                }
            }
            
            _ => {
                // TODO: Implement other operation types
                return Err(CompilerError::Codegen {
                    context: Box::new(crate::error::ErrorContext::new(
                        format!("MIR operation not yet implemented: {:?}", instruction.operation),
                        None,
                        crate::error::ErrorType::Codegen,
                        Some(instruction.location.clone()),
                    )),
                });
            }
        }
        
        Ok(())
    }
    
    /// Generate WASM terminator instruction
    fn generate_terminator(&mut self, terminator: &MirTerminator) -> Result<(), CompilerError> {
        match terminator {
            MirTerminator::Return { value } => {
                if let Some(return_value) = value {
                    self.load_operand(return_value)?;
                }
                self.current_instructions.push(Instruction::Return);
            }
            
            MirTerminator::Jump { target } => {
                if let Some(&label) = self.block_labels.get(target) {
                    self.current_instructions.push(Instruction::Br(label));
                }
            }
            
            MirTerminator::Branch { condition, true_block, false_block } => {
                // Load condition
                self.load_operand(condition)?;
                
                // Branch based on condition
                if let (Some(&true_label), Some(&false_label)) = 
                    (self.block_labels.get(true_block), self.block_labels.get(false_block)) {
                    self.current_instructions.push(Instruction::BrIf(true_label));
                    self.current_instructions.push(Instruction::Br(false_label));
                }
            }
            
            MirTerminator::Unreachable => {
                self.current_instructions.push(Instruction::Unreachable);
            }
        }
        
        Ok(())
    }
    
    /// Load MIR operand onto WASM stack
    fn load_operand(&mut self, operand: &MirOperand) -> Result<(), CompilerError> {
        match operand {
            MirOperand::Value(value_id) => {
                if let Some(&local_index) = self.value_to_local.get(value_id) {
                    self.current_instructions.push(Instruction::LocalGet(local_index));
                } else {
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            format!("Undefined value: {:?}", value_id),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(crate::ast::SourceLocation::default()),
                        )),
                    });
                }
            }
            
            MirOperand::Constant(constant) => {
                self.load_constant(constant)?;
            }
            
            MirOperand::Function(symbol_id) => {
                // TODO: Load function reference
                // For now, just load the function index as a constant
                self.current_instructions.push(Instruction::I32Const(0));
            }
            
            MirOperand::Global(symbol_id) => {
                // TODO: Load global variable
                // For now, load from a placeholder global
                self.current_instructions.push(Instruction::GlobalGet(0));
            }
        }
        
        Ok(())
    }
    
    /// Load MIR constant onto WASM stack
    fn load_constant(&mut self, constant: &MirConstant) -> Result<(), CompilerError> {
        match constant {
            MirConstant::Integer(i) => {
                self.current_instructions.push(Instruction::I64Const(*i));
            }
            MirConstant::Float(f) => {
                self.current_instructions.push(Instruction::F64Const(*f));
            }
            MirConstant::Boolean(b) => {
                self.current_instructions.push(Instruction::I32Const(if *b { 1 } else { 0 }));
            }
            MirConstant::String(index) => {
                // Load string as pointer to string pool
                // TODO: Implement proper string loading
                self.current_instructions.push(Instruction::I32Const(*index as i32));
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
    
    /// Store value from WASM stack to local
    fn store_to_local(&mut self, value_id: ValueId) -> Result<(), CompilerError> {
        if let Some(&local_index) = self.value_to_local.get(&value_id) {
            self.current_instructions.push(Instruction::LocalSet(local_index));
            Ok(())
        } else {
            Err(CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    format!("Cannot store to undefined value: {:?}", value_id),
                    None,
                    crate::error::ErrorType::Codegen,
                    Some(crate::ast::SourceLocation::default()),
                )),
            })
        }
    }
    
    /// Generate WASM binary operation
    fn generate_binary_operation(&mut self, op: &MirBinaryOp) -> Result<(), CompilerError> {
        let instruction = match op {
            // Arithmetic operations (assume i64 for now)
            MirBinaryOp::Add => Instruction::I64Add,
            MirBinaryOp::Sub => Instruction::I64Sub,
            MirBinaryOp::Mul => Instruction::I64Mul,
            MirBinaryOp::Div => Instruction::I64DivS,
            MirBinaryOp::Rem => Instruction::I64RemS,
            
            // Comparison operations
            MirBinaryOp::Eq => Instruction::I64Eq,
            MirBinaryOp::Ne => Instruction::I64Ne,
            MirBinaryOp::Lt => Instruction::I64LtS,
            MirBinaryOp::Le => Instruction::I64LeS,
            MirBinaryOp::Gt => Instruction::I64GtS,
            MirBinaryOp::Ge => Instruction::I64GeS,
            
            // Bitwise operations
            MirBinaryOp::And => Instruction::I64And,
            MirBinaryOp::Or => Instruction::I64Or,
            MirBinaryOp::Xor => Instruction::I64Xor,
            MirBinaryOp::Shl => Instruction::I64Shl,
            MirBinaryOp::Shr => Instruction::I64ShrS,
        };
        
        self.current_instructions.push(instruction);
        Ok(())
    }
    
    /// Generate WASM unary operation
    fn generate_unary_operation(&mut self, op: &MirUnaryOp) -> Result<(), CompilerError> {
        match op {
            MirUnaryOp::Neg => {
                // Negate: 0 - x
                self.current_instructions.push(Instruction::I64Const(0));
                // Swap the order so we have: 0, x
                // Then subtract: 0 - x
                self.current_instructions.push(Instruction::I64Sub);
            }
            MirUnaryOp::Not => {
                // Logical not: x == 0
                self.current_instructions.push(Instruction::I64Const(0));
                self.current_instructions.push(Instruction::I64Eq);
            }
            MirUnaryOp::BitNot => {
                // Bitwise not: x ^ -1
                self.current_instructions.push(Instruction::I64Const(-1));
                self.current_instructions.push(Instruction::I64Xor);
            }
        }
        
        Ok(())
    }
    
    /// Convert MIR function signature to WASM types
    fn convert_function_signature(&self, function: &MirFunction) -> Result<(Vec<ValType>, Vec<ValType>), CompilerError> {
        let mut param_types = Vec::new();
        let mut result_types = Vec::new();
        
        // Convert parameter types
        for param in &function.parameters {
            param_types.push(self.mir_type_to_wasm_type(&param.param_type)?);
        }
        
        // Convert return type
        if !matches!(function.return_type, MirType::Void) {
            result_types.push(self.mir_type_to_wasm_type(&function.return_type)?);
        }
        
        Ok((param_types, result_types))
    }
    
    /// Convert MIR type to WASM ValType
    fn mir_type_to_wasm_type(&self, mir_type: &MirType) -> Result<ValType, CompilerError> {
        match mir_type {
            MirType::I8 | MirType::I16 | MirType::I32 | 
            MirType::U8 | MirType::U16 | MirType::U32 | 
            MirType::Bool => Ok(ValType::I32),
            
            MirType::I64 | MirType::U64 => Ok(ValType::I64),
            
            MirType::F32 => Ok(ValType::F32),
            MirType::F64 => Ok(ValType::F64),
            
            MirType::Ptr(_) => Ok(ValType::I32), // Pointers are 32-bit addresses
            
            _ => Err(CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    format!("Cannot convert MIR type to WASM: {:?}", mir_type),
                    None,
                    crate::error::ErrorType::Codegen,
                    Some(crate::ast::SourceLocation::default()),
                )),
            })
        }
    }
    
    /// Compute local variable types for WASM function
    fn compute_local_types(&self, function: &MirFunction) -> Vec<(u32, ValType)> {
        let mut locals = Vec::new();
        
        // Add locals for each variable
        for (value_id, local) in &function.locals {
            if let Ok(wasm_type) = self.mir_type_to_wasm_type(&local.local_type) {
                locals.push((1, wasm_type));
            }
        }
        
        locals
    }
    
    /// Compute basic block order for code generation
    fn compute_block_order(&self, function: &MirFunction) -> Vec<BasicBlockId> {
        // For now, use a simple ordering starting with entry block
        let mut order = vec![function.entry_block];
        
        for &block_id in function.blocks.keys() {
            if block_id != function.entry_block {
                order.push(block_id);
            }
        }
        
        order
    }
    
    /// Set up memory section
    fn setup_memory_section(&mut self) -> Result<(), CompilerError> {
        // TODO: Implement memory setup
        Ok(())
    }
    
    /// Set up string pool in WASM module
    fn setup_string_pool(&mut self, string_pool: &[String]) -> Result<(), CompilerError> {
        // TODO: Implement string pool setup
        Ok(())
    }
    
    /// Add function to WASM module
    fn add_function_to_module(
        &mut self, 
        name: String, 
        function: WasmFunction, 
        signature: (Vec<ValType>, Vec<ValType>)
    ) -> Result<(), CompilerError> {
        // TODO: Implement function addition to module
        Ok(())
    }
    
    /// Finalize WASM module and return bytecode
    fn finalize_module(&mut self) -> Result<Vec<u8>, CompilerError> {
        // TODO: Implement module finalization
        // For now, return empty WASM module
        Ok(vec![
            0x00, 0x61, 0x73, 0x6d, // WASM magic
            0x01, 0x00, 0x00, 0x00, // WASM version
        ])
    }
}

/// Statistics for function generation
#[derive(Debug, Default)]
struct FunctionStats {
    blocks_generated: usize,
    instructions_generated: usize,
}

impl<'a> Default for MirCodeGenerator<'a> {
    fn default() -> Self {
        Self::new()
    }
}