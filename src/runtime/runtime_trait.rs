use crate::error::CompilerError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unified WebAssembly runtime abstraction for Clean Language
/// This trait allows Clean Language to work with different WebAssembly runtimes
/// including Wasmtime, Wasmer, and potentially others in the future.
pub trait WebAssemblyRuntime: Send + Sync {
    /// Runtime-specific engine type
    type Engine;
    /// Runtime-specific store type  
    type Store;
    /// Runtime-specific module type
    type Module;
    /// Runtime-specific instance type
    type Instance;
    /// Runtime-specific function type
    type Function;
    /// Runtime-specific value type
    type Value;

    /// Create a new engine with the specified configuration
    fn create_engine(config: &RuntimeConfig) -> Result<Self::Engine, CompilerError>;

    /// Create a new store from an engine
    fn create_store(engine: &Self::Engine) -> Result<Self::Store, CompilerError>;

    /// Create a module from WebAssembly bytecode
    fn create_module(
        engine: &Self::Engine,
        wasm_bytes: &[u8],
    ) -> Result<Self::Module, CompilerError>;

    /// Instantiate a module with host functions
    fn instantiate_module(
        store: &mut Self::Store,
        module: &Self::Module,
        host_functions: &HostFunctionRegistry,
    ) -> Result<Self::Instance, CompilerError>;

    /// Get a function from an instance by name
    fn get_function(
        store: &mut Self::Store,
        instance: &Self::Instance,
        name: &str,
    ) -> Result<Option<Self::Function>, CompilerError>;

    /// Call a function with arguments
    fn call_function(
        store: &mut Self::Store,
        function: &Self::Function,
        args: &[Self::Value],
    ) -> Result<Vec<Self::Value>, CompilerError>;

    /// Get runtime name for identification
    fn runtime_name() -> &'static str;

    /// Get runtime version
    fn runtime_version() -> &'static str;

    /// Check if runtime supports specific features
    fn supports_feature(feature: RuntimeFeature) -> bool;

    /// Validate that the runtime is working correctly
    fn validate_runtime(config: &RuntimeConfig) -> Result<(), CompilerError>;
}

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

/// WebAssembly runtime features
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFeature {
    /// Async/await support
    AsyncSupport,
    /// Multi-threading
    Threads,
    /// SIMD operations
    Simd,
    /// Bulk memory operations  
    BulkMemory,
    /// Reference types
    ReferenceTypes,
    /// Memory64 (64-bit addressing)
    Memory64,
    /// Component model
    ComponentModel,
    /// WASI support
    Wasi,
}

/// Registry for host functions that can be called from WebAssembly
#[derive(Debug, Default)]
pub struct HostFunctionRegistry {
    pub functions: HashMap<String, HostFunction>,
}

/// Represents a host function that can be called from WebAssembly
pub struct HostFunction {
    pub name: String,
    pub module: String,
    pub signature: FunctionSignature,
    pub callback:
        Box<dyn Fn(&[RuntimeValue]) -> Result<Vec<RuntimeValue>, CompilerError> + Send + Sync>,
}

impl std::fmt::Debug for HostFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostFunction")
            .field("name", &self.name)
            .field("module", &self.module)
            .field("signature", &self.signature)
            .field("callback", &"<function>")
            .finish()
    }
}

/// Function signature for host functions
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub params: Vec<ValueType>,
    pub results: Vec<ValueType>,
}

/// WebAssembly value types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    I32,
    I64,
    F32,
    F64,
}

/// Runtime values that can be passed between WebAssembly and host
#[derive(Debug, Clone)]
pub enum RuntimeValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
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
    /// Get the best runtime for the current platform
    pub fn auto_detect() -> Self {
        // For now, prefer Wasmtime as it's more mature
        // In the future, we could add platform-specific logic
        #[cfg(feature = "wasmtime-runtime")]
        {
            RuntimeType::Wasmtime
        }

        // Wasmer runtime support is currently disabled
        // #[cfg(feature = "wasmer-runtime")]
        // {
        //     return RuntimeType::Wasmer;
        // }

        // This should never happen due to default feature
        #[cfg(not(any(feature = "wasmtime-runtime", feature = "wasmer-runtime")))]
        {
            RuntimeType::Wasmtime
        }
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

impl HostFunctionRegistry {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    pub fn register<F>(
        &mut self,
        name: &str,
        module: &str,
        signature: FunctionSignature,
        callback: F,
    ) where
        F: Fn(&[RuntimeValue]) -> Result<Vec<RuntimeValue>, CompilerError> + Send + Sync + 'static,
    {
        let host_function = HostFunction {
            name: name.to_string(),
            module: module.to_string(),
            signature,
            callback: Box::new(callback),
        };

        self.functions
            .insert(format!("{}::{}", module, name), host_function);
    }
}
