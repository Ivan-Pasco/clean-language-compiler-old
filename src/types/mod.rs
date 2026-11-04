use wasm_encoder::ValType;

/// WebAssembly value types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
    V128,
    Unit,
}

impl WasmType {
    /// Convert to wasm_encoder::ValType
    pub fn to_val_type(self) -> ValType {
        match self {
            WasmType::I32 => ValType::I32,
            WasmType::I64 => ValType::I64,
            WasmType::F32 => ValType::F32,
            WasmType::F64 => ValType::F64,
            WasmType::V128 => ValType::V128,
            WasmType::Unit => ValType::I32, // Map Unit to I32 for WebAssembly compatibility
        }
    }

    /// Convert to wasmparser ValType
    pub fn to_parser_val_type(&self) -> wasmparser::ValType {
        match self {
            WasmType::I32 => wasmparser::ValType::I32,
            WasmType::I64 => wasmparser::ValType::I64,
            WasmType::F32 => wasmparser::ValType::F32,
            WasmType::F64 => wasmparser::ValType::F64,
            WasmType::V128 => wasmparser::ValType::V128,
            WasmType::Unit => wasmparser::ValType::I32, // Map Unit to I32 for WebAssembly compatibility
        }
    }
}

impl From<WasmType> for ValType {
    fn from(wasm_type: WasmType) -> Self {
        wasm_type.to_val_type()
    }
}

impl From<ValType> for WasmType {
    fn from(val_type: ValType) -> Self {
        match val_type {
            ValType::I32 => WasmType::I32,
            ValType::I64 => WasmType::I64,
            ValType::F32 => WasmType::F32,
            ValType::F64 => WasmType::F64,
            ValType::V128 => WasmType::V128,
            _ => WasmType::I32, // Default fallback for unsupported types
        }
    }
}
