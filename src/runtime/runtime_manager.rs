// Compile-time runtime metadata listing.
//
// This module only describes WebAssembly runtimes the user can target —
// it does not execute WASM itself. Real execution lives in clean-server
// (Layer 2) or clean-runner. See foundation/management/
// ARCHITECTURE_BOUNDARIES.md.

use crate::error::CompilerError;
use crate::runtime::runtime_trait::{RuntimeConfig, RuntimeType};
use std::fmt;

/// Runtime manager for listing and recommending WebAssembly runtimes
pub struct RuntimeManager;

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
    /// Get information about all known runtimes.
    /// The "available" flag reflects what the host clean-server / clean-runner
    /// is expected to support, not whether the compiler can run them
    /// (it can't — execution is delegated).
    pub fn list_available_runtimes() -> Vec<RuntimeInfo> {
        vec![
            RuntimeInfo {
                name: "Wasmtime",
                version: "host-provided",
                available: true,
                description: "Bytecode Alliance WebAssembly runtime — used by clean-server and clean-runner",
                features: vec!["Async Support", "WASI", "Component Model", "Threads", "SIMD"],
            },
            RuntimeInfo {
                name: "Wasmer",
                version: "host-provided",
                available: false,
                description: "Universal WebAssembly runtime — not currently used by the Clean Language platform",
                features: vec!["WASI", "Threads", "SIMD", "Multiple Backends"],
            },
        ]
    }

    /// Select a runtime type for a given configuration.
    /// Returns the requested type unchanged, or maps Auto to the platform default.
    pub fn select_runtime(config: &RuntimeConfig) -> Result<RuntimeType, CompilerError> {
        match config.runtime_type {
            RuntimeType::Wasmtime => Ok(RuntimeType::Wasmtime),
            RuntimeType::Wasmer => Err(CompilerError::runtime_error(
                "Wasmer runtime is not currently supported by the Clean Language platform"
                    .to_string(),
                None,
                None,
            )),
            RuntimeType::Auto => Ok(RuntimeType::Wasmtime),
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
            ],
        }
    }
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

/// Helper function to create a runtime-appropriate configuration
pub fn create_runtime_config(runtime_type: RuntimeType) -> RuntimeConfig {
    let async_support = matches!(runtime_type, RuntimeType::Wasmtime);

    RuntimeConfig {
        runtime_type,
        async_support,
        ..RuntimeConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_available_runtimes() {
        let runtimes = RuntimeManager::list_available_runtimes();
        assert!(!runtimes.is_empty(), "Should list at least one runtime");

        let wasmtime = runtimes.iter().find(|r| r.name == "Wasmtime");
        assert!(wasmtime.is_some(), "Wasmtime should be listed");
        assert!(wasmtime.unwrap().available, "Wasmtime should be available");
    }

    #[test]
    fn test_select_runtime_auto() {
        let config = RuntimeConfig::default();
        let selected = RuntimeManager::select_runtime(&config);
        assert!(selected.is_ok(), "Auto-selection should succeed");
    }

    #[test]
    fn test_create_runtime_config() {
        let config = create_runtime_config(RuntimeType::Auto);
        assert_eq!(config.runtime_type, RuntimeType::Auto);

        let wasmtime_config = create_runtime_config(RuntimeType::Wasmtime);
        assert_eq!(wasmtime_config.runtime_type, RuntimeType::Wasmtime);
        assert!(wasmtime_config.async_support);
    }
}
