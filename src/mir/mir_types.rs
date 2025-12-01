//! MIR Type Definitions
//!
//! Defines the core types and structures for the Medium-level Intermediate Representation.
//! MIR is designed to be optimization-friendly while maintaining type safety.

use crate::ast::SourceLocation;
use crate::resolver::SymbolId;
use crate::typechecker::tast::ConcreteType;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Unique identifier for basic blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BasicBlockId(pub usize);

/// Unique identifier for SSA values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub usize);

/// Unique identifier for virtual registers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegisterId(pub usize);

/// Complete MIR program representation
#[derive(Debug, Clone)]
pub struct MirProgram {
    /// All functions in the program
    pub functions: HashMap<SymbolId, MirFunction>,

    /// Global variables and constants
    pub globals: HashMap<SymbolId, MirGlobal>,

    /// String literals pool
    pub string_pool: Vec<String>,

    /// Entry point function (start function)
    pub entry_point: Option<SymbolId>,

    /// Debug information if enabled
    pub debug_info: Option<MirDebugInfo>,

    /// CRITICAL FIX: Mapping from SymbolId to function name for ALL functions
    /// This includes both builtin functions (print, math.*, etc.) and user-defined functions
    /// Used by code generator to resolve function calls
    pub symbol_name_map: HashMap<SymbolId, String>,
}

/// MIR function representation
#[derive(Debug, Clone)]
pub struct MirFunction {
    /// Original symbol ID from TAST
    pub symbol_id: SymbolId,

    /// Function name
    pub name: String,

    /// Function parameters
    pub parameters: Vec<MirParameter>,

    /// Return type
    pub return_type: MirType,

    /// Basic blocks that make up this function
    pub blocks: HashMap<BasicBlockId, MirBasicBlock>,

    /// Entry basic block
    pub entry_block: BasicBlockId,

    /// Local variables (including temporaries)
    pub locals: HashMap<ValueId, MirLocal>,

    /// Next value ID for SSA form
    pub next_value_id: usize,

    /// Next basic block ID
    pub next_block_id: usize,

    /// Function attributes
    pub attributes: MirFunctionAttributes,

    /// Source location for debugging
    pub location: SourceLocation,
}

/// MIR function parameter
#[derive(Debug, Clone)]
pub struct MirParameter {
    /// Parameter value ID
    pub value_id: ValueId,

    /// Parameter name
    pub name: String,

    /// Parameter type
    pub param_type: MirType,

    /// Source location
    pub location: SourceLocation,
}

/// MIR local variable
#[derive(Debug, Clone)]
pub struct MirLocal {
    /// Variable name (for debugging)
    pub name: Option<String>,

    /// Variable type
    pub local_type: MirType,

    /// Whether this local is mutable
    pub is_mutable: bool,

    /// Source location
    pub location: SourceLocation,
}

/// MIR global variable or constant
#[derive(Debug, Clone)]
pub struct MirGlobal {
    /// Global symbol ID
    pub symbol_id: SymbolId,

    /// Global name
    pub name: String,

    /// Global type
    pub global_type: MirType,

    /// Initial value (for constants)
    pub initializer: Option<MirConstant>,

    /// Whether this global is mutable
    pub is_mutable: bool,

    /// Source location
    pub location: SourceLocation,
}

/// MIR basic block
#[derive(Debug, Clone)]
pub struct MirBasicBlock {
    /// Unique block identifier
    pub id: BasicBlockId,

    /// Block label (for debugging)
    pub label: Option<String>,

    /// Instructions in this block
    pub instructions: Vec<MirInstruction>,

    /// Block terminator instruction
    pub terminator: MirTerminator,

    /// Predecessors in control flow graph
    pub predecessors: HashSet<BasicBlockId>,

    /// Successors in control flow graph
    pub successors: HashSet<BasicBlockId>,

    /// Source location
    pub location: SourceLocation,
}

/// MIR instruction
#[derive(Debug, Clone)]
pub struct MirInstruction {
    /// Destination value (None for side-effect only instructions)
    pub dest: Option<ValueId>,

    /// Instruction operation
    pub operation: MirOperation,

    /// Source location for debugging
    pub location: SourceLocation,
}

/// MIR operation types
#[derive(Debug, Clone)]
pub enum MirOperation {
    /// Load from memory: dest = *src
    Load { source: MirOperand },

    /// Store to memory: *dest = src
    Store {
        destination: MirOperand,
        value: MirOperand,
    },

    /// Binary arithmetic/logical operation
    BinaryOp {
        op: MirBinaryOp,
        left: MirOperand,
        right: MirOperand,
    },

    /// Unary operation
    UnaryOp { op: MirUnaryOp, operand: MirOperand },

