# Clean Language Intermediate Representation (IR) Documentation

This document provides comprehensive documentation for Claude on Clean Language's Intermediate Representation (IR) system, optimization pipeline, and the transformation process from AST to WebAssembly. This knowledge is essential for extending the compiler's optimization capabilities and debugging code generation issues.

> 🔗 **Related Documentation**: [AST Reference](./ast-reference.md) • [WebAssembly Generation](./webassembly.md) • [Semantic Analysis](./semantic-analysis.md) • [Development Guide](./development-guide.md)

## Overview

The Clean Language compiler employs a multi-layered IR architecture that progressively transforms high-level Clean Language constructs into efficient WebAssembly bytecode. The IR system is designed for optimization, analysis, and WebAssembly-specific transformations while maintaining type safety and debug information.

## IR Architecture Overview

### 1. Multi-Layer IR Pipeline

```
Clean Source Code
       ↓
   AST (Abstract Syntax Tree)
       ↓
   HIR (High-level IR) - Desugared, name-resolved
       ↓
   MIR (Mid-level IR) - Control flow, type-checked
       ↓
   LIR (Low-level IR) - WebAssembly-ready
       ↓
   WASM Bytecode
```

**Layer Characteristics:**
- **AST**: Direct syntactic representation
- **HIR**: Semantic analysis ready, desugared
- **MIR**: Optimization target, control flow graph
- **LIR**: WebAssembly instruction sequences
- **WASM**: Final executable bytecode

### 2. IR Design Principles

```rust
// Core IR design traits
pub trait IRNode {
    fn node_id(&self) -> NodeId;
    fn ty(&self) -> Option<&Type>;
    fn span(&self) -> &Span;
}

pub trait IRTransformable {
    type Output;
    fn transform<T: IRTransform>(&self, transformer: &mut T) -> Result<Self::Output, CompilerError>;
}

pub trait IROptimizable {
    fn optimize(&mut self, context: &OptimizationContext) -> bool;  // Returns true if changed
}
```

## High-Level Intermediate Representation (HIR)

### 1. HIR Structure (`src/hir/mod.rs`)

```rust
/// HIR representation of a Clean Language program
#[derive(Debug, Clone)]
pub struct HIRProgram {
    pub node_id: NodeId,
    pub span: Span,
    pub items: Vec<HIRItem>,
    pub symbol_table: SymbolTable,
    pub type_table: TypeTable,
}

#[derive(Debug, Clone)]
pub enum HIRItem {
    Function(HIRFunction),
    Class(HIRClass),
    DataType(HIRDataType),
    Import(HIRImport),
}

#[derive(Debug, Clone)]
pub struct HIRFunction {
    pub node_id: NodeId,
    pub span: Span,
    pub name: Symbol,
    pub parameters: Vec<HIRParameter>,
    pub return_type: Type,
    pub body: HIRBlock,
    pub attributes: FunctionAttributes,
    pub local_variables: Vec<HIRLocalVariable>,
}

#[derive(Debug, Clone)]
pub struct FunctionAttributes {
    pub is_async: bool,
    pub is_static: bool,
    pub visibility: Visibility,
    pub inline_hint: InlineHint,
}

#[derive(Debug, Clone)]
pub enum InlineHint {
    Never,
    Auto,
    Always,
}
```

**HIR Features:**
- **Symbol Resolution**: All names resolved to unique symbols
- **Type Information**: Complete type annotations
- **Desugaring**: Complex syntax reduced to core constructs
- **Attribute Collection**: Metadata for optimization decisions

### 2. HIR Expressions

```rust
#[derive(Debug, Clone)]
pub enum HIRExpression {
    Literal(HIRLiteral),
    Variable(HIRVariable),
    Binary(HIRBinaryOp),
    Unary(HIRUnaryOp),
    Call(HIRCall),
    MethodCall(HIRMethodCall),
    FieldAccess(HIRFieldAccess),
    Index(HIRIndex),
    Cast(HIRCast),
    Block(HIRBlock),
}

#[derive(Debug, Clone)]
pub struct HIRBinaryOp {
    pub node_id: NodeId,
    pub span: Span,
    pub left: Box<HIRExpression>,
    pub operator: BinaryOperator,
    pub right: Box<HIRExpression>,
    pub result_type: Type,
    pub overflow_behavior: OverflowBehavior,  // For arithmetic ops
}

#[derive(Debug, Clone)]
pub enum OverflowBehavior {
    Wrap,      // Wrapping arithmetic
    Saturate,  // Saturating arithmetic
    Panic,     // Panic on overflow
    Check,     // Runtime overflow check
}

#[derive(Debug, Clone)]
pub struct HIRCall {
    pub node_id: NodeId,
    pub span: Span,
    pub function: Symbol,
    pub arguments: Vec<HIRExpression>,
    pub return_type: Type,
    pub call_kind: CallKind,
}

#[derive(Debug, Clone)]
pub enum CallKind {
    Direct,           // Direct function call
    Virtual,          // Virtual method call
    Static,           // Static method call
    BuiltinFunction,  // Standard library function
    ForeignFunction,  // External/imported function
}
```

