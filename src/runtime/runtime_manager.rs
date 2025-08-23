use crate::error::CompilerError;
use crate::runtime::runtime_trait::{RuntimeConfig, RuntimeType, WebAssemblyRuntime};
use std::fmt;

#[cfg(feature = "wasmtime-runtime")]
use crate::runtime::wasmtime_runtime::WasmtimeRuntime;

// Wasmer runtime support temporarily disabled
// #[cfg(feature = "wasmer-runtime")]
// use crate::runtime::wasmer_config::WasmerRuntime;

/// Runtime manager for selecting and configuring WebAssembly runtimes
pub struct RuntimeManager;

/// Enumeration of available runtime implementations
pub enum RuntimeInstance {
    #[cfg(feature = "wasmtime-runtime")]
    Wasmtime(WasmtimeRuntime),
    // Note: Wasmer runtime support is temporarily disabled
}

/// Runtime information for display and selection
#[derive(Debug, Clone)]
pub struct RuntimeInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub available: bool,
    pub description: &'static str,
    pub features: Vec<&'static str>,
}

impl RuntimeManager {
    /// Get information about all available runtimes
    pub fn list_available_runtimes() -> Vec<RuntimeInfo> {
        let mut runtimes = Vec::new();

        #[cfg(feature = "wasmtime-runtime")]
        runtimes.push(RuntimeInfo {
            name: WasmtimeRuntime::runtime_name(),
            version: WasmtimeRuntime::runtime_version(),
            available: true,
            description: "Bytecode Alliance WebAssembly runtime with excellent performance and standards compliance",
            features: vec!["Async Support", "WASI", "Component Model", "Threads", "SIMD"],
        });

        #[cfg(feature = "wasmer-runtime")]
        // runtimes.push(RuntimeInfo {
        //     name: WasmerRuntime::runtime_name(),
        //     version: WasmerRuntime::runtime_version(),
        //     available: true,
        //     description: "Universal WebAssembly runtime with multiple compiler backends",
        //     features: vec!["WASI", "Threads", "SIMD", "Multiple Backends"],
        // });  // Wasmer temporarily disabled

        // Add unavailable runtimes for completeness
        #[cfg(not(feature = "wasmtime-runtime"))]
        runtimes.push(RuntimeInfo {
            name: "Wasmtime",
            version: "N/A",
            available: false,
            description: "Bytecode Alliance WebAssembly runtime (not compiled in)",
            features: vec![
                "Async Support",
                "WASI",
                "Component Model",
                "Threads",
                "SIMD",
            ],
        });

        #[cfg(not(feature = "wasmer-runtime"))]
        runtimes.push(RuntimeInfo {
            name: "Wasmer",
            version: "N/A",
            available: false,
            description: "Universal WebAssembly runtime (not compiled in)",
            features: vec!["WASI", "Threads", "SIMD", "Multiple Backends"],
        });

        runtimes
    }

    /// Select the best available runtime based on configuration
    pub fn select_runtime(config: &RuntimeConfig) -> Result<RuntimeType, CompilerError> {
        match config.runtime_type {
            RuntimeType::Wasmtime => {
                #[cfg(feature = "wasmtime-runtime")]
                return Ok(RuntimeType::Wasmtime);

                #[cfg(not(feature = "wasmtime-runtime"))]
                return Err(CompilerError::runtime_error(
                    "Wasmtime runtime requested but not available (enable 'wasmtime-runtime' feature)".to_string(),
                    None,
                    None,
                ));
            }
            RuntimeType::Wasmer => {
                #[cfg(feature = "wasmer-runtime")]
                return Ok(RuntimeType::Wasmer);

                #[cfg(not(feature = "wasmer-runtime"))]
                return Err(CompilerError::runtime_error(
                    "Wasmer runtime requested but not available (enable 'wasmer-runtime' feature)"
                        .to_string(),
                    None,
                    None,
                ));
            }
            RuntimeType::Auto => {
                // Auto-detection logic - prefer based on features needed

                // If async support is needed, prefer Wasmtime
                #[cfg(feature = "wasmtime-runtime")]
                if config.async_support {
                    return Ok(RuntimeType::Wasmtime);
                }

                // For general use, prefer Wasmtime if available
                #[cfg(feature = "wasmtime-runtime")]
                {
                    return Ok(RuntimeType::Wasmtime);
                }

                // Wasmer runtime support is currently disabled
                // #[cfg(feature = "wasmer-runtime")]
                // {
                //     return Ok(RuntimeType::Wasmer);
                // }

                // If no runtime is available
                #[cfg(not(any(feature = "wasmtime-runtime", feature = "wasmer-runtime")))]
                {
                    return Err(CompilerError::runtime_error(
                        "No WebAssembly runtime available (enable 'wasmtime-runtime' or 'wasmer-runtime' feature)".to_string(),
                        None,
                        None,
                    ));
                }
            }
        }
    }

