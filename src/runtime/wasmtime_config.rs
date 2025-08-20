use crate::error::CompilerError;
use wasmtime::{Config, Engine};

/// Centralized wasmtime configuration for consistent runtime behavior
/// across all Clean Language components (development, release, cleanc, wasmtime_runner)
pub struct CleanWasmtimeConfig;

impl CleanWasmtimeConfig {
    /// Create a standardized wasmtime configuration for Clean Language
    /// This ensures consistent behavior between development and release builds
    pub fn create_config() -> Config {
        let mut config = Config::new();

        // Enable features required for Clean Language runtime
        config.async_support(true); // Required for async operations
        config.wasm_threads(true); // Required for threading support
        config.wasm_bulk_memory(true); // Required for memory operations
        config.wasm_reference_types(true); // Required for reference handling

        // Optimization settings
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);

        // Memory settings for better performance
        config.static_memory_maximum_size(1024 * 1024 * 64); // 64MB max
        config.dynamic_memory_guard_size(1024 * 1024); // 1MB guard

        // Enable debugging info in development builds
        #[cfg(debug_assertions)]
        {
            config.debug_info(true);
            config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
        }

        // Disable debugging info in release builds for performance
        #[cfg(not(debug_assertions))]
        {
            config.debug_info(false);
            config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Disable);
        }

        config
    }

    /// Create a standardized wasmtime engine for Clean Language
    pub fn create_engine() -> Result<Engine, CompilerError> {
        let config = Self::create_config();
        Engine::new(&config).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create Clean Language WebAssembly engine: {e}"),
                None,
                None,
            )
        })
    }

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

    /// Validate that wasmtime is working correctly with a simple test
    pub fn validate_wasmtime() -> Result<(), CompilerError> {
        use wasmtime::{Module, Store};

        let engine = Self::create_minimal_engine()?;
        let _store = Store::new(&engine, ());

        // Simple valid WASM module that just exports a function
        let test_wasm = &[
            0x00, 0x61, 0x73, 0x6d, // WASM magic number
            0x01, 0x00, 0x00, 0x00, // WASM version
            0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: [] -> []
            0x03, 0x02, 0x01, 0x00, // function section: function 0 has type 0
            0x07, 0x08, 0x01, 0x04, 0x74, 0x65, 0x73, 0x74, 0x00,
            0x00, // export section: export "test" function 0
            0x0a, 0x04, 0x01, 0x02, 0x00,
            0x0b, // code section: function 0 body is empty (just end)
        ];

        Module::new(&engine, test_wasm).map_err(|e| {
            CompilerError::runtime_error(format!("Wasmtime validation failed: {e}"), None, None)
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasmtime_config_creation() {
        let config = CleanWasmtimeConfig::create_config();
        // Test that config was created successfully
        assert!(Engine::new(&config).is_ok());
    }

    #[test]
    fn test_wasmtime_engine_creation() {
        let engine = CleanWasmtimeConfig::create_engine();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_wasmtime_validation() {
        let result = CleanWasmtimeConfig::validate_wasmtime();
        assert!(result.is_ok());
    }
}