    /// Type conversion/cast
    Cast {
        value: MirOperand,
        target_type: MirType,
    },

    /// Function call
    Call {
        function: MirOperand,
        arguments: Vec<MirOperand>,
    },

    /// Array/structure element access
    GetElementPtr {
        base: MirOperand,
        indices: Vec<MirOperand>,
        /// True for array access (needs 16-byte header offset), false for struct field access
        is_array: bool,
    },

    /// Allocate local memory
    Alloca { size: MirOperand, alignment: u32 },

    /// Copy value
    Copy { source: MirOperand },

    /// Move value (for move semantics)
    Move { source: MirOperand },

    /// Phi node for SSA form
    Phi {
        incoming: Vec<(BasicBlockId, MirOperand)>,
    },

    /// Async assignment for later variables
    AsyncAssign { source: MirOperand },
}

/// MIR terminator instructions (end basic blocks)
#[derive(Debug, Clone)]
pub enum MirTerminator {
    /// Unconditional jump
    Jump { target: BasicBlockId },

    /// Conditional branch
    Branch {
        condition: MirOperand,
        true_block: BasicBlockId,
        false_block: BasicBlockId,
    },

    /// Return from function
    Return { value: Option<MirOperand> },

    /// Unreachable code
    Unreachable,
}

/// MIR operand (source of operations)
#[derive(Debug, Clone)]
pub enum MirOperand {
    /// SSA value reference
    Value(ValueId),

    /// Constant value
    Constant(MirConstant),

    /// Function reference
    Function(SymbolId),

    /// Named function (for stdlib namespace functions like math.max, string.length)
    /// Stores both the name and SymbolId since SymbolId(0) is shared by all namespace functions
    NamedFunction { name: String, symbol_id: SymbolId },

    /// Global variable reference
    Global(SymbolId),
}

/// MIR constant values
#[derive(Debug, Clone)]
pub enum MirConstant {
    /// Integer constant
    Integer(i64),

    /// Floating point constant
    Float(f64),

    /// Boolean constant
    Boolean(bool),

    /// String constant (index into string pool)
    String(usize),

    /// Null pointer
    Null,

    /// Undefined value
    Undefined,

    /// Array constant
    Array(Vec<MirConstant>),

    /// Structure constant
    Struct(Vec<MirConstant>),
}

/// MIR type system (simplified from TAST)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MirType {
    /// Void type (no value)
    Void,

    /// Integer types with explicit bit width
    I8,
    I16,
    I32,
    I64,

    /// Unsigned integer types
    U8,
    U16,
    U32,
    U64,

    /// Floating point types
    F32,
    F64,

    /// Boolean type
    Bool,

    /// Pointer type
    Ptr(Box<MirType>),

    /// Array type with size
    Array(Box<MirType>, usize),

    /// Function type
    Function {
        parameters: Vec<MirType>,
        return_type: Box<MirType>,
    },

    /// Structure type
    Struct(Vec<MirType>),

    /// String tuple type (pointer, length) for WebAssembly compatibility
    /// Strings in WASM are represented as (i32 ptr, i32 len) pairs
    StringTuple,
}

/// Binary operations in MIR
#[derive(Debug, Clone, PartialEq)]
pub enum MirBinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Rem,

    // Bitwise
    And,
    Or,
    Xor,
    Shl,
    Shr,

    // Comparison (produce boolean)
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Unary operations in MIR
#[derive(Debug, Clone, PartialEq)]
pub enum MirUnaryOp {
    /// Arithmetic negation
    Neg,

    /// Logical negation
    Not,

    /// Bitwise complement
    BitNot,
}

/// Function attributes
#[derive(Debug, Clone, Default)]
pub struct MirFunctionAttributes {
    /// Function is marked as inline
    pub inline: bool,

    /// Function is pure (no side effects)
    pub pure: bool,

    /// Function is the entry point
    pub entry_point: bool,

    /// Function is exported
    pub exported: bool,
}

/// Debug information for MIR
#[derive(Debug, Clone)]
pub struct MirDebugInfo {
    /// Source file names
    pub source_files: Vec<String>,

    /// Line number information for instructions
    pub line_info: HashMap<ValueId, (usize, u32)>, // (file_index, line_number)

    /// Variable names for debugging
    pub variable_names: HashMap<ValueId, String>,
}

impl MirType {
    /// Get the size in bytes of this type
    pub fn size_bytes(&self) -> usize {
        match self {
            MirType::Void => 0,
            MirType::I8 | MirType::U8 | MirType::Bool => 1,
            MirType::I16 | MirType::U16 => 2,
            MirType::I32 | MirType::U32 | MirType::F32 => 4,
            MirType::I64 | MirType::U64 | MirType::F64 | MirType::Ptr(_) => 8,
            MirType::StringTuple => 8, // Two i32 values (ptr + len)
            MirType::Array(element_type, count) => element_type.size_bytes() * count,
            MirType::Function { .. } => 8, // Function pointer
            MirType::Struct(fields) => fields.iter().map(|f| f.size_bytes()).sum(),
        }
    }

