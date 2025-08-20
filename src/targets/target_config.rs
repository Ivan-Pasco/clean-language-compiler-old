use crate::runtime::runtime_trait::RuntimeType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Compilation target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    /// Target name (e.g., "web", "nodejs", "native")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Target type classification
    pub target_type: TargetType,
    /// Preferred WebAssembly runtime
    pub runtime_preference: RuntimeType,
    /// Target capabilities and limitations
    pub capabilities: TargetCapabilities,
    /// Host function sets available for this target
    pub host_functions: HostFunctionSet,
    /// Target-specific optimization settings
    pub optimizations: TargetOptimizations,
    /// Environment variables and settings
    pub environment: HashMap<String, String>,
}

/// Classification of compilation targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetType {
    /// Web browsers (WebAssembly in browser)
    Web,
    /// Node.js runtime environment
    NodeJS,
    /// Native desktop/server applications
    Native,
    /// Embedded systems with resource constraints
    Embedded,
    /// WebAssembly System Interface (WASI)
    WASI,
}

/// Capabilities and features supported by a target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetCapabilities {
    /// Supports async/await operations
    pub async_support: bool,
    /// Supports multi-threading
    pub threads_support: bool,
    /// Supports SIMD instructions
    pub simd_support: bool,
    /// Supports bulk memory operations
    pub bulk_memory: bool,
    /// Supports reference types
    pub reference_types: bool,
    /// Supports 64-bit memory addressing
    pub memory64: bool,
    /// Supports component model
    pub component_model: bool,
    /// Supports WebAssembly System Interface
    pub wasi_support: bool,
    /// Maximum memory size in bytes
    pub max_memory_size: Option<usize>,
    /// Supports file system operations
    pub filesystem_access: bool,
    /// Supports network operations
    pub network_access: bool,
}

/// Set of host functions available for a target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostFunctionSet {
    /// Core system functions (print, memory management)
    pub core_functions: bool,
    /// I/O operations (file, console)
    pub io_functions: bool,
    /// Network operations (HTTP, WebSocket)
    pub network_functions: bool,
    /// Graphics and rendering functions
    pub graphics_functions: bool,
    /// Audio processing functions
    pub audio_functions: bool,
    /// System integration functions
    pub system_functions: bool,
    /// Cryptographic functions
    pub crypto_functions: bool,
    /// Database integration functions
    pub database_functions: bool,
}

/// Target-specific optimization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetOptimizations {
    /// Optimize for code size over speed
    pub optimize_for_size: bool,
    /// Enable dead code elimination
    pub dead_code_elimination: bool,
    /// Enable function inlining
    pub inline_functions: bool,
    /// Enable loop unrolling
    pub loop_unrolling: bool,
    /// Enable vectorization
    pub vectorization: bool,
    /// Target-specific compiler flags
    pub custom_flags: Vec<String>,
}

impl Target {
    /// Create a web browser target configuration
    pub fn web() -> Self {
        Self {
            name: "web".to_string(),
            description: "WebAssembly for web browsers".to_string(),
            target_type: TargetType::Web,
            runtime_preference: RuntimeType::Auto, // Auto-select based on browser capabilities
            capabilities: TargetCapabilities {
                async_support: true,
                threads_support: true, // SharedArrayBuffer support
                simd_support: true,
                bulk_memory: true,
                reference_types: true,
                memory64: false, // Not widely supported in browsers yet
                component_model: false,
                wasi_support: false,
                max_memory_size: Some(2 * 1024 * 1024 * 1024), // 2GB browser limit
                filesystem_access: false, // Limited to virtual filesystem
                network_access: true,
            },
            host_functions: HostFunctionSet {
                core_functions: true,
                io_functions: true, // Console, limited file operations
                network_functions: true, // Fetch API, WebSocket
                graphics_functions: true, // Canvas, WebGL
                audio_functions: true, // Web Audio API
                system_functions: false, // No direct system access
                crypto_functions: true, // Web Crypto API
                database_functions: true, // IndexedDB, WebSQL
            },
            optimizations: TargetOptimizations {
                optimize_for_size: true, // Important for download time
                dead_code_elimination: true,
                inline_functions: true,
                loop_unrolling: false, // Can increase size
                vectorization: true,
                custom_flags: vec!["--enable-bulk-memory".to_string()],
            },
            environment: {
                let mut env = HashMap::new();
                env.insert("target".to_string(), "web".to_string());
                env.insert("platform".to_string(), "browser".to_string());
                env
            },
        }
    }

