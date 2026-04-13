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

    /// NOTE: Mapping from SymbolId to function name for ALL functions
    /// This includes both builtin functions (print, math.*, etc.) and user-defined functions
    /// Used by code generator to resolve function calls
    pub symbol_name_map: HashMap<SymbolId, String>,

    /// Plugins used by this program, detected during AST parsing
    /// Maps plugin name to whether it was explicitly imported or auto-detected
    #[allow(dead_code)]
    pub used_plugins: Vec<String>,

    /// State rules (invariants) — tracked here for tooling/debugging
    /// Actual enforcement is done via MirTerminator::Trap blocks injected into start/frame functions
    pub state_rules: Vec<MirStateRule>,

    /// State guards — per-variable validation
    /// Actual enforcement is done via guard checks injected before state variable assignments
    pub state_guards: Vec<MirStateGuard>,

    /// External functions (WASM imports from host)
    /// These generate import entries during code generation
    pub externals: Vec<MirExternalFunction>,
}

/// MIR External Function - a function provided by the WASM host (imported)
#[derive(Debug, Clone)]
pub struct MirExternalFunction {
    /// Function name (e.g., "_req_body_field", "_http_respond")
    pub name: String,

    /// Function parameters with types
    pub parameters: Vec<MirParameter>,

    /// Return type (MirType::Void for functions that don't return)
    pub return_type: MirType,

    /// WASM import module name (defaults to "env")
    pub module: String,

    /// Source location for debugging
    pub location: SourceLocation,
}

/// State rule — a boolean invariant that must hold at operation boundaries
#[derive(Debug, Clone)]
pub struct MirStateRule {
    /// The boolean expression that must be true
    pub condition: MirExpression,
    /// Source location for error reporting
    pub location: SourceLocation,
}

/// State guard — per-variable validation checked before assignment
#[derive(Debug, Clone)]
pub struct MirStateGuard {
    /// The state variable this guard applies to
    pub variable_symbol_id: SymbolId,
    /// The variable name for error messages
    pub variable_name: String,
    /// The boolean condition (uses 'value' binding for the proposed new value)
    pub condition: MirExpression,
    /// Error message shown on guard violation
    pub error_message: String,
    /// Source location for error reporting
    pub location: SourceLocation,
}

/// Simplified MIR expression for state rules/guards (evaluated at runtime)
#[derive(Debug, Clone)]
pub enum MirExpression {
    /// Compare: left op right → boolean
    Compare {
        left: Box<MirExpression>,
        op: MirCompareOp,
        right: Box<MirExpression>,
    },
    /// Logical AND
    And(Box<MirExpression>, Box<MirExpression>),
    /// Logical OR
    Or(Box<MirExpression>, Box<MirExpression>),
    /// Logical NOT
    Not(Box<MirExpression>),
    /// Read a global state variable
    GlobalGet(SymbolId),
    /// Integer constant
    IntConst(i64),
    /// Float constant
    FloatConst(f64),
    /// Boolean constant
    BoolConst(bool),
}

/// Comparison operator for MIR expressions
#[derive(Debug, Clone)]
pub enum MirCompareOp {
    Equal,
    NotEqual,
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,
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

    /// Box a value into an `any` type with runtime type tag
    /// Memory layout: [tag:i32][value1:i32][value2:i32] = 12 bytes
    BoxAny {
        /// The value to box
        value: MirOperand,
        /// The type tag to use
        type_tag: AnyTypeTag,
        /// The source type (needed for f64 handling)
        source_type: MirType,
    },

    /// Convert an `any` value to string with proper type dispatch
    /// Reads the type tag and calls the appropriate toString function
    AnyToString {
        /// The boxed any value
        value: MirOperand,
    },

    /// Unbox an `any` value to its i32 representation
    UnboxAnyToI32 {
        /// The boxed any value
        value: MirOperand,
    },

    /// Unbox an `any` value to its f64 representation
    UnboxAnyToF64 {
        /// The boxed any value
        value: MirOperand,
    },

    /// Unbox an `any` value to its boolean (i32 0/1) representation
    /// Reads the type tag: tag 1 = false, tag 2 = true, tag 3 = number != 0.0
    UnboxAnyToBoolean {
        /// The boxed any value
        value: MirOperand,
    },

    /// Select instruction (ternary): result = condition ? true_value : false_value
    /// Maps to WASM's `select` instruction
    Select {
        /// Condition (i32 boolean)
        condition: MirOperand,
        /// Value to use if condition is true (non-zero)
        true_value: MirOperand,
        /// Value to use if condition is false (zero)
        false_value: MirOperand,
    },

    /// Access a field on an Any (JSON object) value by string key
    /// Generates a call to __json_get_field(any_ptr, key_ptr, key_len)
    /// Returns Any (pointer to field value or null if not found)
    AnyGetField {
        /// The boxed any value (JSON object pointer)
        object: MirOperand,
        /// The field key (string pointer)
        key: MirOperand,
    },

    /// Access an element on an Any (JSON array) value by integer index
    /// Generates a call to __json_get_index(any_ptr, index)
    /// Returns Any (pointer to element or null if out of bounds)
    AnyGetIndex {
        /// The boxed any value (JSON array pointer)
        array: MirOperand,
        /// The integer index
        index: MirOperand,
    },