    /// Get the alignment requirement for this type
    pub fn alignment(&self) -> usize {
        match self {
            MirType::Void => 1,
            MirType::I8 | MirType::U8 | MirType::Bool => 1,
            MirType::I16 | MirType::U16 => 2,
            MirType::I32 | MirType::U32 | MirType::F32 => 4,
            MirType::I64 | MirType::U64 | MirType::F64 | MirType::Ptr(_) => 8,
            MirType::StringTuple => 4, // Aligned to i32
            MirType::Array(element_type, _) => element_type.alignment(),
            MirType::Function { .. } => 8,
            MirType::Struct(fields) => fields.iter().map(|f| f.alignment()).max().unwrap_or(1),
        }
    }

    /// Check if this type is a primitive type
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            MirType::I8
                | MirType::I16
                | MirType::I32
                | MirType::I64
                | MirType::U8
                | MirType::U16
                | MirType::U32
                | MirType::U64
                | MirType::F32
                | MirType::F64
                | MirType::Bool
        )
    }

    /// Convert from TAST ConcreteType to MIR type
    pub fn from_concrete_type(concrete_type: &ConcreteType) -> Self {
        match concrete_type {
            ConcreteType::Integer => MirType::I32, // CRITICAL FIX: Integers are i32 in WASM, not i64
            ConcreteType::Number => MirType::F64,
            ConcreteType::String => MirType::I32, // CRITICAL FIX: Strings are i32 pointer to [len|content] structure in memory
            ConcreteType::Boolean => MirType::Bool,
            ConcreteType::Null => MirType::Ptr(Box::new(MirType::Void)),
            ConcreteType::Undefined => MirType::Void,
            ConcreteType::Array(element_type) => {
                // Dynamic arrays as pointer to elements
                MirType::Ptr(Box::new(Self::from_concrete_type(element_type)))
            }
            ConcreteType::Function {
                parameters,
                return_type,
                ..
            } => MirType::Function {
                parameters: parameters.iter().map(Self::from_concrete_type).collect(),
                return_type: Box::new(Self::from_concrete_type(return_type)),
            },
            ConcreteType::Class { .. } => {
                // CRITICAL FIX: Classes as i32 pointer in WASM (heap-allocated objects)
                // Cannot use Ptr(Void) because codegen treats that as void return type
                MirType::I32
            }
            ConcreteType::Pairs(_, _) => {
                // CRITICAL FIX: Pairs as i32 pointer in WASM (heap-allocated map structure)
                // Similar to Class - cannot use Ptr(Void) as it becomes void return type
                MirType::I32
            }
            ConcreteType::Matrix(_) => {
                // CRITICAL FIX: Matrix as i32 pointer in WASM (heap-allocated 2D array)
                // Similar to Class and Pairs - i32 pointer representation
                MirType::I32
            }
            ConcreteType::Generic { name, .. } => {
                // Generic types that haven't been resolved should be void for function returns
                // This handles cases like start() which may have unresolved generic return types
                if name == "0" {
                    MirType::Void
                } else {
                    MirType::Ptr(Box::new(MirType::Void)) // Fallback for other generics
                }
            }
            _ => MirType::Ptr(Box::new(MirType::Void)), // Fallback for complex types
        }
    }
}

impl fmt::Display for MirType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MirType::Void => write!(f, "void"),
            MirType::I8 => write!(f, "i8"),
            MirType::I16 => write!(f, "i16"),
            MirType::I32 => write!(f, "i32"),
            MirType::I64 => write!(f, "i64"),
            MirType::U8 => write!(f, "u8"),
            MirType::U16 => write!(f, "u16"),
            MirType::U32 => write!(f, "u32"),
            MirType::U64 => write!(f, "u64"),
            MirType::F32 => write!(f, "f32"),
            MirType::F64 => write!(f, "f64"),
            MirType::Bool => write!(f, "bool"),
            MirType::Ptr(inner) => write!(f, "{}*", inner),
            MirType::Array(element, size) => write!(f, "[{} x {}]", element, size),
            MirType::Function {
                parameters,
                return_type,
            } => {
                let params = parameters
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({}) -> {}", params, return_type)
            }
            MirType::Struct(fields) => {
                let field_types = fields
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{{{}}}", field_types)
            }
            MirType::StringTuple => write!(f, "string"),
        }
    }
}

impl fmt::Display for BasicBlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

impl fmt::Display for RegisterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}