    /// Create a Node.js target configuration
    pub fn nodejs() -> Self {
        Self {
            name: "nodejs".to_string(),
            description: "WebAssembly for Node.js runtime".to_string(),
            target_type: TargetType::NodeJS,
            runtime_preference: RuntimeType::Wasmtime, // Better Node.js integration
            capabilities: TargetCapabilities {
                async_support: true,
                threads_support: true,
                simd_support: true,
                bulk_memory: true,
                reference_types: true,
                memory64: false,
                component_model: false,
                wasi_support: true, // Node.js WASI support
                max_memory_size: None, // No strict limit
                filesystem_access: true,
                network_access: true,
            },
            host_functions: HostFunctionSet {
                core_functions: true,
                io_functions: true,
                network_functions: true,
                graphics_functions: false, // No direct graphics in Node.js
                audio_functions: false,
                system_functions: true, // Access to Node.js APIs
                crypto_functions: true,
                database_functions: true,
            },
            optimizations: TargetOptimizations {
                optimize_for_size: false, // Favor speed over size
                dead_code_elimination: true,
                inline_functions: true,
                loop_unrolling: true,
                vectorization: true,
                custom_flags: vec!["--enable-threads".to_string()],
            },
            environment: {
                let mut env = HashMap::new();
                env.insert("target".to_string(), "nodejs".to_string());
                env.insert("platform".to_string(), "node".to_string());
                env
            },
        }
    }

    /// Create a native application target configuration
    pub fn native() -> Self {
        Self {
            name: "native".to_string(),
            description: "Native desktop/server applications".to_string(),
            target_type: TargetType::Native,
            runtime_preference: RuntimeType::Wasmtime, // Best performance for native
            capabilities: TargetCapabilities {
                async_support: true,
                threads_support: true,
                simd_support: true,
                bulk_memory: true,
                reference_types: true,
                memory64: true, // Native applications can support larger memory
                component_model: true,
                wasi_support: true,
                max_memory_size: None, // Limited by system RAM
                filesystem_access: true,
                network_access: true,
            },
            host_functions: HostFunctionSet {
                core_functions: true,
                io_functions: true,
                network_functions: true,
                graphics_functions: true,
                audio_functions: true,
                system_functions: true,
                crypto_functions: true,
                database_functions: true,
            },
            optimizations: TargetOptimizations {
                optimize_for_size: false,
                dead_code_elimination: true,
                inline_functions: true,
                loop_unrolling: true,
                vectorization: true,
                custom_flags: vec![
                    "--enable-all".to_string(),
                    "--opt-level=speed".to_string(),
                ],
            },
            environment: {
                let mut env = HashMap::new();
                env.insert("target".to_string(), "native".to_string());
                env.insert("platform".to_string(), "desktop".to_string());
                env
            },
        }
    }