    /// Load from a global variable
    /// Generates WASM global.get instruction
    GlobalLoad {
        /// Symbol ID of the global variable
        global_id: SymbolId,
        /// Type of the global (needed for proper WASM type)
        global_type: MirType,
    },

    /// Store to a global variable
    /// Generates WASM global.set instruction
    GlobalStore {
        /// Symbol ID of the global variable
        global_id: SymbolId,
        /// Value to store
        value: MirOperand,
        /// Type of the global (needed for proper WASM type)
        global_type: MirType,
    },
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

    /// Unreachable code (placeholder for void functions)
    Unreachable,

    /// Trap: explicit contract violation (require/rules failure)
    /// Unlike Unreachable, this ALWAYS generates WASM unreachable instruction
    Trap,
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

    /// Any type - boxed value with runtime type tag
    /// Memory layout: [tag:i32][value1:i32][value2:i32] = 12 bytes
    /// Tag values: 0=Null, 1=Integer, 2=Boolean, 3=Number, 4=String, 5=List, 6=Object
    /// For Number (f64): bits are split across value1 (low) and value2 (high)
    /// Represented as i32 pointer to boxed structure in memory
    Any,
}

/// Type tags for boxed `any` values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AnyTypeTag {
    Null = 0,
    Integer = 1,
    Boolean = 2,
    Number = 3,
    String = 4,
    List = 5,
    Object = 6,
}

impl AnyTypeTag {
    /// Get the tag value as i32 for WASM
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    /// Convert from i32 tag value
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(AnyTypeTag::Null),
            1 => Some(AnyTypeTag::Integer),
            2 => Some(AnyTypeTag::Boolean),
            3 => Some(AnyTypeTag::Number),
            4 => Some(AnyTypeTag::String),
            5 => Some(AnyTypeTag::List),
            6 => Some(AnyTypeTag::Object),
            _ => None,
        }
    }
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

    // BOOK: required-operator - Postfix ! assertion for null check
    /// Required assertion - asserts value is not null at runtime
    Required,
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
            MirType::Any => 12,        // [tag:i32][value1:i32][value2:i32]
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
            MirType::Any => 4,         // Aligned to i32 (all fields are i32)
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
            ConcreteType::Integer => MirType::I32,
            ConcreteType::Number => MirType::F64,
            // Precision integer types → appropriate WASM integer sizes
            ConcreteType::IntegerSized {
                bits: 8,
                unsigned: false,
            } => MirType::I8,
            ConcreteType::IntegerSized {
                bits: 8,
                unsigned: true,
            } => MirType::U8,
            ConcreteType::IntegerSized {
                bits: 16,
                unsigned: false,
            } => MirType::I16,
            ConcreteType::IntegerSized {
                bits: 16,
                unsigned: true,
            } => MirType::U16,
            ConcreteType::IntegerSized {
                bits: 32,
                unsigned: false,
            } => MirType::I32,
            ConcreteType::IntegerSized {
                bits: 32,
                unsigned: true,
            } => MirType::U32,
            ConcreteType::IntegerSized {
                bits: 64,
                unsigned: false,
            } => MirType::I64,
            ConcreteType::IntegerSized {
                bits: 64,
                unsigned: true,
            } => MirType::U64,
            ConcreteType::IntegerSized { .. } => MirType::I32, // fallback for other sizes
            // Precision number types → appropriate WASM float sizes
            ConcreteType::NumberSized { bits: 32 } => MirType::F32,
            ConcreteType::NumberSized { bits: 64 } => MirType::F64,
            ConcreteType::NumberSized { .. } => MirType::F64, // fallback
            ConcreteType::String => MirType::Ptr(Box::new(MirType::I8)), // Strings are i32 pointer to [len|content] - use Ptr(I8) for consistent type tracking with string literals
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
                // NOTE: Classes as i32 pointer in WASM (heap-allocated objects)
                // Cannot use Ptr(Void) because codegen treats that as void return type
                MirType::I32
            }
            ConcreteType::Pairs(_, _) => {
                // NOTE: Pairs as i32 pointer in WASM (heap-allocated map structure)
                // Similar to Class - cannot use Ptr(Void) as it becomes void return type
                MirType::I32
            }
            ConcreteType::Matrix(_) => {
                // NOTE: Matrix as i32 pointer in WASM (heap-allocated 2D array)
                // Similar to Class and Pairs - i32 pointer representation
                MirType::I32
            }
            ConcreteType::Generic { name, .. } => {
                // Generic types that haven't been resolved should be void for function returns
                // This handles cases like start() which may have unresolved generic return types
                if name == "0" {
                    MirType::Void
                } else {
                    MirType::Any // Unresolved generics → Any (boxed at runtime)
                }
            }
            ConcreteType::Any => MirType::Any, // Boxed any type with runtime type tag
            _ => MirType::Any, // Fallback for complex types → Any (avoids Ptr(Void) ambiguity)
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
            MirType::Any => write!(f, "any"),
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
