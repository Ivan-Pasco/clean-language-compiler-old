//! Builtin type descriptors used by the bridge function ABI.
//!
//! Historically this module also hosted a `BuiltinRegistry` that duplicated
//! the language built-in signatures already in `src/resolver/symbol_table.rs`.
//! That duplication was the largest sub-finding of BUILTIN-NAMESPACE-OVERREACH
//! and was removed in 0.30.289 — MCP `tool_list_builtins` now enumerates
//! from a freshly-built `SymbolTable` (no category filter, no static/instance
//! method distinction; both were accepted regressions).
//!
//! What remains here is the `BuiltinType` enum: a small, conversion-friendly
//! type representation used by `BridgeFunction::get_param_types` /
//! `get_return_type` to bridge between plugin manifest type strings and the
//! compiler's own `HirType` / `ConcreteType` / `MirType` enums.

/// Simplified type representation for builtins
/// Can be converted to HirType, ConcreteType, or WasmType as needed
#[derive(Debug, Clone, PartialEq)]
pub enum BuiltinType {
    Integer,
    Number,
    String,
    Boolean,
    Void,
    List(Box<BuiltinType>),
    Matrix(Box<BuiltinType>),
    Pairs(Box<BuiltinType>, Box<BuiltinType>),
    Namespace,
    Any,     // For generic functions
    Handler, // Function reference passed as callback index to bridge functions
}

impl BuiltinType {
    /// Convert to HirType for resolver stage
    pub fn to_hir_type(&self) -> crate::hir::HirType {
        match self {
            BuiltinType::Integer => crate::hir::HirType::Integer,
            BuiltinType::Number => crate::hir::HirType::Number,
            BuiltinType::String => crate::hir::HirType::String,
            BuiltinType::Boolean => crate::hir::HirType::Boolean,
            BuiltinType::Void => crate::hir::HirType::Void,
            BuiltinType::List(inner) => crate::hir::HirType::List(Box::new(inner.to_hir_type())),
            BuiltinType::Matrix(inner) => {
                crate::hir::HirType::Matrix(Box::new(inner.to_hir_type()))
            }
            BuiltinType::Pairs(k, v) => {
                crate::hir::HirType::Pairs(Box::new(k.to_hir_type()), Box::new(v.to_hir_type()))
            }
            BuiltinType::Namespace => crate::hir::HirType::Void, // Namespace is a special case
            BuiltinType::Any => crate::hir::HirType::Any,        // Dynamic type for JSON values
            BuiltinType::Handler => crate::hir::HirType::Integer, // Handler is an i32 index at WASM level
        }
    }

    /// Convert to ConcreteType for type checker stage
    pub fn to_concrete_type(&self) -> crate::typechecker::ConcreteType {
        use crate::typechecker::ConcreteType;
        match self {
            BuiltinType::Integer => ConcreteType::Integer,
            BuiltinType::Number => ConcreteType::Number,
            BuiltinType::String => ConcreteType::String,
            BuiltinType::Boolean => ConcreteType::Boolean,
            BuiltinType::Void => ConcreteType::Null,
            BuiltinType::List(inner) => ConcreteType::Array(Box::new(inner.to_concrete_type())),
            BuiltinType::Matrix(inner) => ConcreteType::Matrix(Box::new(inner.to_concrete_type())),
            BuiltinType::Pairs(k, v) => ConcreteType::Pairs(
                Box::new(k.to_concrete_type()),
                Box::new(v.to_concrete_type()),
            ),
            BuiltinType::Namespace => ConcreteType::Namespace,
            BuiltinType::Any => ConcreteType::Any, // Dynamic type with runtime type tag
            BuiltinType::Handler => ConcreteType::Integer, // Handler is an i32 index at WASM level
        }
    }
}