    /// Create an embedded systems target configuration
    pub fn embedded() -> Self {
        Self {
            name: "embedded".to_string(),
            description: "Embedded systems with resource constraints".to_string(),
            target_type: TargetType::Embedded,
            runtime_preference: RuntimeType::Wasmer, // Smaller footprint
            capabilities: TargetCapabilities {
                async_support: false, // Often not needed in embedded
                threads_support: false,
                simd_support: false,
                bulk_memory: true,
                reference_types: false,
                memory64: false,
                component_model: false,
                wasi_support: false,
                max_memory_size: Some(1024 * 1024), // 1MB limit
                filesystem_access: false,
                network_access: false,
            },
            host_functions: HostFunctionSet {
                core_functions: true,
                io_functions: false, // Minimal I/O
                network_functions: false,
                graphics_functions: false,
                audio_functions: false,
                system_functions: false,
                crypto_functions: false,
                database_functions: false,
            },
            optimizations: TargetOptimizations {
                optimize_for_size: true, // Critical for embedded
                dead_code_elimination: true,
                inline_functions: false, // Can increase size
                loop_unrolling: false,
                vectorization: false,
                custom_flags: vec![
                    "--optimize-size".to_string(),
                    "--no-debug".to_string(),
                ],
            },
            environment: {
                let mut env = HashMap::new();
                env.insert("target".to_string(), "embedded".to_string());
                env.insert("platform".to_string(), "microcontroller".to_string());
                env
            },
        }
    }

    /// Create a WASI (WebAssembly System Interface) target configuration
    pub fn wasi() -> Self {
        Self {
            name: "wasi".to_string(),
            description: "WebAssembly System Interface for portable system integration".to_string(),
            target_type: TargetType::WASI,
            runtime_preference: RuntimeType::Wasmtime, // Best WASI support
            capabilities: TargetCapabilities {
                async_support: false, // WASI doesn't define async primitives yet
                threads_support: false, // Limited thread support in current WASI
                simd_support: true,
                bulk_memory: true,
                reference_types: true,
                memory64: false,
                component_model: true, // Component model aligns with WASI
                wasi_support: true,
                max_memory_size: None,
                filesystem_access: true, // Core WASI feature
                network_access: false, // Not in current WASI spec
            },
            host_functions: HostFunctionSet {
                core_functions: true,
                io_functions: true, // WASI I/O
                network_functions: false,
                graphics_functions: false,
                audio_functions: false,
                system_functions: true, // WASI system calls
                crypto_functions: false,
                database_functions: false,
            },
            optimizations: TargetOptimizations {
                optimize_for_size: false,
                dead_code_elimination: true,
                inline_functions: true,
                loop_unrolling: true,
                vectorization: true,
                custom_flags: vec![
                    "--enable-wasi".to_string(),
                    "--enable-component-model".to_string(),
                ],
            },
            environment: {
                let mut env = HashMap::new();
                env.insert("target".to_string(), "wasi".to_string());
                env.insert("platform".to_string(), "wasi".to_string());
                env
            },
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} - {}", self.name, self.description)
    }
}

impl fmt::Display for TargetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetType::Web => write!(f, "web"),
            TargetType::NodeJS => write!(f, "nodejs"),
            TargetType::Native => write!(f, "native"),
            TargetType::Embedded => write!(f, "embedded"),
            TargetType::WASI => write!(f, "wasi"),
        }
    }
}

impl Default for TargetCapabilities {
    fn default() -> Self {
        Self {
            async_support: true,
            threads_support: true,
            simd_support: true,
            bulk_memory: true,
            reference_types: true,
            memory64: false,
            component_model: false,
            wasi_support: false,
            max_memory_size: None,
            filesystem_access: false,
            network_access: false,
        }
    }
}

impl Default for HostFunctionSet {
    fn default() -> Self {
        Self {
            core_functions: true,
            io_functions: false,
            network_functions: false,
            graphics_functions: false,
            audio_functions: false,
            system_functions: false,
            crypto_functions: false,
            database_functions: false,
        }
    }
}

impl Default for TargetOptimizations {
    fn default() -> Self {
        Self {
            optimize_for_size: false,
            dead_code_elimination: true,
            inline_functions: true,
            loop_unrolling: false,
            vectorization: false,
            custom_flags: Vec::new(),
        }
    }
}