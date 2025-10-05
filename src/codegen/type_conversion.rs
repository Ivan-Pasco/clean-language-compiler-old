//! Type conversion utilities for the Clean Language compiler.
//!
//! This module handles conversion between Clean Language AST types and WebAssembly types,
//! as well as runtime type conversion functions and compatibility checking.
//!
//! Note: Many conversion methods are infrastructure for future type system features.

#![allow(dead_code)]

use crate::ast::Type as AstType;
use crate::error::CompilerError;
use crate::types::WasmType;
use wasm_encoder::ValType;

/// Handles type conversions between AST and WASM types.
pub struct TypeConverter;

impl TypeConverter {
    /// Create a new type converter.
    pub fn new() -> Self {
        Self
    }

    /// Convert Clean Language AST type to WebAssembly type.
    pub fn ast_type_to_wasm_type(&self, ast_type: &AstType) -> Result<WasmType, CompilerError> {
        match ast_type {
            AstType::Boolean => Ok(WasmType::I32),
            AstType::Integer => Ok(WasmType::I32),
            AstType::Number => Ok(WasmType::F64),
            AstType::String => Ok(WasmType::I32), // String pointers
            AstType::Void => Ok(WasmType::I32),   // Void represented as I32
            AstType::List(_) => Ok(WasmType::I32), // List pointers
            AstType::Matrix(_) => Ok(WasmType::I32), // Matrix pointers
            AstType::Pairs(_, _) => Ok(WasmType::I32), // Pairs are represented as pointers
            AstType::Object(_) => Ok(WasmType::I32), // Object pointers
            AstType::Generic(_, _) => Ok(WasmType::I32), // Generic type pointers
            AstType::TypeParameter(_) => Ok(WasmType::I32), // Type parameter pointers
            AstType::Any => Ok(WasmType::I32),    // Any type is represented as a pointer
            // Sized types
            AstType::IntegerSized { bits: 8..=32, .. } => Ok(WasmType::I32),
            AstType::IntegerSized { bits: 64, .. } => Ok(WasmType::I64),
            AstType::NumberSized { bits: 32 } => Ok(WasmType::F32),
            AstType::NumberSized { bits: 64 } => Ok(WasmType::F64),
            AstType::Class { .. } => Ok(WasmType::I32), // Pointer to object
            AstType::Function(_, _) => Ok(WasmType::I32), // Function pointer
            _ => Ok(WasmType::I32),                     // Default fallback for any other types
        }
    }

    /// Convert WebAssembly type to Clean Language AST type (best effort).
    pub fn wasm_type_to_ast_type(&self, wasm_type: &WasmType) -> AstType {
        match wasm_type {
            WasmType::I32 => AstType::Integer,
            WasmType::I64 => AstType::IntegerSized {
                bits: 64,
                unsigned: false,
            },
            WasmType::F32 => AstType::NumberSized { bits: 32 },
            WasmType::F64 => AstType::Number,
            WasmType::V128 => AstType::Any, // V128 doesn't have a direct Clean Language equivalent
            WasmType::Unit => AstType::Void,
        }
    }

    /// Check if two WASM types are compatible for operations.
    pub fn types_compatible(&self, from: &WasmType, to: &WasmType) -> bool {
        // Exact type match
        if from == to {
            return true;
        }

        // Integer type conversions - allow conversions between all integer precisions
        match (from, to) {
            (WasmType::I32, WasmType::I64) => true,
            (WasmType::I64, WasmType::I32) => true,
            // Float conversions
            (WasmType::F32, WasmType::F64) => true,
            (WasmType::F64, WasmType::F32) => true,
            // Integer to float conversions
            (WasmType::I32, WasmType::F32) => true,
            (WasmType::I32, WasmType::F64) => true,
            (WasmType::I64, WasmType::F32) => true,
            (WasmType::I64, WasmType::F64) => true,
            // Float to integer conversions (with potential loss of precision)
            (WasmType::F32, WasmType::I32) => true,
            (WasmType::F64, WasmType::I32) => true,
            (WasmType::F32, WasmType::I64) => true,
            (WasmType::F64, WasmType::I64) => true,
            // Unit type conversions
            (WasmType::Unit, WasmType::I32) => true,
            (WasmType::I32, WasmType::Unit) => true,
            _ => false,
        }
    }

    /// Check if a type conversion requires explicit conversion instructions.
    pub fn requires_conversion(&self, from: &WasmType, to: &WasmType) -> bool {
        if from == to {
            return false;
        }

        match (from, to) {
            // These conversions require explicit instructions
            (WasmType::I32, WasmType::I64) => true,
            (WasmType::I64, WasmType::I32) => true,
            (WasmType::F32, WasmType::F64) => true,
            (WasmType::F64, WasmType::F32) => true,
            (WasmType::I32, WasmType::F32) => true,
            (WasmType::I32, WasmType::F64) => true,
            (WasmType::I64, WasmType::F32) => true,
            (WasmType::I64, WasmType::F64) => true,
            (WasmType::F32, WasmType::I32) => true,
            (WasmType::F64, WasmType::I32) => true,
            (WasmType::F32, WasmType::I64) => true,
            (WasmType::F64, WasmType::I64) => true,
            _ => false,
        }
    }

    /// Get the default WASM type for a given Clean Language type.
    pub fn get_default_wasm_type(&self, ast_type: &AstType) -> Result<WasmType, CompilerError> {
        self.ast_type_to_wasm_type(ast_type)
    }

    /// Check if an AST type represents a numeric type.
    pub fn is_numeric_type(&self, ast_type: &AstType) -> bool {
        matches!(
            ast_type,
            AstType::Integer
                | AstType::Number
                | AstType::IntegerSized { .. }
                | AstType::NumberSized { .. }
        )
    }

