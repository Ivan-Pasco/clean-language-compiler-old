// Compile-time runtime metadata: which WebAssembly runtime a target prefers.
// The trait/host-bridge machinery that used to live here (WebAssemblyRuntime,
// HostFunctionRegistry, RuntimeFeature, ValueType) belongs to the host
// runtime, not the compiler — see
// foundation/management/ARCHITECTURE_BOUNDARIES.md.

/// Runtime selection preference.
#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    /// Runtime type preference
    pub runtime_type: RuntimeType,
}

/// Available WebAssembly runtime types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeType {
    /// Wasmtime runtime (Bytecode Alliance)
    Wasmtime,
    /// Wasmer runtime
    Wasmer,
    /// Auto-detect best runtime for current platform
    #[default]
    Auto,
}

impl RuntimeType {
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
