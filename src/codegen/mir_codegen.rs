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
use crate::resolver::SymbolId;
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

    /// String pool from MIR program for string constant handling
    string_pool: Option<Vec<String>>,

    /// Mapping from ValueId to string pool index (for string constants loaded as locals)
    value_to_string_index: HashMap<ValueId, usize>,
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
            string_pool: None,
            value_to_string_index: HashMap::new(),
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
            string_pool: None,
            value_to_string_index: HashMap::new(),
        }
    }
    
    /// Generate WASM from MIR program
    pub fn generate(&mut self, mir_program: MirProgram) -> Result<MirCodegenResult, Vec<CompilerError>> {
        println!("DEBUG MIR: MirCodeGenerator::generate called with {} functions", mir_program.functions.len());
        for (symbol_id, function) in &mir_program.functions {
            println!("DEBUG MIR: Function {} '{}' has {} basic blocks", symbol_id.0, function.name, function.blocks.len());
        }

        let start_time = std::time::Instant::now();
        let mut stats = MirCodegenStats::default();
        let mut warnings = Vec::new();

        // CRITICAL FIX: Set up the underlying WASM generator with runtime imports
        if self.wasm_generator.include_runtime_imports {
            self.wasm_generator.register_print_imports().map_err(|e| vec![e])?;
        }

        // Set up memory section
        self.setup_memory_section().map_err(|e| vec![e])?;

        // CRITICAL FIX: Transfer string pool to WASM module BEFORE function generation
        // Functions need access to string pool during code generation
        self.setup_string_pool(&mir_program.string_pool).map_err(|e| vec![e])?;

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

        // CRITICAL FIX: Handle entry point if it exists
        if let Some(entry_symbol_id) = mir_program.entry_point {
            self.generate_start_function_export(entry_symbol_id).map_err(|e| vec![e])?;
        }

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
        println!("DEBUG MIR: Starting generate_function for '{}' with {} blocks", function.name, function.blocks.len());
        let mut stats = FunctionStats::default();

        // Reset per-function state
        self.value_to_local.clear();
        self.block_labels.clear();
        self.value_to_string_index.clear();
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
        println!("DEBUG MIR: Block order computed: {} blocks to generate", block_order.len());

        for block_id in block_order {
            if let Some(block) = function.blocks.get(&block_id) {
                println!("DEBUG MIR: Generating block {:?} with {} instructions", block_id, block.instructions.len());
                self.generate_basic_block(block)?;
                stats.blocks_generated += 1;
                stats.instructions_generated += block.instructions.len();
                println!("DEBUG MIR: Block {:?} generation completed", block_id);
            }
        }
        
        // Create WASM function with generated instructions
        println!("DEBUG MIR: Computing local types for function '{}'", function.name);
        let local_types = self.compute_local_types(&function);
        println!("DEBUG MIR: Creating WASM function with {} local types and {} instructions", local_types.len(), self.current_instructions.len());
        let mut wasm_function = WasmFunction::new(local_types);
        for instruction in &self.current_instructions {
            wasm_function.instruction(instruction);
        }

        // Add function to WASM module
        println!("DEBUG MIR: Adding function '{}' to WASM module", function.name);
        self.add_function_to_module(function.name.clone(), wasm_function, wasm_signature)?;
        
        Ok(stats)
    }
    
    /// Generate WASM instructions for a basic block
    fn generate_basic_block(&mut self, block: &MirBasicBlock) -> Result<(), CompilerError> {
        println!("DEBUG MIR: generate_basic_block - starting block with {} predecessors", block.predecessors.len());

        // Start block if it has predecessors (not entry block)
        if !block.predecessors.is_empty() {
            if let Some(&label) = self.block_labels.get(&block.id) {
                self.current_instructions.push(Instruction::Block(BlockType::Empty));
                println!("DEBUG MIR: Added Block instruction for label {}", label);
            }
        }

        // Generate instructions
        println!("DEBUG MIR: Generating {} instructions", block.instructions.len());
        for (i, instruction) in block.instructions.iter().enumerate() {
            println!("DEBUG MIR: Processing instruction {}: {:?}", i, instruction.operation);
            self.generate_instruction(instruction)?;
        }

        // Generate terminator
        println!("DEBUG MIR: Generating terminator: {:?}", block.terminator);
        self.generate_terminator(&block.terminator)?;

        // End block if it was started
        if !block.predecessors.is_empty() {
            self.current_instructions.push(Instruction::End);
        }

        println!("DEBUG MIR: generate_basic_block completed successfully");
        Ok(())
    }
    
    /// Generate WASM instruction from MIR instruction
    fn generate_instruction(&mut self, instruction: &MirInstruction) -> Result<(), CompilerError> {
        match &instruction.operation {
            MirOperation::Copy { source } => {
                // Load source operand and store to destination
                self.load_operand(source)?;
                if let Some(dest) = instruction.dest {
                    // Track string constants being copied to locals
                    if let MirOperand::Constant(MirConstant::String(index)) = source {
                        println!("DEBUG MIR: Tracking string constant: ValueId({:?}) -> string index {}", dest.0, index);
                        self.value_to_string_index.insert(dest, *index);
                    }
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
                println!("DEBUG MIR: Processing Call operation with function {:?} and {} arguments", function, arguments.len());

                // Get function name to determine how to handle arguments
                let function_name = match function {
                    MirOperand::Function(symbol_id) => self.get_function_name_by_symbol(*symbol_id),
                    _ => None,
                };
                println!("DEBUG MIR: Function name resolved to: {:?}", function_name);

                // Handle arguments based on function type
                match function_name.as_deref() {
                    Some("print") | Some("printl") | Some("println") => {
                        // Print functions need string arguments expanded to pointer+length
                        for arg in arguments {
                            self.load_string_argument_for_print(arg)?;
                        }
                    }
                    _ => {
                        // Regular argument loading for other functions
                        for arg in arguments {
                            self.load_operand(arg)?;
                        }
                    }
                }

                // Generate function call
                match function {
                    MirOperand::Function(symbol_id) => {
                        // CRITICAL FIX: Map symbol_id to actual function index using symbol table
                        // For built-in functions like print, map directly to function_map
                        if let Some(function_name) = self.get_function_name_by_symbol(*symbol_id) {
                            if let Some(&function_index) = self.wasm_generator.function_map.get(&function_name) {
                                self.current_instructions.push(Instruction::Call(function_index));
                            } else {
                                // Fallback: try to find by name in common patterns
                                let function_index = match function_name.as_str() {
                                    "print" => {
                                        if let Some(&idx) = self.wasm_generator.function_map.get("print") {
                                            idx
                                        } else {
                                            0 // Import index 0 for print
                                        }
                                    }
                                    "printl" => {
                                        if let Some(&idx) = self.wasm_generator.function_map.get("printl") {
                                            idx
                                        } else {
                                            1 // Import index 1 for printl
                                        }
                                    }
                                    _ => 0
                                };
                                self.current_instructions.push(Instruction::Call(function_index));
                            }
                        } else {
                            // Fallback to placeholder
                            self.current_instructions.push(Instruction::Call(0));
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

                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                }

                println!("DEBUG MIR: Call operation processing completed");
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
                // For string constants, we need to load just the index
                // The caller (like function call generation) will handle expansion to ptr+len if needed
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

    /// Load string argument for print functions (expands to pointer + length)
    fn load_string_argument_for_print(&mut self, operand: &MirOperand) -> Result<(), CompilerError> {
        println!("DEBUG MIR: load_string_argument_for_print called with operand {:?}", operand);
        match operand {
            MirOperand::Constant(MirConstant::String(index)) => {
                println!("DEBUG MIR: Processing string constant with index {}", index);
                // For string constants, we need to expand to pointer + length
                if let Some(string_pool) = &self.string_pool {
                    if let Some(string_content) = string_pool.get(*index) {
                        println!("DEBUG MIR: Found string content: '{}'", string_content);
                        // Get the string offset in WASM memory using the underlying generator
                        let data_offset = self.wasm_generator.get_or_create_string_offset(string_content)?;
                        let str_len = string_content.len() as i32;
                        println!("DEBUG MIR: String offset: {}, length: {}", data_offset, str_len);

                        // Push pointer to string content (direct data section offset)
                        self.current_instructions.push(Instruction::I32Const(data_offset as i32));
                        // Push string length
                        self.current_instructions.push(Instruction::I32Const(str_len));
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
                // Check if this value represents a string constant
                println!("DEBUG MIR: Checking ValueId({:?}) for string mapping", value_id.0);
                if let Some(&string_index) = self.value_to_string_index.get(value_id) {
                    println!("DEBUG MIR: Found string mapping: ValueId({:?}) -> index {}", value_id.0, string_index);
                    // This value is a string constant - expand to pointer + length
                    if let Some(string_pool) = &self.string_pool {
                        if let Some(string_content) = string_pool.get(string_index) {
                            let data_offset = self.wasm_generator.get_or_create_string_offset(string_content)?;
                            let str_len = string_content.len() as i32;

                            // Push pointer to string content
                            self.current_instructions.push(Instruction::I32Const(data_offset as i32));
                            // Push string length
                            self.current_instructions.push(Instruction::I32Const(str_len));
                        } else {
                            return Err(CompilerError::Codegen {
                                context: Box::new(crate::error::ErrorContext::new(
                                    format!("String index {} not found in string pool", string_index),
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
                } else {
                    // This is not a string constant - load normally and handle conversion
                    self.load_operand(operand)?;
                    // TODO: Handle conversion of other types to string format for print
                }
            }
            _ => {
                // For other operand types, load normally (this might need conversion)
                self.load_operand(operand)?;
                // TODO: Handle conversion of other types to string format
                // For now, assume the caller handles this
            }
        }
        println!("DEBUG MIR: load_string_argument_for_print completed successfully");
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
    
    /// Get function name by symbol ID
    fn get_function_name_by_symbol(&self, symbol_id: SymbolId) -> Option<String> {
        // For built-in functions, map symbol IDs to standard names
        // This is a simplified approach - ideally we'd have a proper symbol table lookup
        match symbol_id.0 {
            0 => Some("print".to_string()),
            1 => Some("printl".to_string()),
            2 => Some("println".to_string()),
            _ => {
                // For other functions, we need proper symbol resolution
                // For now, assume common built-in functions based on common patterns
                if symbol_id.0 < 10 {
                    Some("print".to_string()) // Default to print for low symbol IDs
                } else {
                    None
                }
            }
        }
    }

    /// Set up memory section
    fn setup_memory_section(&mut self) -> Result<(), CompilerError> {
        // TODO: Implement memory setup
        Ok(())
    }
    
    /// Set up string pool in WASM module
    fn setup_string_pool(&mut self, string_pool: &[String]) -> Result<(), CompilerError> {
        // Store the string pool for use during code generation
        self.string_pool = Some(string_pool.to_vec());

        // Pre-register all strings in the underlying WASM generator's string pool
        // This ensures they get proper data section offsets
        for string_content in string_pool {
            self.wasm_generator.get_or_create_string_offset(string_content)?;
        }

        Ok(())
    }
    
    /// Add function to WASM module
    fn add_function_to_module(
        &mut self,
        name: String,
        function: WasmFunction,
        signature: (Vec<ValType>, Vec<ValType>)
    ) -> Result<(), CompilerError> {
        // Convert signature to the format expected by register_function
        let (param_types, return_types) = signature;
        let return_type = if return_types.is_empty() {
            None
        } else {
            // For now, take the first return type (WASM functions can only return one value in MVP)
            Some(self.val_type_to_wasm_type(&return_types[0])?)
        };

        let param_wasm_types: Vec<_> = param_types.iter()
            .map(|vt| self.val_type_to_wasm_type(vt))
            .collect::<Result<Vec<_>, _>>()?;

        // Get the instructions we've already collected for this function
        let instructions = self.current_instructions.clone();

        // DEBUG: Print function registration info
        println!("DEBUG MIR: Registering function '{}' with {} instructions", name, instructions.len());
        for (i, inst) in instructions.iter().enumerate() {
            println!("DEBUG MIR: Instruction {}: {:?}", i, inst);
        }

        // Register the function in the underlying WASM generator
        self.wasm_generator.register_function(
            &name,
            &param_wasm_types,
            return_type,
            &instructions,
        )?;

        Ok(())
    }

    /// Convert ValType to WasmType
    fn val_type_to_wasm_type(&self, val_type: &ValType) -> Result<crate::codegen::WasmType, CompilerError> {
        use crate::codegen::WasmType;
        match val_type {
            ValType::I32 => Ok(WasmType::I32),
            ValType::I64 => Ok(WasmType::I64),
            ValType::F32 => Ok(WasmType::F32),
            ValType::F64 => Ok(WasmType::F64),
            _ => Err(CompilerError::codegen_error(
                format!("Unsupported WASM value type: {:?}", val_type),
                None,
                None
            ))
        }
    }

    /// Generate start function export for the entry point
    fn generate_start_function_export(&mut self, _entry_symbol_id: SymbolId) -> Result<(), CompilerError> {
        // Look up the function name by symbol ID
        // Since we already processed all functions, the entry function should be registered in the WASM generator
        if let Some(entry_function_index) = self.wasm_generator.function_map.get("start") {
            // Create a _start function that calls the entry function
            let type_index = self.wasm_generator.type_manager.add_function_type(&[], None)?;
            self.wasm_generator.function_section.function(type_index);

            let mut instructions = Vec::new();
            // Call the start function
            instructions.push(Instruction::Call(*entry_function_index));
            // Drop any return value
            instructions.push(Instruction::Drop);
            // End function
            instructions.push(Instruction::End);

            // Create the start function
            let mut start_function = WasmFunction::new(vec![]);
            for instruction in instructions {
                start_function.instruction(&instruction);
            }

            // Add to code section
            self.wasm_generator.code_section.function(&start_function);

            // Export as _start
            let start_func_index = self.wasm_generator.imported_functions.len() as u32
                + self.wasm_generator.function_names.len() as u32;
            self.wasm_generator.export_section.export("_start", wasm_encoder::ExportKind::Func, start_func_index);

            // Update function tracking
            self.wasm_generator.function_names.push("_start".to_string());
            self.wasm_generator.function_count += 1;
        } else {
            return Err(CompilerError::codegen_error(
                "Entry point function 'start' not found in function map".to_string(),
                None,
                None
            ));
        }

        Ok(())
    }


    /// Finalize WASM module and return bytecode
    fn finalize_module(&mut self) -> Result<Vec<u8>, CompilerError> {
        use wasm_encoder::Module;

        let mut module = Module::new();

        // Add sections from the internal CodeGenerator
        // Note: We need to clone the sections since they don't implement Copy/Clone directly

        // 1. Add type section
        let type_section = self.wasm_generator.type_manager.clone_type_section();
        module.section(&type_section);

        // 2. Add import section - clone it
        let import_section = self.wasm_generator.import_section.clone();
        module.section(&import_section);

        // 3. Add function section - clone it
        let function_section = self.wasm_generator.function_section.clone();
        module.section(&function_section);

        // 4. Add memory section - clone it
        let memory_section = self.wasm_generator.memory_section.clone();
        module.section(&memory_section);

        // 5. Add export section - clone it
        let export_section = self.wasm_generator.export_section.clone();
        module.section(&export_section);

        // 6. Add code section - clone it
        let code_section = self.wasm_generator.code_section.clone();
        module.section(&code_section);

        Ok(module.finish())
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