**HIR Expression Features:**
- **Complete Type Information**: Every expression has a resolved type
- **Overflow Behavior**: Explicit handling of arithmetic overflow
- **Call Classification**: Different call types for optimization
- **Desugared Structure**: Complex expressions broken down

### 3. HIR Control Flow

```rust
#[derive(Debug, Clone)]
pub enum HIRStatement {
    Expression(HIRExpression),
    Let(HIRLet),
    Assign(HIRAssign),
    Block(HIRBlock),
    If(HIRIf),
    While(HIRWhile),
    For(HIRFor),
    Return(HIRReturn),
    Break(HIRBreak),
    Continue(HIRContinue),
}

#[derive(Debug, Clone)]
pub struct HIRIf {
    pub node_id: NodeId,
    pub span: Span,
    pub condition: Box<HIRExpression>,
    pub then_block: HIRBlock,
    pub else_block: Option<HIRBlock>,
    pub result_type: Type,  // For if expressions
}

#[derive(Debug, Clone)]
pub struct HIRWhile {
    pub node_id: NodeId,
    pub span: Span,
    pub condition: Box<HIRExpression>,
    pub body: HIRBlock,
    pub loop_id: LoopId,  // For break/continue targeting
}

#[derive(Debug, Clone)]
pub struct HIRFor {
    pub node_id: NodeId,
    pub span: Span,
    pub variable: Symbol,
    pub iterable: Box<HIRExpression>,
    pub body: HIRBlock,
    pub loop_id: LoopId,
    pub iteration_kind: IterationKind,
}

#[derive(Debug, Clone)]
pub enum IterationKind {
    Range { start: Box<HIRExpression>, end: Box<HIRExpression>, step: Option<Box<HIRExpression>> },
    Collection { collection: Box<HIRExpression> },
    Iterator { iterator: Box<HIRExpression> },
}
```

## Mid-Level Intermediate Representation (MIR)

### 1. MIR Structure - Control Flow Graph

```rust
/// MIR representation using Control Flow Graph
#[derive(Debug, Clone)]
pub struct MIRFunction {
    pub node_id: NodeId,
    pub name: Symbol,
    pub signature: FunctionSignature,
    pub basic_blocks: Arena<BasicBlock>,
    pub entry_block: BlockId,
    pub local_variables: Arena<LocalVariable>,
    pub debug_info: DebugInfo,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub statements: Vec<MIRStatement>,
    pub terminator: MIRTerminator,
    pub predecessors: Vec<BlockId>,
    pub successors: Vec<BlockId>,
}

#[derive(Debug, Clone)]
pub enum MIRStatement {
    Assign(MIRAssign),
    Call(MIRCall),
    Drop(MIRDrop),
    StorageLive(LocalId),
    StorageDead(LocalId),
    Nop,
}

#[derive(Debug, Clone)]
pub struct MIRAssign {
    pub target: MIRPlace,
    pub source: MIRRValue,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum MIRTerminator {
    Return(Option<MIROperand>),
    Goto(BlockId),
    If { condition: MIROperand, true_block: BlockId, false_block: BlockId },
    Switch { discriminant: MIROperand, targets: Vec<(u128, BlockId)>, otherwise: BlockId },
    Call { function: MIROperand, args: Vec<MIROperand>, destination: MIRPlace, target: Option<BlockId> },
    Unreachable,
}
```

**MIR Design Benefits:**
- **Control Flow Graph**: Explicit representation of program flow
- **Static Single Assignment**: Each variable assigned exactly once
- **Memory Operations**: Explicit storage lifetime management
- **Optimization Friendly**: Designed for analysis and transformation

### 2. MIR Data Structures

```rust
#[derive(Debug, Clone)]
pub enum MIRRValue {
    Use(MIROperand),
    Repeat(MIROperand, u64),           // [value; count]
    Ref(BorrowKind, MIRPlace),         // &place or &mut place
    Aggregate(AggregateKind, Vec<MIROperand>),  // Structs, arrays
    BinaryOp(BinOp, MIROperand, MIROperand),
    UnaryOp(UnOp, MIROperand),
    Cast(CastKind, MIROperand, Type),
    Discriminant(MIRPlace),
}

#[derive(Debug, Clone)]
pub enum MIROperand {
    Copy(MIRPlace),
    Move(MIRPlace),
    Constant(MIRConstant),
}

#[derive(Debug, Clone)]
pub struct MIRPlace {
    pub local: LocalId,
    pub projection: Vec<MIRProjection>,
}

#[derive(Debug, Clone)]
pub enum MIRProjection {
    Deref,                    // *place
    Field(FieldId),          // place.field
    Index(LocalId),          // place[local]
    ConstantIndex { offset: u64, from_end: bool },
}

#[derive(Debug, Clone)]
pub struct MIRConstant {
    pub value: ConstValue,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ConstValue {
    Scalar(ScalarValue),
    List(Vec<ConstValue>),
    Struct(Vec<ConstValue>),
    Function(Symbol),
}
```

