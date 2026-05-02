//! Module for generating WebAssembly instructions.

use crate::ast::{self, BinaryOperator, Expression, StringPart, Value};
use crate::error::CompilerError;
use crate::types::WasmType;
use wasm_encoder::{Instruction, MemArg, ValType};

// Removed unused import DEFAULT_ALIGN

use super::type_manager::TypeManager;

/// Represents a local variable in a function
#[derive(Debug, Clone)]
pub struct LocalVarInfo {
    pub index: u32,
    pub type_: wasm_encoder::ValType,
}

/// Function type descriptor used for return-type lookup during code generation.
#[derive(Clone)]
pub struct FuncType {
    results: Vec<ValType>,
}

impl FuncType {
    pub fn new(_params: Vec<ValType>, results: Vec<ValType>) -> Self {
        Self { results }
    }

    pub fn results(&self) -> &[ValType] {
        &self.results
    }
}

/// Generates WebAssembly instructions for various language constructs
pub(crate) struct InstructionGenerator {
    #[allow(dead_code)]
    type_manager: TypeManager,
    variable_map: std::collections::HashMap<String, LocalVarInfo>,
    current_locals: Vec<LocalVarInfo>,
    function_map: std::collections::HashMap<String, u32>, // Simple name -> primary index mapping
    function_signatures: std::collections::HashMap<String, u32>, // signature -> index mapping
    function_types: std::collections::HashMap<u32, FuncType>,
}

impl InstructionGenerator {
    /// Create a new instruction generator
    pub(crate) fn new(type_manager: TypeManager) -> Self {
        Self {
            type_manager,
            variable_map: std::collections::HashMap::new(),
            current_locals: Vec::new(),
            function_map: std::collections::HashMap::new(),
            function_signatures: std::collections::HashMap::new(),
            function_types: std::collections::HashMap::new(),
        }
    }

    /// Add a function mapping from name to index
    pub(crate) fn add_function_mapping(&mut self, name: &str, index: u32) {
        self.function_map.insert(name.to_string(), index);
    }

    /// Reset locals for a new function
    pub(crate) fn reset_locals(&mut self) {
        self.current_locals.clear();
        self.variable_map.clear();
    }

    /// Get a function index by name (returns the first/primary overload)
    pub(crate) fn get_function_index(&self, name: &str) -> Option<u32> {
        self.function_map.get(name).copied()
    }

    /// Get a function index by signature (for overload resolution)
    pub(crate) fn get_function_index_by_signature(
        &self,
        name: &str,
        param_types: &[WasmType],
    ) -> Option<u32> {
        let signature = self.create_function_signature(name, param_types);
        self.function_signatures.get(&signature).copied()
    }

