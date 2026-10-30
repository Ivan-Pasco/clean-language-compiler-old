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

/// Simplified type representation for builtins, mapped to `HirType` by the
/// resolver and to `WasmType` by the code generator
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
