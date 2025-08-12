//! Low-level Intermediate Representation (LIR)
//!
//! LIR represents WebAssembly-ready instruction sequences


/// LIR Program - WebAssembly ready
#[derive(Debug, Clone)]
pub struct LIRProgram {
    pub functions: Vec<LIRFunction>,
    pub memory_layout: LIRMemoryLayout,
    pub imports: Vec<LIRImport>,
    pub exports: Vec<LIRExport>,
}

/// LIR Function
#[derive(Debug, Clone)]
pub struct LIRFunction {
    pub name: String,
    pub parameters: Vec<LIRType>,
    pub return_type: Option<LIRType>,
    pub locals: Vec<LIRType>,
    pub instructions: Vec<LIRInstruction>,
}

/// LIR Memory Layout
#[derive(Debug, Clone)]
pub struct LIRMemoryLayout {
    pub initial_pages: u32,
    pub max_pages: Option<u32>,
    pub heap_start: u32,
    pub stack_start: u32,
}

/// LIR Import
#[derive(Debug, Clone)]
pub struct LIRImport {
    pub module: String,
    pub name: String,
    pub import_type: LIRImportType,
}

/// LIR Import Type
#[derive(Debug, Clone)]
pub enum LIRImportType {
    Function(Vec<LIRType>, Option<LIRType>),
    Memory(u32, Option<u32>),
    Global(LIRType, bool), // type, mutable
}

/// LIR Export
#[derive(Debug, Clone)]
pub struct LIRExport {
    pub name: String,
    pub export_type: LIRExportType,
}

/// LIR Export Type
#[derive(Debug, Clone)]
pub enum LIRExportType {
    Function(usize), // function index
    Memory(usize),   // memory index
    Global(usize),   // global index
}

/// LIR Instruction - maps directly to WASM instructions
#[derive(Debug, Clone)]
pub enum LIRInstruction {
    // Control flow
    Block(LIRType),
    Loop(LIRType),
    If(LIRType),
    Else,
    End,
    Br(u32),
    BrIf(u32),
    Return,

    // Constants
    I32Const(i32),
    I64Const(i64),
    F32Const(f32),
    F64Const(f64),

    // Arithmetic (i32)
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,

    // Arithmetic (i64)
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64DivU,
    I64RemS,
    I64RemU,

    // Arithmetic (f32)
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,

    // Arithmetic (f64)
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,

    // Comparison (i32)
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,

    // Comparison (f32)
    F32Eq,
    F32Ne,
    F32Lt,
    F32Gt,
    F32Le,
    F32Ge,

    // Comparison (f64)
    F64Eq,
    F64Ne,
    F64Lt,
    F64Gt,
    F64Le,
    F64Ge,

    // Memory
    I32Load(u32, u32), // align, offset
    I64Load(u32, u32),
    F32Load(u32, u32),
    F64Load(u32, u32),
    I32Store(u32, u32), // align, offset
    I64Store(u32, u32),
    F32Store(u32, u32),
    F64Store(u32, u32),
    MemorySize,
    MemoryGrow,

    // Variables
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(u32),
    GlobalSet(u32),

    // Function calls
    Call(u32),         // function index
    CallIndirect(u32), // type index

    // Stack manipulation
    Drop,
    Select,

    // Type conversions
    I32WrapI64,
    I64ExtendI32S,
    I64ExtendI32U,
    I32TruncF32S,
    I32TruncF32U,
    I32TruncF64S,
    I32TruncF64U,
    I64TruncF32S,
    I64TruncF32U,
    I64TruncF64S,
    I64TruncF64U,
    F32ConvertI32S,
    F32ConvertI32U,
    F32ConvertI64S,
    F32ConvertI64U,
    F32DemoteF64,
    F64ConvertI32S,
    F64ConvertI32U,
    F64ConvertI64S,
    F64ConvertI64U,
    F64PromoteF32,

    // Memory management instructions
    /// Allocate memory: (type_id: i32, size: i32) -> address: i32
    MemAlloc,
    /// Increment reference count: (address: i32) -> ()
    MemRetain,
    /// Decrement reference count: (address: i32) -> ()
    MemRelease,
    /// Trigger garbage collection: () -> freed_count: i32
    MemCollect,
    /// Get reference count: (address: i32) -> ref_count: i32
    MemGetRefCount,
}

/// LIR Type - WebAssembly value types
#[derive(Debug, Clone, PartialEq)]
pub enum LIRType {
    I32,
    I64,
    F32,
    F64,
}