    /// Create a function signature string for overload resolution
    fn create_function_signature(&self, name: &str, param_types: &[WasmType]) -> String {
        let param_str = param_types
            .iter()
            .map(|t| format!("{t:?}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{name}({param_str})")
    }

    /// Determine the type of an expression without generating instructions
    pub(crate) fn get_expression_type(&self, expr: &Expression) -> Result<WasmType, CompilerError> {
        match expr {
            Expression::Literal(value) => {
                match value {
                    ast::Value::Integer(_) => Ok(WasmType::I32),
                    ast::Value::Number(_) => Ok(WasmType::F64),
                    ast::Value::String(_) => Ok(WasmType::I32), // String pointer
                    ast::Value::Boolean(_) => Ok(WasmType::I32),
                    _ => Ok(WasmType::I32), // Default
                }
            }
            Expression::Variable(name) => {
                // Look up variable type from local variables
                if let Some(local_info) = self.find_local(name) {
                    let wasm_type = match local_info.type_ {
                        wasm_encoder::ValType::I32 => WasmType::I32,
                        wasm_encoder::ValType::I64 => WasmType::I64,
                        wasm_encoder::ValType::F32 => WasmType::F32,
                        wasm_encoder::ValType::F64 => WasmType::F64,
                        wasm_encoder::ValType::V128 => WasmType::V128,
                        _ => WasmType::I32,
                    };
                    Ok(wasm_type)
                } else {
                    // Default to I32 if we can't determine the type
                    Ok(WasmType::I32)
                }
            }
            Expression::Call(func_name, _args) => {
                // Get function return type by looking up the function
                if let Some(func_index) = self.get_function_index(func_name) {
                    if let Some(func_type) = self.get_function_type(func_index) {
                        if let Some(return_val_type) = func_type.results().first() {
                            Ok(Self::from_parser_val_type(*return_val_type))
                        } else {
                            Ok(WasmType::Unit)
                        }
                    } else {
                        Ok(WasmType::I32)
                    }
                } else {
                    Ok(WasmType::I32) // Default
                }
            }
            Expression::Binary(_, op, _) => {
                // Most binary operations return the same type as their operands
                // For simplicity, assume numeric operations
                match op {
                    ast::BinaryOperator::Equal
                    | ast::BinaryOperator::NotEqual
                    | ast::BinaryOperator::Less
                    | ast::BinaryOperator::Greater
                    | ast::BinaryOperator::LessEqual
                    | ast::BinaryOperator::GreaterEqual => {
                        Ok(WasmType::I32) // Boolean result
                    }
                    _ => Ok(WasmType::F64), // Most math operations use F64
                }
            }
            _ => Ok(WasmType::I32), // Default for other expression types
        }
    }

    /// Find a local variable by name
    pub(crate) fn find_local(&self, name: &str) -> Option<&LocalVarInfo> {
        self.variable_map.get(name)
    }

    /// Add a parameter to the list of locals
    pub(crate) fn add_parameter(&mut self, name: &str, wasm_type: WasmType) {
        let local_info = LocalVarInfo {
            index: self.current_locals.len() as u32,
            type_: wasm_type.into(),
        };
        self.current_locals.push(local_info.clone());
        self.variable_map.insert(name.to_string(), local_info);
    }
    /// Generate instructions for an expression
    pub(crate) fn generate_expression(
        &mut self,
        expr: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Extract location if available, or use None
        let loc = match expr {
            Expression::Binary(_, _, _) => None, // Binary has no location field
            // Add other expression variants with locations as needed
            _ => None,
        };

        match expr {
            Expression::Literal(value) => self.generate_value(value, instructions),
            Expression::Variable(name) => {
                let local = self.find_local(name).ok_or_else(|| {
                    CompilerError::codegen_error(
                        format!("Undefined variable '{name}'"),
                        Some("Check if the variable is defined".to_string()),
                        None,
                    )
                })?;
                instructions.push(Instruction::LocalGet(local.index));
                Ok(Self::from_parser_val_type(local.type_))
            }
            Expression::Binary(left, op, right) => {
                let left_type = self.generate_expression(left, instructions)?;
                let right_type = self.generate_expression(right, instructions)?;

                if left_type != right_type {
                    return Err(CompilerError::codegen_error(
                        format!(
                            "Type mismatch in binary operation: {left_type:?} and {right_type:?}"
                        ),
                        Some("Use operands of the same type".to_string()),
                        None,
                    ));
                }

                match (left_type, op) {
                    (WasmType::I32, BinaryOperator::Add) => {
                        instructions.push(Instruction::I32Add);
                        Ok(WasmType::I32)
                    }
                    // Add more operators as needed
                    _ => Err(CompilerError::codegen_error(
                        format!("Unsupported binary operation: {op:?} for type {left_type:?}"),
                        Some("Check operand types".to_string()),
                        None,
                    )),
                }
            }
            Expression::Call(func_name, args) => {
                // First, determine argument types for signature-based function resolution
                let mut arg_types = Vec::new();
                for arg in args {
                    let arg_type = self.get_expression_type(arg)?;
                    arg_types.push(arg_type);
                }

                // Try signature-based function resolution first, fall back to name-based resolution
                let func_index = if let Some(index) =
                    self.get_function_index_by_signature(func_name, &arg_types)
                {
                    Some(index)
                } else {
                    // Fall back to name-based resolution for backwards compatibility
                    self.get_function_index(func_name)
                };

                if let Some(func_index) = func_index {
                    // Generate argument instructions
                    for arg in args {
                        self.generate_expression(arg, instructions)?;
                    }
                    instructions.push(Instruction::Call(func_index));

                    // Determine return type
                    if let Some(func_type) = self.get_function_type(func_index) {
                        if let Some(return_val_type) = func_type.results().first() {
                            Ok(Self::from_parser_val_type(*return_val_type))
                        } else {
                            Ok(WasmType::I32) // Default to I32 if no return type
                        }
                    } else {
                        Ok(WasmType::I32) // Default to I32 if type info not available
                    }
                } else {
                    Err(CompilerError::codegen_error(
                        format!("Function not found: {func_name}"),
                        None,
                        None,
                    ))
                }
            }
            Expression::ListAccess(list, index) => {
                self.generate_list_access(list, index, instructions)
            }
            Expression::MatrixAccess(matrix, row, col) => {
                self.generate_expression(matrix, instructions)?;
                self.generate_expression(row, instructions)?;
                self.generate_expression(col, instructions)?;

                if let Some(matrix_get_index) = self.get_function_index("matrix_get") {
                    instructions.push(Instruction::Call(matrix_get_index));
                    Ok(WasmType::F64)
                } else {
                    Err(CompilerError::codegen_error(
                        "matrix_get function not found",
                        None,
                        loc.clone(),
                    ))
                }
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
                location: _,
            } => {
                // Handle matrix methods like transpose(), inverse(), etc.
                self.generate_expression(object, instructions)?;

                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                if let Some(method_index) = self.get_function_index(&format!("matrix_{method}")) {
                    instructions.push(Instruction::Call(method_index));
                    Ok(WasmType::I32) // Method calls return appropriate type
                } else {
                    Err(CompilerError::codegen_error(
                        format!("Method '{method}' not found"),
                        None,
                        None,
                    ))
                }
            }
            Expression::StringInterpolation(parts) => {
                self.generate_string_interpolation(parts, instructions)?;
                Ok(WasmType::I32) // String interpolation returns a string pointer
            }
            _ => Err(CompilerError::codegen_error(
                format!("Unsupported expression: {expr:?}"),
                Some("This expression type is not yet implemented".to_string()),
                None,
            )),
        }
    }

    /// Generate instructions for a value
    pub(crate) fn generate_value(
        &mut self,
        value: &Value,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        match value {
            Value::Number(f) => {
                instructions.push(Instruction::F64Const(*f));
                Ok(WasmType::F64)
            }
            Value::Integer(i) => {
                instructions.push(Instruction::I32Const((*i).try_into().unwrap()));
                Ok(WasmType::I32)
            }
            Value::String(_s) => {
                // This should use memory.allocate_string
                // For now, just return a placeholder pointing to "empty string"
                instructions.push(Instruction::I32Const(0));
                Ok(WasmType::I32)
            }
            Value::Boolean(b) => {
                instructions.push(Instruction::I32Const(if *b { 1 } else { 0 }));
                Ok(WasmType::I32)
            }
            Value::Matrix(_) => {
                // This should use memory.allocate_matrix
                // For now, just return a placeholder pointing to "empty matrix"
                instructions.push(Instruction::I32Const(0));
                Ok(WasmType::I32)
            }
            Value::None => {
                // None represents absence of value - push 0 as null pointer
                instructions.push(Instruction::I32Const(0));
                Ok(WasmType::I32)
            }
            Value::Void => {
                instructions.push(Instruction::I32Const(0));
                Ok(WasmType::I32)
            }
            // Sized integer types
            Value::Integer8(i) => {
                instructions.push(Instruction::I32Const(*i as i32));
                Ok(WasmType::I32)
            }
            Value::Integer8u(u) => {
                instructions.push(Instruction::I32Const(*u as i32));
                Ok(WasmType::I32)
            }
            Value::Integer16(i) => {
                instructions.push(Instruction::I32Const(*i as i32));
                Ok(WasmType::I32)
            }
            Value::Integer16u(u) => {
                instructions.push(Instruction::I32Const(*u as i32));
                Ok(WasmType::I32)
            }
            Value::Integer32(i) => {
                instructions.push(Instruction::I32Const(*i));
                Ok(WasmType::I32)
            }
            Value::Integer64(l) => {
                instructions.push(Instruction::I64Const(*l));
                Ok(WasmType::I64)
            }
            // Sized float types
            Value::Number32(f) => {
                instructions.push(Instruction::F32Const(*f));
                Ok(WasmType::F32)
            }
            Value::Number64(f) => {
                instructions.push(Instruction::F64Const(*f));
                Ok(WasmType::F64)
            }
            Value::List(items) => {
                // Implement real list literal generation
                // 1. Allocate memory for the list structure
                // 2. Store list metadata (length, capacity, behavior)
                // 3. Store each item in the list
                // 4. Return pointer to the list

                let list_length = items.len() as i32;

                // For now, implement a simple list as: [length][item1][item2]...[itemN]
                // Each item is 4 bytes (I32), plus 4 bytes for length = (n+1)*4 bytes total
                let list_size = (list_length + 1) * 4;

                // Allocate memory for the list
                if let Some(alloc_fn) = self.get_function_index("allocate_memory") {
                    instructions.push(Instruction::I32Const(list_size));
                    instructions.push(Instruction::Call(alloc_fn));

                    // Store the list pointer in a local variable
                    let list_ptr_local = self.current_locals.len() as u32;
                    self.current_locals.push(LocalVarInfo {
                        index: list_ptr_local,
                        type_: wasm_encoder::ValType::I32,
                    });
                    instructions.push(Instruction::LocalSet(list_ptr_local));

                    // Store the length at the beginning of the list
                    instructions.push(Instruction::LocalGet(list_ptr_local));
                    instructions.push(Instruction::I32Const(list_length));
                    instructions.push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));

                    // Store each item in the list
                    for (i, item) in items.iter().enumerate() {
                        instructions.push(Instruction::LocalGet(list_ptr_local));

                        // Generate the item value
                        let item_type = self.generate_value(item, instructions)?;

                        // Convert to I32 if needed (for simplicity, store everything as I32)
                        match item_type {
                            WasmType::I32 => {} // Already correct type
                            WasmType::F32 => {
                                // Convert F32 to I32 (reinterpret bits)
                                instructions.push(Instruction::I32ReinterpretF32);
                            }
                            WasmType::F64 => {
                                // Convert F64 to I32 safely without trapping
                                // Use saturating truncation instead of trapping truncation
                                instructions.push(Instruction::I32TruncSatF64S);
                            }
                            _ => {
                                // For other types, just use as-is (already I32)
                            }
                        }

                        // Store at offset (i+1)*4 (skip the length field)
                        instructions.push(Instruction::I32Store(wasm_encoder::MemArg {
                            offset: ((i + 1) * 4) as u64,
                            align: 2,
                            memory_index: 0,
                        }));
                    }

                    // Return the list pointer
                    instructions.push(Instruction::LocalGet(list_ptr_local));
                    Ok(WasmType::I32)
                } else {
                    // Fallback: create empty list if no allocator available
                    instructions.push(Instruction::I32Const(0));
                    Ok(WasmType::I32)
                }
            }
            Value::Pairs(pairs) => {
                // Pairs literal: allocate a structure storing [count][key0][val0][key1][val1]...
                // Each slot is 4 bytes (I32 pointer for strings/values).
                // Total size: (1 + 2 * pair_count) * 4 bytes.
                let pair_count = pairs.len() as i32;
                let pairs_size = (1 + 2 * pair_count) * 4;

                if let Some(alloc_fn) = self.get_function_index("allocate_memory") {
                    // Allocate memory for the pairs structure
                    instructions.push(Instruction::I32Const(pairs_size));
                    instructions.push(Instruction::Call(alloc_fn));

                    // Store the pairs pointer in a local variable
                    let pairs_ptr_local = self.current_locals.len() as u32;
                    self.current_locals.push(LocalVarInfo {
                        index: pairs_ptr_local,
                        type_: wasm_encoder::ValType::I32,
                    });
                    instructions.push(Instruction::LocalSet(pairs_ptr_local));

                    // Store the pair count at offset 0
                    instructions.push(Instruction::LocalGet(pairs_ptr_local));
                    instructions.push(Instruction::I32Const(pair_count));
                    instructions.push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));

                    // Store each key-value pair starting at offset 4
                    for (i, (key, val)) in pairs.iter().enumerate() {
                        let key_offset = ((1 + 2 * i) * 4) as u64;
                        let val_offset = ((2 + 2 * i) * 4) as u64;

                        // Store key pointer
                        instructions.push(Instruction::LocalGet(pairs_ptr_local));
                        let key_type = self.generate_value(key, instructions)?;
                        // Normalize key to I32 (all string/integer keys are I32-representable)
                        match key_type {
                            WasmType::I32 => {}
                            WasmType::F32 => {
                                instructions.push(Instruction::I32ReinterpretF32);
                            }
                            WasmType::F64 => {
                                instructions.push(Instruction::I32TruncSatF64S);
                            }
                            _ => {}
                        }
                        instructions.push(Instruction::I32Store(wasm_encoder::MemArg {
                            offset: key_offset,
                            align: 2,
                            memory_index: 0,
                        }));

                        // Store value pointer
                        instructions.push(Instruction::LocalGet(pairs_ptr_local));
                        let val_type = self.generate_value(val, instructions)?;
                        // Normalize value to I32
                        match val_type {
                            WasmType::I32 => {}
                            WasmType::F32 => {
                                instructions.push(Instruction::I32ReinterpretF32);
                            }
                            WasmType::F64 => {
                                instructions.push(Instruction::I32TruncSatF64S);
                            }
                            _ => {}
                        }
                        instructions.push(Instruction::I32Store(wasm_encoder::MemArg {
                            offset: val_offset,
                            align: 2,
                            memory_index: 0,
                        }));
                    }

                    // Return the pairs structure pointer
                    instructions.push(Instruction::LocalGet(pairs_ptr_local));
                    Ok(WasmType::I32)
                } else {
                    // No allocator: for an empty pairs literal return a minimal valid pointer (0),
                    // which signals "empty pairs" to the runtime. Non-empty pairs without an
                    // allocator cannot be stored, so we return 0 and let the runtime handle it.
                    instructions.push(Instruction::I32Const(0));
                    Ok(WasmType::I32)
                }
            }
        }
    }

    /// Generate string interpolation instructions
    pub(crate) fn generate_string_interpolation(
        &mut self,
        parts: &[StringPart],
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        if let Some(builder_init) = self.get_function_index("string_builder_init") {
            instructions.push(Instruction::Call(builder_init));

            for part in parts {
                match part {
                    StringPart::Text(_text) => {
                        // This would allocate string and then call append
                        instructions.push(Instruction::I32Const(0)); // placeholder

                        if let Some(append) = self.get_function_index("string_builder_append") {
                            instructions.push(Instruction::Call(append));
                        } else {
                            return Err(CompilerError::codegen_error(
                                "string_builder_append function not found",
                                None,
                                None,
                            ));
                        }
                    }
                    StringPart::Interpolation(expr) => {
                        self.generate_expression(expr, instructions)?;

                        if let Some(append_value) =
                            self.get_function_index("string_builder_append_value")
                        {
                            instructions.push(Instruction::Call(append_value));
                        } else {
                            return Err(CompilerError::codegen_error(
                                "string_builder_append_value function not found",
                                None,
                                None,
                            ));
                        }
                    }
                }
            }

            if let Some(finish) = self.get_function_index("string_builder_finish") {
                instructions.push(Instruction::Call(finish));
                Ok(())
            } else {
                Err(CompilerError::codegen_error(
                    "string_builder_finish function not found",
                    None,
                    None,
                ))
            }
        } else {
            Err(CompilerError::codegen_error(
                "string_builder_init function not found",
                None,
                None,
            ))
        }
    }

    /// Add function type mapping
    pub(crate) fn add_function_type(
        &mut self,
        index: u32,
        params: Vec<ValType>,
        results: Vec<ValType>,
    ) {
        self.function_types
            .insert(index, FuncType::new(params, results));
    }

    /// Get the function type for a given function index
    pub fn get_function_type(&self, index: u32) -> Option<FuncType> {
        self.function_types.get(&index).cloned()
    }

    /// Convert a parser ValType to a WasmType
    fn from_parser_val_type(val_type: ValType) -> WasmType {
        match val_type {
            ValType::I32 => WasmType::I32,
            ValType::I64 => WasmType::I64,
            ValType::F32 => WasmType::F32,
            ValType::F64 => WasmType::F64,
            ValType::V128 => WasmType::V128,
            _ => WasmType::I32, // Default for other types
        }
    }

    /// Register a function with the instruction generator
    /// The function index is provided by the CodeGenerator to ensure consistency
    pub(crate) fn register_function(
        &mut self,
        name: &str,
        params: &[WasmType],
        return_type: Option<WasmType>,
        _instructions: &[Instruction],
        function_index: u32,
    ) -> Result<u32, CompilerError> {
        // Create signature for this specific overload
        let signature = self.create_function_signature(name, params);

        if function_index >= 42 {
            tracing::trace!(
                name = %name,
                function_index = function_index,
                signature = %signature,
                "Registering function"
            );
        }

        // Check if this exact signature already exists
        if let Some(existing_index) = self.function_signatures.get(&signature) {
            // If the signature exists, return the existing index
            // This allows idempotent registration (multiple calls for same function are OK)
            tracing::debug!(
                signature = %signature,
                existing_index = *existing_index,
                new_index = function_index,
                "Function signature already registered, using existing index"
            );
            return Ok(*existing_index);
        }

        // Use the provided function index from CodeGenerator
        let index = function_index;

        // Add to signature map (for exact overload lookup)
        self.function_signatures.insert(signature, index);

        // Add to function map (for simple name lookup - only store the first registration)
        if !self.function_map.contains_key(name) {
            self.function_map.insert(name.to_string(), index);
        }

        // Create function type and store it
        let param_types: Vec<ValType> = params
            .iter()
            .map(|wasm_type| match wasm_type {
                WasmType::I32 => ValType::I32,
                WasmType::I64 => ValType::I64,
                WasmType::F32 => ValType::F32,
                WasmType::F64 => ValType::F64,
                WasmType::V128 => ValType::V128,
                _ => ValType::I32, // Default for other types
            })
            .collect();

        let result_types: Vec<ValType> = if let Some(ret_type) = return_type {
            vec![match ret_type {
                WasmType::I32 => ValType::I32,
                WasmType::I64 => ValType::I64,
                WasmType::F32 => ValType::F32,
                WasmType::F64 => ValType::F64,
                WasmType::V128 => ValType::V128,
                _ => ValType::I32, // Default for other types
            }]
        } else {
            vec![] // Void function
        };

        // Store the function type
        self.add_function_type(index, param_types, result_types);

        // Return the function index
        Ok(index)
    }
    pub(crate) fn generate_list_access(
        &mut self,
        list: &Expression,
        index: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // First, generate the list expression
        let list_type = self.generate_expression(list, instructions)?;
        if list_type != WasmType::I32 {
            return Err(CompilerError::codegen_error(
                "List access requires list pointer (I32)",
                Some("The list must be a valid list pointer".to_string()),
                None,
            ));
        }

        // Then, generate the index expression
        let index_type = self.generate_expression(index, instructions)?;
        if index_type != WasmType::I32 {
            return Err(CompilerError::codegen_error(
                "List index must be I32",
                Some("The list index must be an integer".to_string()),
                None,
            ));
        }

        // Get the list_get function index
        if let Some(list_get_index) = self.get_function_index("list.get") {
            instructions.push(Instruction::Call(list_get_index));
        } else {
            return Err(CompilerError::codegen_error(
                "No list access function found (list.get)",
                Some("Register list operations to enable list access".to_string()),
                None,
            ));
        }

        // The list access function returns a pointer to the element
        // Now we need to load the actual value based on the expected type

        // Determine element type from list expression context
        let element_type = self.infer_list_element_type(list)?;

        match element_type {
            WasmType::I32 => {
                // Load 32-bit integer
                instructions.push(Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                Ok(WasmType::I32)
            }
            WasmType::F64 => {
                // Load 64-bit float
                instructions.push(Instruction::F64Load(MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                Ok(WasmType::F64)
            }
            WasmType::I64 => {
                // Load 64-bit integer
                instructions.push(Instruction::I64Load(MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                Ok(WasmType::I64)
            }
            WasmType::F32 => {
                // Load 32-bit float
                instructions.push(Instruction::F32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                Ok(WasmType::F32)
            }
            WasmType::V128 => {
                // SIMD not supported in list elements for now
                Err(CompilerError::codegen_error(
                    "V128 types not supported in list elements",
                    Some("Use basic types (i32, f64, etc.) for list elements".to_string()),
                    None,
                ))
            }
            WasmType::Unit => {
                // Unit type doesn't make sense for list elements
                Err(CompilerError::codegen_error(
                    "Unit type not supported in list elements",
                    Some("Use concrete types for list elements".to_string()),
                    None,
                ))
            }
        }
    }

    /// Infer the element type of a list based on its declaration or context
    fn infer_list_element_type(&self, list_expr: &Expression) -> Result<WasmType, CompilerError> {
        use crate::ast::Expression;

        match list_expr {
            Expression::Variable(var_name) => {
                // Try to look up the variable type in our type context
                // For now, use heuristics based on variable name
                if var_name.contains("number") || var_name.contains("float") {
                    Ok(WasmType::F64)
                } else if var_name.contains("string") || var_name.contains("name") {
                    Ok(WasmType::I32) // String pointers are i32
                } else {
                    // Default to i32 for integer lists
                    Ok(WasmType::I32)
                }
            }
            // For other expressions, default to i32
            _ => Ok(WasmType::I32),
        }
    }
}

/// Group locals of the same type to reduce WASM size
pub fn group_locals(locals: &[wasm_encoder::ValType]) -> Vec<(u32, wasm_encoder::ValType)> {
    let mut grouped: Vec<(u32, wasm_encoder::ValType)> = Vec::new();
    if locals.is_empty() {
        return grouped;
    }

    let mut current_type = locals[0];
    let mut current_count: u32 = 0;

    for &local_type in locals {
        if local_type == current_type {
            current_count += 1;
        } else {
            if current_count > 0 {
                grouped.push((current_count, current_type));
            }
            current_type = local_type;
            current_count = 1;
        }
    }

    if current_count > 0 {
        grouped.push((current_count, current_type));
    }

    grouped
}