    /// Check if an AST type represents a pointer type.
    pub fn is_pointer_type(&self, ast_type: &AstType) -> bool {
        matches!(
            ast_type,
            AstType::String
                | AstType::List(_)
                | AstType::Matrix(_)
                | AstType::Object(_)
                | AstType::Class { .. }
                | AstType::Function(_, _)
        )
    }

    /// Get the memory type ID for runtime type checking.
    pub fn get_memory_type_id(&self, ast_type: &AstType) -> u32 {
        match ast_type {
            AstType::Integer => 1,
            AstType::Number => 2,
            AstType::String => 3,
            AstType::List(_) => 4,
            AstType::Matrix(_) => 5,
            AstType::Boolean => 6,
            AstType::Object(_) => 7,
            AstType::Class { .. } => 8,
            _ => 0, // Unknown/default type
        }
    }

    /// Convert WasmType to wasm_encoder::ValType for compatibility.
    pub fn wasm_type_to_val_type(&self, wasm_type: &WasmType) -> ValType {
        match wasm_type {
            WasmType::I32 => ValType::I32,
            WasmType::I64 => ValType::I64,
            WasmType::F32 => ValType::F32,
            WasmType::F64 => ValType::F64,
            WasmType::V128 => ValType::V128,
            WasmType::Unit => ValType::I32,
        }
    }

    /// Convert wasm_encoder::ValType to WasmType.
    pub fn val_type_to_wasm_type(&self, val_type: &ValType) -> Result<WasmType, CompilerError> {
        match val_type {
            ValType::I32 => Ok(WasmType::I32),
            ValType::I64 => Ok(WasmType::I64),
            ValType::F32 => Ok(WasmType::F32),
            ValType::F64 => Ok(WasmType::F64),
            ValType::V128 => Ok(WasmType::V128),
            other => Err(CompilerError::type_error(
                format!("Unsupported ValType: {other:?}"),
                None,
                None,
            )),
        }
    }
}

impl Default for TypeConverter {
    fn default() -> Self {
        Self::new()
    }
}

/// Type conversion operations for runtime.
pub struct TypeConversionOperations {
    converter: TypeConverter,
}

impl TypeConversionOperations {
    /// Create new type conversion operations.
    pub fn new() -> Self {
        Self {
            converter: TypeConverter::new(),
        }
    }

    /// Get common type conversion strings for pre-allocation.
    pub fn get_conversion_strings(&self) -> Vec<(&'static str, u32)> {
        vec![
            ("true", 300),
            ("false", 310),
            ("42", 320),
            ("[int]", 330),
            ("3.14", 340),
            ("[float]", 350),
            ("0", 400),
            ("1", 410),
            ("null", 420),
            ("[object]", 430),
        ]
    }

    /// Get the type converter.
    pub fn get_converter(&self) -> &TypeConverter {
        &self.converter
    }
}

impl Default for TypeConversionOperations {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ast_to_wasm_type_conversion() {
        let converter = TypeConverter::new();

        assert_eq!(
            converter.ast_type_to_wasm_type(&AstType::Integer).unwrap(),
            WasmType::I32
        );
        assert_eq!(
            converter.ast_type_to_wasm_type(&AstType::Number).unwrap(),
            WasmType::F64
        );
        assert_eq!(
            converter.ast_type_to_wasm_type(&AstType::String).unwrap(),
            WasmType::I32
        );
        assert_eq!(
            converter.ast_type_to_wasm_type(&AstType::Boolean).unwrap(),
            WasmType::I32
        );
    }

    #[test]
    fn test_wasm_to_ast_type_conversion() {
        let converter = TypeConverter::new();

        assert_eq!(
            converter.wasm_type_to_ast_type(&WasmType::I32),
            AstType::Integer
        );
        assert_eq!(
            converter.wasm_type_to_ast_type(&WasmType::F64),
            AstType::Number
        );
        assert_eq!(
            converter.wasm_type_to_ast_type(&WasmType::Unit),
            AstType::Void
        );
    }

    #[test]
    fn test_type_compatibility() {
        let converter = TypeConverter::new();

        assert!(converter.types_compatible(&WasmType::I32, &WasmType::I32));
        assert!(converter.types_compatible(&WasmType::I32, &WasmType::I64));
        assert!(converter.types_compatible(&WasmType::F32, &WasmType::F64));
        assert!(!converter.types_compatible(&WasmType::I32, &WasmType::V128));
    }

    #[test]
    fn test_numeric_type_detection() {
        let converter = TypeConverter::new();

        assert!(converter.is_numeric_type(&AstType::Integer));
        assert!(converter.is_numeric_type(&AstType::Number));
        assert!(!converter.is_numeric_type(&AstType::String));
        assert!(!converter.is_numeric_type(&AstType::Boolean));
    }

    #[test]
    fn test_pointer_type_detection() {
        let converter = TypeConverter::new();

        assert!(converter.is_pointer_type(&AstType::String));
        assert!(converter.is_pointer_type(&AstType::List(Box::new(AstType::Integer))));
        assert!(!converter.is_pointer_type(&AstType::Integer));
        assert!(!converter.is_pointer_type(&AstType::Boolean));
    }

    #[test]
    fn test_conversion_strings() {
        let ops = TypeConversionOperations::new();
        let strings = ops.get_conversion_strings();

        assert!(!strings.is_empty());
        assert!(strings.iter().any(|(s, _)| *s == "true"));
        assert!(strings.iter().any(|(s, _)| *s == "false"));
    }
}
