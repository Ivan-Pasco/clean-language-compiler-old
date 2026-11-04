use crate::error::CompilerError;
use wasmtime::{Config, Engine};

/// Wasmtime engine construction for the `wasmtime_runner` dev binary.
pub struct CleanWasmtimeConfig;

impl CleanWasmtimeConfig {
    /// Create an engine with minimal configuration for testing/validation
    pub fn create_minimal_engine() -> Result<Engine, CompilerError> {
        let mut config = Config::new();
        config.wasm_bulk_memory(true);
        config.wasm_reference_types(true);

        Engine::new(&config).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create minimal WebAssembly engine: {e}"),
                None,
                None,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_engine_creation() {
        assert!(CleanWasmtimeConfig::create_minimal_engine().is_ok());
    }
}
