//! Mid-level Intermediate Representation (MIR)
//!
//! MIR represents control flow graphs with basic blocks

use crate::ir::*;
use std::collections::HashMap;

/// MIR Program - control flow representation
#[derive(Debug, Clone)]
pub struct MIRProgram {
    pub functions: HashMap<String, MIRFunction>,
    pub classes: HashMap<String, MIRClass>,
    pub globals: Vec<MIRGlobal>,
}

/// MIR Function with control flow graph
#[derive(Debug, Clone)]
pub struct MIRFunction {
    pub name: String,
    pub parameters: Vec<MIRLocal>,
    pub return_type: MIRType,
    pub locals: Vec<MIRLocal>,
    pub basic_blocks: Vec<MIRBasicBlock>,
    pub entry_block: BlockId,
}

/// MIR Basic Block
#[derive(Debug, Clone)]
pub struct MIRBasicBlock {
    pub id: BlockId,
    pub instructions: Vec<MIRInstruction>,
    pub terminator: MIRTerminator,
}

/// Basic block identifier
pub type BlockId = usize;

/// MIR Local variable
#[derive(Debug, Clone)]
pub struct MIRLocal {
    pub id: LocalId,
    pub name: String,
    pub local_type: MIRType,
}

/// Local variable identifier
pub type LocalId = usize;

/// MIR Global variable
#[derive(Debug, Clone)]
pub struct MIRGlobal {
    pub name: String,
    pub global_type: MIRType,
    pub initializer: Option<MIRConstant>,
}

/// MIR Class
#[derive(Debug, Clone)]
pub struct MIRClass {
    pub name: String,
    pub fields: Vec<MIRField>,
    pub methods: HashMap<String, MIRFunction>,
}

/// MIR Field
#[derive(Debug, Clone)]
pub struct MIRField {
    pub name: String,
    pub field_type: MIRType,
    pub offset: usize,
}

/// MIR Instruction
#[derive(Debug, Clone)]
pub enum MIRInstruction {
    // Arithmetic
    Add(LocalId, MIROperand, MIROperand),
    Sub(LocalId, MIROperand, MIROperand),
    Mul(LocalId, MIROperand, MIROperand),
    Div(LocalId, MIROperand, MIROperand),

    // Memory
    Load(LocalId, MIROperand),
    Store(MIROperand, MIROperand),

    // Function calls
    Call(LocalId, String, Vec<MIROperand>),

    // Type conversions
    Cast(LocalId, MIROperand, MIRType),

    // Constants
    Const(LocalId, MIRConstant),
}

/// MIR Operand
#[derive(Debug, Clone)]
pub enum MIROperand {
    Local(LocalId),
    Constant(MIRConstant),
    Global(String),
}

/// MIR Constant
#[derive(Debug, Clone)]
pub enum MIRConstant {
    Integer(i64),
    Number(f64),
    String(String),
    Boolean(bool),
}

/// MIR Terminator (end of basic block)
#[derive(Debug, Clone)]
pub enum MIRTerminator {
    /// Unconditional jump
    Goto(BlockId),

    /// Conditional jump
    Branch {
        condition: MIROperand,
        then_block: BlockId,
        else_block: BlockId,
    },

    /// Return from function
    Return(Option<MIROperand>),

    /// Unreachable code
    Unreachable,
}

/// MIR Type system
#[derive(Debug, Clone, PartialEq)]
pub enum MIRType {
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Bool,
    Ptr(Box<MIRType>),
    Array(Box<MIRType>, usize),
    Struct(String),
    Function(Vec<MIRType>, Box<MIRType>),
    Void,
}