**MIR Value System:**
- **Places**: Memory locations (variables, fields, indices)
- **RValues**: Values that can be assigned to places
- **Operands**: Values used in operations (copy, move, constant)
- **Constants**: Compile-time known values

### 3. MIR Type System

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Bool,
    Int(IntType),
    Float(FloatType),
    String,
    List(Box<Type>),
    Matrix(Box<Type>),
    Function(FunctionType),
    Class(ClassId),
    DataType(DataTypeId),
    Tuple(Vec<Type>),
    Any,      // Dynamic type for JSON values and runtime-typed data
    Never,    // For functions that never return
    Unit,     // For void/empty return
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionType {
    pub parameters: Vec<Type>,
    pub return_type: Box<Type>,
    pub is_async: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntType {
    I8, I16, I32, I64,
    U8, U16, U32, U64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloatType {
    F32, F64,
}
```

### 4. Dynamic Type Operations

The `Any` type supports dynamic access operations for JSON values and runtime-typed data. These operations are represented as specific MIR operations that are lowered to runtime function calls.

```rust
#[derive(Debug, Clone)]
pub enum MirOperation {
    // ... other operations ...

    /// Access a field on an Any (JSON object) value by string key
    /// Generates a call to __json_get_field runtime function
    AnyGetField {
        object: MirOperand,  // The Any value (JSON object pointer)
        key: MirOperand,     // String key for field access
    },

    /// Access an element on an Any (JSON array) value by integer index
    /// Generates a call to __json_get_index runtime function
    AnyGetIndex {
        array: MirOperand,   // The Any value (JSON array pointer)
        index: MirOperand,   // Integer index for array access
    },
}
```

**Dynamic Access Semantics:**
- `AnyGetField`: Accesses a field by string key, returns `Any` (or null if field doesn't exist)
- `AnyGetIndex`: Accesses an element by integer index, returns `Any` (or null if out of bounds)
- Both operations handle null input gracefully, returning null
- Chained access is supported through successive operations

**Code Generation:**
```rust
// AnyGetField generates:
//   1. Load object operand (i32 pointer)
//   2. Load key operand (i32 string pointer)
//   3. Call __json_get_field(object, key) -> i32
//   4. Store result

// AnyGetIndex generates:
//   1. Load array operand (i32 pointer)
//   2. Load index operand (i32)
//   3. Call __json_get_index(array, index) -> i32
//   4. Store result
```

## Low-Level Intermediate Representation (LIR)

### 1. WebAssembly-Ready LIR (`src/lir/mod.rs`)

```rust
/// LIR designed for direct WebAssembly translation
#[derive(Debug, Clone)]
pub struct LIRFunction {
    pub name: Symbol,
    pub signature: WasmFunctionSignature,
    pub locals: Vec<WasmType>,
    pub body: Vec<LIRInstruction>,
    pub debug_info: Option<WasmDebugInfo>,
}

#[derive(Debug, Clone)]
pub struct WasmFunctionSignature {
    pub parameters: Vec<WasmType>,
    pub return_type: Option<WasmType>,
}

#[derive(Debug, Clone)]
pub enum LIRInstruction {
    // Control flow
    Block(WasmType),
    Loop(WasmType),
    If(WasmType),
    Else,
    End,
    Br(u32),
    BrIf(u32),
    BrTable(Vec<u32>, u32),
    Return,
    
    // Variable access
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(u32),
    GlobalSet(u32),
    
    // Memory access
    Load(MemoryOperation),
    Store(MemoryOperation),
    MemorySize,
    MemoryGrow,
    
    // Constants
    I32Const(i32),
    I64Const(i64),
    F32Const(f32),
    F64Const(f64),
    
    // Arithmetic
    I32Add, I32Sub, I32Mul, I32DivS, I32DivU,
    I32RemS, I32RemU, I32And, I32Or, I32Xor,
    F64Add, F64Sub, F64Mul, F64Div,
    
    // Comparison
    I32Eq, I32Ne, I32LtS, I32LtU, I32GtS, I32GtU,
    I32LeS, I32LeU, I32GeS, I32GeU,
    F64Eq, F64Ne, F64Lt, F64Gt, F64Le, F64Ge,
    
    // Conversion
    I32WrapI64, I64ExtendI32S, I64ExtendI32U,
    F32DemoteF64, F64PromoteF32,
    I32TruncF32S, I32TruncF32U, I32TruncF64S, I32TruncF64U,
    I64TruncF32S, I64TruncF32U, I64TruncF64S, I64TruncF64U,
    F32ConvertI32S, F32ConvertI32U, F32ConvertI64S, F32ConvertI64U,
    F64ConvertI32S, F64ConvertI32U, F64ConvertI64S, F64ConvertI64U,
    
    // Function calls
    Call(u32),
    CallIndirect(u32),
    
    // Special operations
    Drop,
    Select,
    Unreachable,
    Nop,
    
    // Clean Language specific
    StringConcat,
    StringCompare,
    ListCreate,
    ListPush,
    ListGet,
    ObjectNew,
    ObjectGetField,
    ObjectSetField,
    MemoryAlloc,
    MemoryFree,
    GarbageCollect,
}

#[derive(Debug, Clone)]
pub struct MemoryOperation {
    pub offset: u32,
    pub align: u32,
    pub memory_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WasmType {
    I32, I64, F32, F64,
}
```

**LIR Characteristics:**
- **WebAssembly Mapping**: Direct correspondence to WASM instructions
- **Type Explicit**: All operations have explicit WebAssembly types
- **Memory Model**: Explicit memory operations and management
- **Standard Library**: Built-in operations for Clean Language features

### 2. LIR Optimization Opportunities

```rust
/// LIR-specific optimizations
pub struct LIROptimizer {
    pub dead_code_elimination: bool,
    pub constant_folding: bool,
    pub instruction_combining: bool,
    pub local_variable_reuse: bool,
}

impl LIROptimizer {
    pub fn optimize_function(&self, function: &mut LIRFunction) -> bool {
        let mut changed = false;
        
        if self.dead_code_elimination {
            changed |= self.eliminate_dead_code(function);
        }
        
        if self.constant_folding {
            changed |= self.fold_constants(function);
        }
        
        if self.instruction_combining {
            changed |= self.combine_instructions(function);
        }
        
        if self.local_variable_reuse {
            changed |= self.reuse_locals(function);
        }
        
        changed
    }
    
    fn eliminate_dead_code(&self, function: &mut LIRFunction) -> bool {
        // Remove unreachable instructions after unconditional branches
        let mut changed = false;
        let mut reachable = vec![true; function.body.len()];
        let mut i = 0;
        
        while i < function.body.len() {
            match &function.body[i] {
                LIRInstruction::Br(_) | LIRInstruction::Return => {
                    // Mark subsequent instructions as unreachable until next label
                    i += 1;
                    while i < function.body.len() {
                        match &function.body[i] {
                            LIRInstruction::Block(_) | LIRInstruction::Loop(_) => break,
                            _ => {
                                reachable[i] = false;
                                i += 1;
                            }
                        }
                    }
                    changed = true;
                }
                _ => i += 1,
            }
        }
        
        // Remove unreachable instructions
        function.body = function.body.iter().enumerate()
            .filter(|(i, _)| reachable[*i])
            .map(|(_, inst)| inst.clone())
            .collect();
        
        changed
    }
    
    fn fold_constants(&self, function: &mut LIRFunction) -> bool {
        let mut changed = false;
        let mut stack: Vec<LIRInstruction> = Vec::new();
        let mut result: Vec<LIRInstruction> = Vec::new();
        
        for instruction in &function.body {
            match instruction {
                LIRInstruction::I32Const(a) => {
                    stack.push(instruction.clone());
                }
                LIRInstruction::I32Add => {
                    if let (Some(LIRInstruction::I32Const(b)), Some(LIRInstruction::I32Const(a))) = 
                        (stack.pop(), stack.pop()) {
                        stack.push(LIRInstruction::I32Const(a + b));
                        changed = true;
                    } else {
                        // Can't fold, emit all pending instructions
                        result.extend(stack.drain(..));
                        result.push(instruction.clone());
                    }
                }
                _ => {
                    result.extend(stack.drain(..));
                    result.push(instruction.clone());
                }
            }
        }
        
        result.extend(stack);
        function.body = result;
        changed
    }
}
```

## Optimization Pipeline

### 1. Optimization Pass Architecture

```rust
/// Optimization pass trait for all IR levels
pub trait OptimizationPass {
    type Input;
    type Output;
    
    fn name(&self) -> &'static str;
    fn run(&mut self, input: Self::Input) -> Result<Self::Output, CompilerError>;
    fn is_analysis_pass(&self) -> bool { false }
}

/// Pass manager for orchestrating optimizations
pub struct PassManager<T> {
    passes: Vec<Box<dyn OptimizationPass<Input = T, Output = T>>>,
    config: OptimizationConfig,
}

#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    pub level: OptimizationLevel,
    pub passes: PassSelection,
    pub debug: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OptimizationLevel {
    None,      // -O0: No optimizations
    Basic,     // -O1: Basic optimizations
    Default,   // -O2: Default optimizations
    Aggressive, // -O3: Aggressive optimizations
    Size,      // -Os: Optimize for size
}

#[derive(Debug, Clone)]
pub enum PassSelection {
    Default,
    Custom(Vec<String>),
    All,
}

impl<T> PassManager<T> {
    pub fn new(config: OptimizationConfig) -> Self {
        let mut passes: Vec<Box<dyn OptimizationPass<Input = T, Output = T>>> = Vec::new();
        
        match config.level {
            OptimizationLevel::None => {
                // No optimization passes
            }
            OptimizationLevel::Basic => {
                passes.push(Box::new(DeadCodeEliminationPass::new()));
                passes.push(Box::new(ConstantFoldingPass::new()));
            }
            OptimizationLevel::Default => {
                passes.push(Box::new(DeadCodeEliminationPass::new()));
                passes.push(Box::new(ConstantFoldingPass::new()));
                passes.push(Box::new(ConstantPropagationPass::new()));
                passes.push(Box::new(InliningPass::new()));
                passes.push(Box::new(LoopOptimizationPass::new()));
            }
            OptimizationLevel::Aggressive => {
                passes.push(Box::new(DeadCodeEliminationPass::new()));
                passes.push(Box::new(ConstantFoldingPass::new()));
                passes.push(Box::new(ConstantPropagationPass::new()));
                passes.push(Box::new(InliningPass::new()));
                passes.push(Box::new(LoopOptimizationPass::new()));
                passes.push(Box::new(LoopUnrollingPass::new()));
                passes.push(Box::new(FunctionSpecializationPass::new()));
            }
            OptimizationLevel::Size => {
                passes.push(Box::new(DeadCodeEliminationPass::new()));
                passes.push(Box::new(ConstantFoldingPass::new()));
                // Size-focused passes only
            }
        }
        
        Self { passes, config }
    }
    
    pub fn run(&mut self, mut input: T) -> Result<T, CompilerError> {
        for pass in &mut self.passes {
            if self.config.debug {
                eprintln!("Running optimization pass: {}", pass.name());
            }
            input = pass.run(input)?;
        }
        Ok(input)
    }
}
```

### 2. Specific Optimization Passes

```rust
/// Dead code elimination pass
pub struct DeadCodeEliminationPass {
    removed_count: usize,
}

impl OptimizationPass for DeadCodeEliminationPass {
    type Input = MIRFunction;
    type Output = MIRFunction;
    
    fn name(&self) -> &'static str {
        "dead-code-elimination"
    }
    
    fn run(&mut self, mut function: MIRFunction) -> Result<MIRFunction, CompilerError> {
        let mut changed = true;
        self.removed_count = 0;
        
        while changed {
            changed = false;
            
            // Find reachable basic blocks
            let mut reachable = vec![false; function.basic_blocks.len()];
            let mut worklist = vec![function.entry_block];
            
            while let Some(block_id) = worklist.pop() {
                if reachable[block_id.index()] {
                    continue;
                }
                reachable[block_id.index()] = true;
                
                let block = &function.basic_blocks[block_id];
                for &successor in &block.successors {
                    if !reachable[successor.index()] {
                        worklist.push(successor);
                    }
                }
            }
            
            // Remove unreachable blocks
            let old_count = function.basic_blocks.len();
            function.basic_blocks.retain(|id, _| reachable[id.index()]);
            
            if function.basic_blocks.len() < old_count {
                changed = true;
                self.removed_count += old_count - function.basic_blocks.len();
            }
        }
        
        Ok(function)
    }
}

/// Constant folding and propagation pass
pub struct ConstantPropagationPass {
    constants: HashMap<LocalId, ConstValue>,
}

impl OptimizationPass for ConstantPropagationPass {
    type Input = MIRFunction;
    type Output = MIRFunction;
    
    fn name(&self) -> &'static str {
        "constant-propagation"
    }
    
    fn run(&mut self, mut function: MIRFunction) -> Result<MIRFunction, CompilerError> {
        self.constants.clear();
        
        // Forward propagation pass
        for (block_id, block) in function.basic_blocks.iter_mut() {
            for statement in &mut block.statements {
                match statement {
                    MIRStatement::Assign(assign) => {
                        match &assign.source {
                            MIRRValue::Use(MIROperand::Constant(constant)) => {
                                if let MIRPlace { local, projection } = &assign.target {
                                    if projection.is_empty() {
                                        self.constants.insert(*local, constant.value.clone());
                                    }
                                }
                            }
                            MIRRValue::BinaryOp(op, left, right) => {
                                if let (Some(left_val), Some(right_val)) = 
                                    (self.get_constant_operand(left), self.get_constant_operand(right)) {
                                    if let Some(result) = self.fold_binary_op(op, &left_val, &right_val) {
                                        assign.source = MIRRValue::Use(MIROperand::Constant(MIRConstant {
                                            value: result.clone(),
                                            ty: assign.target.ty(&function).clone(),
                                            span: assign.span.clone(),
                                        }));
                                        
                                        if let MIRPlace { local, projection } = &assign.target {
                                            if projection.is_empty() {
                                                self.constants.insert(*local, result);
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        
        Ok(function)
    }
    
    fn get_constant_operand(&self, operand: &MIROperand) -> Option<&ConstValue> {
        match operand {
            MIROperand::Constant(constant) => Some(&constant.value),
            MIROperand::Copy(MIRPlace { local, projection }) |
            MIROperand::Move(MIRPlace { local, projection }) => {
                if projection.is_empty() {
                    self.constants.get(local)
                } else {
                    None
                }
            }
        }
    }
    
    fn fold_binary_op(&self, op: &BinOp, left: &ConstValue, right: &ConstValue) -> Option<ConstValue> {
        use ConstValue::Scalar;
        match (left, right) {
            (Scalar(ScalarValue::Int(a)), Scalar(ScalarValue::Int(b))) => {
                match op {
                    BinOp::Add => Some(Scalar(ScalarValue::Int(a + b))),
                    BinOp::Sub => Some(Scalar(ScalarValue::Int(a - b))),
                    BinOp::Mul => Some(Scalar(ScalarValue::Int(a * b))),
                    BinOp::Div if *b != 0 => Some(Scalar(ScalarValue::Int(a / b))),
                    BinOp::Eq => Some(Scalar(ScalarValue::Bool(a == b))),
                    BinOp::Ne => Some(Scalar(ScalarValue::Bool(a != b))),
                    BinOp::Lt => Some(Scalar(ScalarValue::Bool(a < b))),
                    BinOp::Le => Some(Scalar(ScalarValue::Bool(a <= b))),
                    BinOp::Gt => Some(Scalar(ScalarValue::Bool(a > b))),
                    BinOp::Ge => Some(Scalar(ScalarValue::Bool(a >= b))),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// Function inlining pass
pub struct InliningPass {
    inline_threshold: usize,
    inlined_count: usize,
}

impl InliningPass {
    pub fn new() -> Self {
        Self {
            inline_threshold: 50,  // Inline functions with <= 50 instructions
            inlined_count: 0,
        }
    }
    
    fn should_inline(&self, function: &MIRFunction) -> bool {
        // Simple heuristic: inline small functions
        let instruction_count = function.basic_blocks.values()
            .map(|block| block.statements.len())
            .sum::<usize>();
            
        instruction_count <= self.inline_threshold &&
        !function.signature.is_recursive &&
        !function.attributes.contains(&FunctionAttribute::NoInline)
    }
}
```

### 3. WebAssembly-Specific Optimizations

```rust
/// WebAssembly-specific optimization passes
pub struct WasmOptimizationPass {
    memory_coalescing: bool,
    stack_optimization: bool,
    import_optimization: bool,
}

impl WasmOptimizationPass {
    pub fn optimize_memory_operations(&self, function: &mut LIRFunction) -> bool {
        let mut changed = false;
        
        // Coalesce adjacent memory operations
        let mut i = 0;
        while i + 1 < function.body.len() {
            match (&function.body[i], &function.body[i + 1]) {
                (LIRInstruction::I32Const(offset1), LIRInstruction::Load(mem_op1)) 
                if mem_op1.offset == 0 => {
                    // Replace with single load with offset
                    function.body[i] = LIRInstruction::Load(MemoryOperation {
                        offset: *offset1 as u32,
                        align: mem_op1.align,
                        memory_index: mem_op1.memory_index,
                    });
                    function.body.remove(i + 1);
                    changed = true;
                }
                _ => i += 1,
            }
        }
        
        changed
    }
    
    pub fn optimize_stack_operations(&self, function: &mut LIRFunction) -> bool {
        let mut changed = false;
        
        // Eliminate redundant local.get/local.set pairs
        let mut i = 0;
        while i + 1 < function.body.len() {
            match (&function.body[i], &function.body[i + 1]) {
                (LIRInstruction::LocalSet(local1), LIRInstruction::LocalGet(local2))
                if local1 == local2 => {
                    // Replace with local.tee
                    function.body[i] = LIRInstruction::LocalTee(*local1);
                    function.body.remove(i + 1);
                    changed = true;
                }
                _ => i += 1,
            }
        }
        
        changed
    }
}
```

## IR Analysis and Validation

### 1. Control Flow Analysis

```rust
/// Control flow analysis for MIR
pub struct ControlFlowAnalysis {
    pub dominators: DominatorTree,
    pub loops: LoopInfo,
    pub post_dominators: PostDominatorTree,
}

impl ControlFlowAnalysis {
    pub fn analyze(function: &MIRFunction) -> Result<Self, CompilerError> {
        let cfg = ControlFlowGraph::from_function(function);
        
        Ok(Self {
            dominators: DominatorTree::compute(&cfg),
            loops: LoopInfo::compute(&cfg),
            post_dominators: PostDominatorTree::compute(&cfg),
        })
    }
}

#[derive(Debug)]
pub struct ControlFlowGraph {
    pub nodes: Vec<BlockId>,
    pub edges: Vec<(BlockId, BlockId)>,
    pub entry: BlockId,
    pub exits: Vec<BlockId>,
}

impl ControlFlowGraph {
    pub fn from_function(function: &MIRFunction) -> Self {
        let mut cfg = Self {
            nodes: function.basic_blocks.indices().collect(),
            edges: Vec::new(),
            entry: function.entry_block,
            exits: Vec::new(),
        };
        
        for (block_id, block) in function.basic_blocks.iter() {
            for &successor in &block.successors {
                cfg.edges.push((block_id, successor));
            }
            
            // Check if this is an exit block
            if block.successors.is_empty() {
                cfg.exits.push(block_id);
            }
        }
        
        cfg
    }
}
```

### 2. Data Flow Analysis

```rust
/// Generic data flow analysis framework
pub trait DataFlowAnalysis {
    type Domain: Clone + PartialEq;
    
    fn initial_value(&self) -> Self::Domain;
    fn transfer_function(&self, block: &BasicBlock, input: &Self::Domain) -> Self::Domain;
    fn meet_operation(&self, left: &Self::Domain, right: &Self::Domain) -> Self::Domain;
    fn is_forward(&self) -> bool { true }
}

/// Live variable analysis
pub struct LiveVariableAnalysis;

impl DataFlowAnalysis for LiveVariableAnalysis {
    type Domain = BitSet<LocalId>;
    
    fn initial_value(&self) -> Self::Domain {
        BitSet::new()
    }
    
    fn transfer_function(&self, block: &BasicBlock, input: &Self::Domain) -> Self::Domain {
        let mut result = input.clone();
        
        // Process statements in reverse order for backward analysis
        for statement in block.statements.iter().rev() {
            match statement {
                MIRStatement::Assign(assign) => {
                    // Remove variables that are defined (killed)
                    if let MIRPlace { local, projection } = &assign.target {
                        if projection.is_empty() {
                            result.remove(*local);
                        }
                    }
                    
                    // Add variables that are used (generated)
                    self.add_used_variables(&assign.source, &mut result);
                }
                MIRStatement::Call(call) => {
                    // Add all argument variables
                    for arg in &call.args {
                        self.add_used_operand(arg, &mut result);
                    }
                }
                _ => {}
            }
        }
        
        result
    }
    
    fn meet_operation(&self, left: &Self::Domain, right: &Self::Domain) -> Self::Domain {
        left.union(right)
    }
    
    fn is_forward(&self) -> bool {
        false  // Live variable analysis is a backward analysis
    }
}

/// Reaching definitions analysis
pub struct ReachingDefinitionsAnalysis;

impl DataFlowAnalysis for ReachingDefinitionsAnalysis {
    type Domain = BitSet<(LocalId, NodeId)>;  // (variable, definition site)
    
    fn initial_value(&self) -> Self::Domain {
        BitSet::new()
    }
    
    fn transfer_function(&self, block: &BasicBlock, input: &Self::Domain) -> Self::Domain {
        let mut result = input.clone();
        
        for statement in &block.statements {
            match statement {
                MIRStatement::Assign(assign) => {
                    if let MIRPlace { local, projection } = &assign.target {
                        if projection.is_empty() {
                            // Kill all previous definitions of this variable
                            result.retain(|(var, _)| var != local);
                            
                            // Add this definition
                            result.insert((*local, statement.node_id()));
                        }
                    }
                }
                _ => {}
            }
        }
        
        result
    }
    
    fn meet_operation(&self, left: &Self::Domain, right: &Self::Domain) -> Self::Domain {
        left.union(right)
    }
}
```

### 3. IR Validation

```rust
/// Validates IR invariants at each level
pub struct IRValidator {
    errors: Vec<CompilerError>,
}

impl IRValidator {
    pub fn validate_mir_function(&mut self, function: &MIRFunction) -> Result<(), Vec<CompilerError>> {
        self.errors.clear();
        
        // Check that entry block exists
        if !function.basic_blocks.contains(function.entry_block) {
            self.errors.push(CompilerError::InvalidEntryBlock {
                function: function.name.clone(),
                block_id: function.entry_block,
            });
        }
        
        // Validate each basic block
        for (block_id, block) in function.basic_blocks.iter() {
            self.validate_basic_block(block_id, block, function);
        }
        
        // Check control flow consistency
        self.validate_control_flow(function);
        
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }
    
    fn validate_basic_block(&mut self, block_id: BlockId, block: &BasicBlock, function: &MIRFunction) {
        // Check that all predecessors/successors are valid
        for &pred in &block.predecessors {
            if !function.basic_blocks.contains(pred) {
                self.errors.push(CompilerError::InvalidPredecessor {
                    block: block_id,
                    predecessor: pred,
                });
            }
        }
        
        for &succ in &block.successors {
            if !function.basic_blocks.contains(succ) {
                self.errors.push(CompilerError::InvalidSuccessor {
                    block: block_id,
                    successor: succ,
                });
            }
        }
        
        // Validate terminator consistency
        let expected_successors = self.get_terminator_successors(&block.terminator);
        if expected_successors != block.successors {
            self.errors.push(CompilerError::InconsistentSuccessors {
                block: block_id,
                expected: expected_successors,
                actual: block.successors.clone(),
            });
        }
    }
    
    fn get_terminator_successors(&self, terminator: &MIRTerminator) -> Vec<BlockId> {
        match terminator {
            MIRTerminator::Return(_) => vec![],
            MIRTerminator::Goto(target) => vec![*target],
            MIRTerminator::If { true_block, false_block, .. } => vec![*true_block, *false_block],
            MIRTerminator::Switch { targets, otherwise, .. } => {
                let mut succs: Vec<BlockId> = targets.iter().map(|(_, block)| *block).collect();
                succs.push(*otherwise);
                succs.sort();
                succs.dedup();
                succs
            }
            MIRTerminator::Call { target: Some(target), .. } => vec![*target],
            MIRTerminator::Call { target: None, .. } => vec![],
            MIRTerminator::Unreachable => vec![],
        }
    }
}
```

## Integration with Compiler Pipeline

### 1. IR Lowering Pipeline

```rust
/// Coordinates IR transformations through the pipeline
pub struct IRPipeline {
    pub optimization_config: OptimizationConfig,
    pub target_config: TargetConfig,
}

impl IRPipeline {
    pub fn compile_function(&mut self, ast_function: ASTFunction) -> Result<LIRFunction, CompilerError> {
        // AST -> HIR
        let hir_function = self.lower_ast_to_hir(ast_function)?;
        
        // HIR -> MIR
        let mir_function = self.lower_hir_to_mir(hir_function)?;
        
        // MIR optimizations
        let optimized_mir = self.optimize_mir(mir_function)?;
        
        // MIR -> LIR
        let lir_function = self.lower_mir_to_lir(optimized_mir)?;
        
        // LIR optimizations
        let optimized_lir = self.optimize_lir(lir_function)?;
        
        Ok(optimized_lir)
    }
    
    fn optimize_mir(&mut self, function: MIRFunction) -> Result<MIRFunction, CompilerError> {
        let mut pass_manager = PassManager::new(self.optimization_config.clone());
        pass_manager.run(function)
    }
    
    fn optimize_lir(&mut self, function: LIRFunction) -> Result<LIRFunction, CompilerError> {
        let wasm_optimizer = WasmOptimizationPass {
            memory_coalescing: true,
            stack_optimization: true,
            import_optimization: true,
        };
        
        let mut optimized = function;
        wasm_optimizer.optimize_memory_operations(&mut optimized);
        wasm_optimizer.optimize_stack_operations(&mut optimized);
        
        Ok(optimized)
    }
}
```

### 2. Debug Information Preservation

```rust
/// Debug information maintained through IR transformations
#[derive(Debug, Clone)]
pub struct DebugInfo {
    pub source_map: SourceMap,
    pub variable_names: HashMap<LocalId, String>,
    pub line_table: LineTable,
    pub scope_tree: ScopeTree,
}

#[derive(Debug, Clone)]
pub struct SourceMap {
    pub mappings: Vec<SourceMapping>,
}

#[derive(Debug, Clone)]
pub struct SourceMapping {
    pub generated_offset: u32,
    pub source_location: Span,
    pub symbol: Option<Symbol>,
}

impl DebugInfo {
    pub fn preserve_through_optimization(&mut self, transformation: &dyn IRTransform) {
        // Update source mappings based on transformation
        for mapping in &mut self.source_map.mappings {
            if let Some(new_location) = transformation.map_source_location(&mapping.source_location) {
                mapping.source_location = new_location;
            }
        }
        
        // Update variable name mappings
        let mut new_variable_names = HashMap::new();
        for (local_id, name) in &self.variable_names {
            if let Some(new_local_id) = transformation.map_local_id(*local_id) {
                new_variable_names.insert(new_local_id, name.clone());
            }
        }
        self.variable_names = new_variable_names;
    }
}
```

## Best Practices for Claude

When working with Clean Language's IR system:

1. **IR Level Selection**: Choose the appropriate IR level for your task
   - AST: Syntax analysis and transformation
   - HIR: Semantic analysis and type checking  
   - MIR: Control flow analysis and optimization
   - LIR: WebAssembly generation and low-level optimization

2. **Optimization Strategy**: Balance compilation speed vs. optimization quality
   - Use appropriate optimization levels
   - Consider compilation targets (debug vs. release)
   - Profile optimization passes for effectiveness

3. **Type Safety**: Maintain type information throughout transformations
   - Validate type consistency after optimizations
   - Use type-guided transformations when possible

4. **Debug Information**: Preserve debug information through optimizations
   - Map source locations through transformations
   - Maintain variable name mappings

5. **Validation**: Validate IR invariants at each level
   - Check control flow consistency
   - Verify type constraints
   - Validate memory safety properties

6. **Performance**: Consider compilation performance impact
   - Use efficient data structures (Arena, BitSet)
   - Avoid unnecessary copying of large IR structures
   - Cache analysis results when possible

This IR documentation provides the foundation for understanding and extending Clean Language's intermediate representation system and optimization pipeline.