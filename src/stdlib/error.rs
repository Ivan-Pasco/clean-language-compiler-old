use crate::error::CompilerError;
use anyhow;

/// Trait for standard library errors that can be converted to CompilerError
pub trait StdlibError {
    /// Converts the error to a CompilerError
    fn to_compiler_error(&self) -> CompilerError;
}

/// Implementation for converting any StdlibError to CompilerError
impl<T: StdlibError> From<T> for CompilerError {
    fn from(error: T) -> Self {
        error.to_compiler_error()
    }
}

/// Implementation of StdlibError for anyhow::Error which is used by wasmtime
impl StdlibError for anyhow::Error {
    fn to_compiler_error(&self) -> CompilerError {
        CompilerError::runtime_error(format!("Runtime error: {}", self), None, None)
    }
}