    /// Validate that a runtime can be used with the given configuration
    pub fn validate_runtime_config(
        runtime_type: RuntimeType,
        config: &RuntimeConfig,
    ) -> Result<(), CompilerError> {
        match runtime_type {
            #[cfg(feature = "wasmtime-runtime")]
            RuntimeType::Wasmtime => WasmtimeRuntime::validate_runtime(config),
            RuntimeType::Wasmer => Err(CompilerError::runtime_error(
                "Wasmer runtime is temporarily disabled".to_string(),
                None,
                None,
            )),
            RuntimeType::Auto => {
                // Auto-detection: prefer Wasmtime if available
                #[cfg(feature = "wasmtime-runtime")]
                return WasmtimeRuntime::validate_runtime(config);
                
                #[cfg(not(feature = "wasmtime-runtime"))]
                return Err(CompilerError::runtime_error(
                    "No runtime available for auto-detection".to_string(),
                    None,
                    None,
                ));
            }
        }
    }

    /// Get runtime-specific recommendations for configuration
    pub fn get_runtime_recommendations(runtime_type: RuntimeType) -> Vec<String> {
        match runtime_type {
            RuntimeType::Wasmtime => vec![
                "Use async_support=true for async/await features".to_string(),
                "Enable threads_support for parallel processing".to_string(),
                "Set optimization_level=SpeedAndSize for production".to_string(),
            ],
            RuntimeType::Wasmer => vec![
                "Use optimization_level=Speed for best performance".to_string(),
                "Enable SIMD support for numerical computations".to_string(),
                "Consider multiple compiler backends for different use cases".to_string(),
            ],
            RuntimeType::Auto => vec![
                "Auto-selection will choose the best runtime for your configuration".to_string(),
                "Enable both runtime features for maximum flexibility".to_string(),
            ],
        }
    }

    /// Benchmark available runtimes (placeholder for future implementation)
    pub fn benchmark_runtimes(_wasm_bytes: &[u8]) -> Result<Vec<RuntimeBenchmark>, CompilerError> {
        // TODO: Implement runtime benchmarking
        Ok(vec![])
    }
}

/// Benchmark result for runtime comparison
#[derive(Debug, Clone)]
pub struct RuntimeBenchmark {
    pub runtime_name: String,
    pub compilation_time_ms: f64,
    pub execution_time_ms: f64,
    pub memory_usage_bytes: usize,
    pub success: bool,
    pub error_message: Option<String>,
}

impl fmt::Display for RuntimeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} - {}",
            self.name,
            if self.available {
                self.version
            } else {
                "(unavailable)"
            },
            self.description
        )
    }
}

impl fmt::Display for RuntimeBenchmark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.success {
            write!(
                f,
                "{}: Compilation {:.2}ms, Execution {:.2}ms, Memory {}KB",
                self.runtime_name,
                self.compilation_time_ms,
                self.execution_time_ms,
                self.memory_usage_bytes / 1024
            )
        } else {
            write!(
                f,
                "{}: Failed - {}",
                self.runtime_name,
                self.error_message.as_deref().unwrap_or("Unknown error")
            )
        }
    }
}

/// Helper function to create a runtime-appropriate configuration
pub fn create_runtime_config(runtime_type: RuntimeType) -> RuntimeConfig {
    let mut config = RuntimeConfig::default();
    config.runtime_type = runtime_type;

    match runtime_type {
        RuntimeType::Wasmtime => {
            // Wasmtime-specific defaults
            config.async_support = true;
        }
        RuntimeType::Wasmer => {
            // Wasmer-specific defaults
            config.async_support = false; // Wasmer doesn't have built-in async
        }
        RuntimeType::Auto => {
            // Auto defaults - will be adjusted based on selected runtime
        }
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_available_runtimes() {
        let runtimes = RuntimeManager::list_available_runtimes();
        assert!(!runtimes.is_empty(), "Should list at least one runtime");

        // Check that default runtime is available
        #[cfg(feature = "wasmtime-runtime")]
        {
            let wasmtime = runtimes.iter().find(|r| r.name == "Wasmtime");
            assert!(wasmtime.is_some(), "Wasmtime should be listed");
            assert!(wasmtime.unwrap().available, "Wasmtime should be available");
        }
    }

    #[test]
    fn test_select_runtime_auto() {
        let config = RuntimeConfig::default(); // Uses RuntimeType::Auto
        let selected = RuntimeManager::select_runtime(&config);
        assert!(selected.is_ok(), "Auto-selection should succeed");
    }

    #[test]
    fn test_create_runtime_config() {
        let config = create_runtime_config(RuntimeType::Auto);
        assert_eq!(config.runtime_type, RuntimeType::Auto);

        #[cfg(feature = "wasmtime-runtime")]
        {
            let wasmtime_config = create_runtime_config(RuntimeType::Wasmtime);
            assert_eq!(wasmtime_config.runtime_type, RuntimeType::Wasmtime);
            assert!(wasmtime_config.async_support);
        }
    }
}
