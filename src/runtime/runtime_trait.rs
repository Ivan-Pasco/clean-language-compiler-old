// Compile-time runtime metadata: configuration types describing which
// WebAssembly runtime a target prefers, what optimization level to apply,
// and basic memory parameters. The trait/host-bridge machinery that used
// to live here (WebAssemblyRuntime, HostFunctionRegistry, RuntimeFeature,
// ValueType) belongs to the host runtime, not the compiler — see
// foundation/management/ARCHITECTURE_BOUNDARIES.md.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for WebAssembly runtime initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Runtime type preference
    pub runtime_type: RuntimeType,
    /// Enable async support
    pub async_support: bool,
    /// Enable threading support
    pub threads_support: bool,
    /// Enable SIMD operations
    pub simd_support: bool,
    /// Enable bulk memory operations
    pub bulk_memory: bool,
    /// Enable reference types
    pub reference_types: bool,
    /// Optimization level
    pub optimization_level: OptimizationLevel,
    /// Memory configuration
    pub memory_config: MemoryConfig,
    /// Debug information
    pub debug_info: bool,
    /// Target-specific settings
    pub target_settings: HashMap<String, String>,
}

/// Memory configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum static memory size in bytes
    pub static_memory_maximum: usize,
    /// Dynamic memory guard size in bytes
    pub dynamic_memory_guard: usize,
    /// Enable memory64 (64-bit addressing)
    pub memory64: bool,
}

/// Available WebAssembly runtime types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeType {
    /// Wasmtime runtime (Bytecode Alliance)
    Wasmtime,
    /// Wasmer runtime
    Wasmer,
    /// Auto-detect best runtime for current platform
    Auto,
}

/// Optimization levels for WebAssembly compilation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptimizationLevel {
    /// No optimization - fastest compilation
    None,
    /// Light optimization - balanced
    Speed,
    /// Heavy optimization - best performance
    SpeedAndSize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            runtime_type: RuntimeType::Auto,
            async_support: true,
            threads_support: true,
            simd_support: false,
            bulk_memory: true,
            reference_types: true,
            optimization_level: OptimizationLevel::Speed,
            memory_config: MemoryConfig::default(),
            debug_info: cfg!(debug_assertions),
            target_settings: HashMap::new(),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            static_memory_maximum: 64 * 1024 * 1024, // 64MB
            dynamic_memory_guard: 1024 * 1024,       // 1MB
            memory64: false,
        }
    }
}

impl RuntimeType {
    /// Get the best runtime for the current platform.
    /// This is a metadata-only preference — the compiler does not execute
    /// WASM in any case, so the choice only influences what gets recorded
    /// in target configuration.
    pub fn auto_detect() -> Self {
        RuntimeType::Wasmtime
    }

    /// Get human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            RuntimeType::Wasmtime => "Wasmtime",
            RuntimeType::Wasmer => "Wasmer",
            RuntimeType::Auto => "Auto",
        }
    }
}

impl std::fmt::Display for RuntimeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl std::fmt::Display for OptimizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OptimizationLevel::None => write!(f, "none"),
            OptimizationLevel::Speed => write!(f, "speed"),
            OptimizationLevel::SpeedAndSize => write!(f, "speed-and-size"),
        }
    }
}
