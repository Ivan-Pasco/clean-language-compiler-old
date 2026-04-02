//! Module for type management during code generation.

use crate::ast::{Expression, Type, Value};
use crate::error::CompilerError;
use crate::types::WasmType;
use wasm_encoder::{TypeSection, ValType};
use wasmparser::FuncType;

/// Manages type information and conversions during code generation
#[derive(Clone)]
pub(crate) struct TypeManager {
    type_section: TypeSection,
    function_types: Vec<FuncType>,
}

impl TypeManager {
    /// Create a new type manager
    pub(crate) fn new() -> Self {
        Self {
            type_section: TypeSection::new(),
            function_types: Vec::new(),
        }
    }

    /// Get a cloned type section for module assembly
    pub(crate) fn clone_type_section(&self) -> TypeSection {
        self.type_section.clone()
    }

    /// Add a function type to the type section (supports multi-value returns)
    /// Returns the type index, reusing existing types if they match
    pub(crate) fn add_function_type(
        &mut self,
        params: &[WasmType],
        return_types: &[WasmType],
    ) -> Result<u32, CompilerError> {
        let param_val_types: Vec<ValType> = params.iter().map(|t| (*t).into()).collect();
        let return_val_types: Vec<ValType> = return_types.iter().map(|t| (*t).into()).collect();

        // Convert to parser types for comparison
        let parser_param_types: Vec<wasmparser::ValType> = param_val_types
            .iter()
            .map(|vt| WasmType::from(*vt).to_parser_val_type())
            .collect();
        let parser_result_types: Vec<wasmparser::ValType> = return_val_types
            .iter()
            .map(|vt| WasmType::from(*vt).to_parser_val_type())
            .collect();

        // Check if this function type already exists
        for (index, existing_type) in self.function_types.iter().enumerate() {
            if existing_type.params() == parser_param_types.as_slice()
                && existing_type.results() == parser_result_types.as_slice()
            {
                // Type already exists, return its index
                return Ok(index as u32);
            }
        }

        // Type doesn't exist, add it
        self.type_section
            .function(param_val_types.clone(), return_val_types.clone());
        let type_index = self.function_types.len() as u32;

        self.function_types
            .push(FuncType::new(parser_param_types, parser_result_types));

        Ok(type_index)
    }

    /// Add a function type with single or no return value (convenience method)
    pub(crate) fn add_function_type_single(
        &mut self,
        params: &[WasmType],
        return_type: Option<WasmType>,
    ) -> Result<u32, CompilerError> {
        let return_types: Vec<WasmType> = return_type.map(|t| vec![t]).unwrap_or_default();
        self.add_function_type(params, &return_types)
    }

    /// Check if conversion is possible between two types
    #[allow(dead_code)]
    pub(crate) fn can_convert(&self, from: WasmType, to: WasmType) -> bool {
        // Any type is compatible with any other type
        if from == WasmType::I32 && to == WasmType::I32 {
            return true;
        }

        match (from, to) {
            (WasmType::I32, WasmType::F64) => true,
            (WasmType::F64, WasmType::I32) => true,
            (WasmType::I64, WasmType::F64) => true,
            (WasmType::F64, WasmType::I64) => true,
            (WasmType::F32, WasmType::F64) => true,
            (WasmType::F64, WasmType::F32) => true,
            _ => from == to,
        }
    }

    /// Check if the given expression is a string type
    #[allow(dead_code)]
    pub(crate) fn is_string_type(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Literal(Value::String(_)) => true,
            Expression::StringInterpolation(_) => true,
            // For variables, ideally this would check the variable's type
            _ => false,
        }
    }

    /// Convert AST type to WasmType
    pub(crate) fn ast_type_to_wasm_type(&self, ast_type: &Type) -> Result<WasmType, CompilerError> {
        match ast_type {
            Type::Boolean => Ok(WasmType::I32),
            Type::Integer => Ok(WasmType::I64),
            Type::Number => Ok(WasmType::F64),
            Type::String => Ok(WasmType::I32),  // String pointers
            Type::Void => Ok(WasmType::I32),    // Void represented as I32
            Type::List(_) => Ok(WasmType::I32), // List pointers
            Type::Matrix(_) => Ok(WasmType::I32), // Matrix pointers
            Type::Pairs(_, _) => Ok(WasmType::I32), // Pairs are represented as pointers
            Type::Object(_) => Ok(WasmType::I32), // Object pointers
            Type::Generic(_, _) => Ok(WasmType::I32), // Generic type pointers
            Type::TypeParameter(_) => Ok(WasmType::I32), // Type parameter pointers
            Type::Any => Ok(WasmType::I32),     // Any type is represented as a pointer
            // Sized types
            Type::IntegerSized { bits: 8..=32, .. } => Ok(WasmType::I32),
            Type::IntegerSized { bits: 64, .. } => Ok(WasmType::I64),
            Type::NumberSized { bits: 32 } => Ok(WasmType::F32),
            Type::NumberSized { bits: 64 } => Ok(WasmType::F64),
            Type::Class { .. } => Ok(WasmType::I32), // Pointer to object
            Type::Function(_, _) => Ok(WasmType::I32), // Function pointer
            _ => Ok(WasmType::I32),                  // Default fallback for any other types
        }
    }
}
