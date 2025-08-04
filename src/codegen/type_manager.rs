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

    /// Get a reference to the type section
    #[allow(dead_code)]
    pub(crate) fn get_type_section(&self) -> &TypeSection {
        &self.type_section
    }

    /// Get a cloned type section for module assembly
    pub(crate) fn clone_type_section(&self) -> TypeSection {
        self.type_section.clone()
    }

    /// Add a function type to the type section
    pub(crate) fn add_function_type(
        &mut self,
        params: &[WasmType],
        return_type: Option<WasmType>,
    ) -> Result<u32, CompilerError> {
        let param_val_types: Vec<ValType> = params.iter().map(|t| (*t).into()).collect();
        let return_val_type: Vec<ValType> = return_type.map(|t| vec![t.into()]).unwrap_or_default();

        self.type_section
            .function(param_val_types.clone(), return_val_type.clone());
        let type_index = self.function_types.len() as u32;

        let parser_param_types: Vec<wasmparser::ValType> = param_val_types
            .iter()
            .map(|vt| WasmType::from(*vt).to_parser_val_type())
            .collect();
        let parser_result_types: Vec<wasmparser::ValType> = return_val_type
            .iter()
            .map(|vt| WasmType::from(*vt).to_parser_val_type())
            .collect();
        self.function_types
            .push(FuncType::new(parser_param_types, parser_result_types));

        Ok(type_index)
    }

    /// Get the function types stored in this manager
    #[allow(dead_code)]
    pub(crate) fn get_function_types(&self) -> &Vec<FuncType> {
        &self.function_types
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
    #[allow(dead_code)]
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

    /// Infer the WasmType from a Value
    #[allow(dead_code)]
    pub(crate) fn infer_type(&self, value: &Value) -> Result<WasmType, CompilerError> {
        Ok(match value {
            Value::Integer(_) => WasmType::I32,
            Value::Boolean(_) => WasmType::I32, // Booleans are represented as I32 in WASM
            Value::String(_) => WasmType::I32,  // Strings are pointers in WASM
            Value::Number(_) => WasmType::F64,
            Value::List(_) => WasmType::I32, // Lists are pointers in WASM
            Value::Matrix(_) => WasmType::I32, // Matrices are pointers in WASM
            Value::Void => WasmType::I32,    // Void represented as I32
            // Sized types
            Value::Integer8(_) => WasmType::I32,
            Value::Integer8u(_) => WasmType::I32,
            Value::Integer16(_) => WasmType::I32,
            Value::Integer16u(_) => WasmType::I32,
            Value::Integer32(_) => WasmType::I32,
            Value::Integer64(_) => WasmType::I64,
            Value::Number32(_) => WasmType::F32,
            Value::Number64(_) => WasmType::F64,
        })
    }

    #[allow(dead_code)]
    pub fn convert_value_to_wasm_type(&self, value: &Value) -> Result<WasmType, CompilerError> {
        Ok(match value {
            Value::Integer(_) => WasmType::I32,
            Value::Boolean(_) => WasmType::I32, // Booleans are represented as I32 in WASM
            Value::String(_) => WasmType::I32,  // Strings are pointers in WASM
            Value::Number(_) => WasmType::F64,
            Value::List(_) => WasmType::I32, // Lists are pointers in WASM
            Value::Matrix(_) => WasmType::I32, // Matrices are pointers in WASM
            Value::Void => WasmType::I32,    // Void represented as I32
            // Sized types
            Value::Integer8(_) => WasmType::I32,
            Value::Integer8u(_) => WasmType::I32,
            Value::Integer16(_) => WasmType::I32,
            Value::Integer16u(_) => WasmType::I32,
            Value::Integer32(_) => WasmType::I32,
            Value::Integer64(_) => WasmType::I64,
            Value::Number32(_) => WasmType::F32,
            Value::Number64(_) => WasmType::F64,
        })
    }

    /// Check if a type is Any
    #[allow(dead_code)]
    pub(crate) fn is_any_type(&self, ast_type: &Type) -> bool {
        matches!(ast_type, Type::Any)
    }
}
