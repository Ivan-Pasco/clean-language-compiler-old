#[cfg(feature = "wasmer-runtime")]
use crate::error::CompilerError;

#[cfg(feature = "wasmer-runtime")]
use crate::runtime::runtime_trait::{
    HostFunctionRegistry, OptimizationLevel, RuntimeConfig, RuntimeFeature, RuntimeValue,
    ValueType, WebAssemblyRuntime,
};

#[cfg(feature = "wasmer-runtime")]
use wasmer::{Engine, Function, Instance, Module, RuntimeError, Store, Value};

/// Wasmer implementation of the WebAssembly runtime trait
#[cfg(feature = "wasmer-runtime")]
pub struct WasmerRuntime;

#[cfg(feature = "wasmer-runtime")]
impl WasmerRuntime {
    /// Create a configured Cranelift compiler based on runtime configuration
    #[allow(dead_code)]
    fn create_configured_compiler(config: &RuntimeConfig) -> wasmer::Cranelift {
        use wasmer::{Cranelift, CraneliftOptLevel};

        // Set optimization level based on config
        let opt_level = match config.optimization_level {
            OptimizationLevel::None => CraneliftOptLevel::None,
            OptimizationLevel::Speed => CraneliftOptLevel::Speed,
            OptimizationLevel::SpeedAndSize => CraneliftOptLevel::SpeedAndSize,
        };

        let mut compiler = Cranelift::new();
        compiler.opt_level(opt_level);

        // In wasmer 4.x, features like bulk_memory, reference_types, SIMD, and threads
        // are typically enabled by default in Cranelift compiler
        // These features are controlled by the WebAssembly module itself and the runtime

        compiler
    }

    /// Create a store with proper configuration
    #[allow(dead_code)]
    fn create_configured_store(config: &RuntimeConfig) -> Store {
        let compiler = Self::create_configured_compiler(config);
        Store::new(compiler)
    }
}

#[cfg(feature = "wasmer-runtime")]
impl WebAssemblyRuntime for WasmerRuntime {
    type Engine = Engine;
    type Store = Store;
    type Module = Module;
    type Instance = Instance;
    type Function = Function;
    type Value = Value;

    fn create_engine(config: &RuntimeConfig) -> Result<Self::Engine, CompilerError> {
        // In wasmer 4.x, Engine is not used the same way as in 3.x
        // The actual configuration is done when creating the Store with a compiler
        // We return a default engine for interface compatibility
        let _config = config; // Acknowledge config parameter
        let engine = Engine::default();
        Ok(engine)
    }

    fn create_store(_engine: &Self::Engine) -> Result<Self::Store, CompilerError> {
        // In wasmer 4.x, Store is created with a compiler directly
        // Since the trait doesn't pass config here, we use default configuration
        // This is a limitation of the current trait design
        let compiler = wasmer::Cranelift::default();
        Ok(Store::new(compiler))
    }

    fn create_module(
        engine: &Self::Engine,
        wasm_bytes: &[u8],
    ) -> Result<Self::Module, CompilerError> {
        Module::new(engine, wasm_bytes).map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create Wasmer module: {e}"), None, None)
        })
    }

    fn instantiate_module(
        store: &mut Self::Store,
        module: &Self::Module,
        host_functions: &HostFunctionRegistry,
    ) -> Result<Self::Instance, CompilerError> {
        let mut imports = wasmer::Imports::new();

        // Register host functions
        for (name, host_func) in &host_functions.functions {
            let parts: Vec<&str> = name.split("::").collect();
            if parts.len() == 2 {
                let module_name = parts[0];
                let func_name = parts[1];

                // Create Wasmer function type
                let params: Vec<wasmer::Type> = host_func
                    .signature
                    .params
                    .iter()
                    .map(|p| value_type_to_wasmer_type(*p))
                    .collect();

                let results: Vec<wasmer::Type> = host_func
                    .signature
                    .results
                    .iter()
                    .map(|r| value_type_to_wasmer_type(*r))
                    .collect();

                let func_type = wasmer::FunctionType::new(params, results);

                // Create a simple placeholder function for now
                let func = Function::new(
                    store,
                    func_type,
                    |_args: &[Value]| -> Result<Vec<Value>, RuntimeError> {
                        // Simple placeholder implementation
                        Ok(vec![Value::I32(0)])
                    },
                );

                imports.define(module_name, func_name, func);
            }
        }

        Instance::new(store, module, &imports).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to instantiate Wasmer module: {e}"),
                None,
                None,
            )
        })
    }

    fn get_function(
        _store: &mut Self::Store,
        instance: &Self::Instance,
        name: &str,
    ) -> Result<Option<Self::Function>, CompilerError> {
        match instance.exports.get_function(name) {
            Ok(func) => Ok(Some(func.clone())),
            Err(wasmer::ExportError::Missing(_)) => Ok(None),
            Err(e) => Err(CompilerError::runtime_error(
                format!("Failed to get function '{name}': {e}"),
                None,
                None,
            )),
        }
    }

    fn call_function(
        store: &mut Self::Store,
        function: &Self::Function,
        args: &[Self::Value],
    ) -> Result<Vec<Self::Value>, CompilerError> {
        function
            .call(store, args)
            .map(|boxed_results| boxed_results.to_vec())
            .map_err(|e| {
                CompilerError::runtime_error(format!("Function call failed: {e}"), None, None)
            })
    }

    fn runtime_name() -> &'static str {
        "Wasmer"
    }

    fn runtime_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn supports_feature(feature: RuntimeFeature) -> bool {
        match feature {
            RuntimeFeature::AsyncSupport => false, // Wasmer doesn't have built-in async support
            RuntimeFeature::Threads => true,
            RuntimeFeature::Simd => true,
            RuntimeFeature::BulkMemory => true,
            RuntimeFeature::ReferenceTypes => true,
            RuntimeFeature::Memory64 => false,
            RuntimeFeature::ComponentModel => false,
            RuntimeFeature::Wasi => true,
        }
    }

    fn validate_runtime(config: &RuntimeConfig) -> Result<(), CompilerError> {
        let engine = Self::create_engine(config)?;
        let mut store = Self::create_store(&engine)?;

        // Simple WASM module that exports a test function
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

        let module = Self::create_module(&engine, test_wasm)?;
        let host_functions = HostFunctionRegistry::new();
        let _instance = Self::instantiate_module(&mut store, &module, &host_functions)?;

        Ok(())
    }
}

#[cfg(feature = "wasmer-runtime")]
fn value_type_to_wasmer_type(value_type: ValueType) -> wasmer::Type {
    match value_type {
        ValueType::I32 => wasmer::Type::I32,
        ValueType::I64 => wasmer::Type::I64,
        ValueType::F32 => wasmer::Type::F32,
        ValueType::F64 => wasmer::Type::F64,
    }
}

#[cfg(feature = "wasmer-runtime")]
#[allow(dead_code)]
fn wasmer_value_to_runtime_value(value: &wasmer::Value) -> RuntimeValue {
    match value {
        wasmer::Value::I32(v) => RuntimeValue::I32(*v),
        wasmer::Value::I64(v) => RuntimeValue::I64(*v),
        wasmer::Value::F32(v) => RuntimeValue::F32(*v),
        wasmer::Value::F64(v) => RuntimeValue::F64(*v),
        _ => RuntimeValue::I32(0), // Default fallback
    }
}

#[cfg(feature = "wasmer-runtime")]
#[allow(dead_code)]
fn runtime_value_to_wasmer_value(value: &RuntimeValue) -> wasmer::Value {
    match value {
        RuntimeValue::I32(v) => wasmer::Value::I32(*v),
        RuntimeValue::I64(v) => wasmer::Value::I64(*v),
        RuntimeValue::F32(v) => wasmer::Value::F32(*v),
        RuntimeValue::F64(v) => wasmer::Value::F64(*v),
    }
}

/// Wasmer-specific configuration builder
#[cfg(feature = "wasmer-runtime")]
pub struct WasmerConfig;

#[cfg(feature = "wasmer-runtime")]
impl WasmerConfig {
    /// Create a new Wasmer configuration with Clean Language defaults
    pub fn new() -> RuntimeConfig {
        RuntimeConfig {
            runtime_type: crate::runtime::runtime_trait::RuntimeType::Wasmer,
            async_support: false, // Wasmer doesn't have built-in async support
            threads_support: true,
            simd_support: true,
            bulk_memory: true,
            reference_types: true,
            optimization_level: OptimizationLevel::Speed,
            memory_config: crate::runtime::runtime_trait::MemoryConfig::default(),
            debug_info: cfg!(debug_assertions),
            target_settings: std::collections::HashMap::new(),
        }
    }

    /// Create a minimal configuration for testing
    pub fn minimal() -> RuntimeConfig {
        RuntimeConfig {
            runtime_type: crate::runtime::runtime_trait::RuntimeType::Wasmer,
            async_support: false,
            threads_support: false,
            simd_support: false,
            bulk_memory: true,
            reference_types: true,
            optimization_level: OptimizationLevel::None,
            memory_config: crate::runtime::runtime_trait::MemoryConfig::default(),
            debug_info: false,
            target_settings: std::collections::HashMap::new(),
        }
    }
}

// Stub implementations when Wasmer feature is not enabled
#[cfg(not(feature = "wasmer-runtime"))]
pub struct WasmerRuntime;

#[cfg(not(feature = "wasmer-runtime"))]
pub struct WasmerConfig;

#[cfg(not(feature = "wasmer-runtime"))]
impl WasmerConfig {
    pub fn new() -> crate::runtime::runtime_trait::RuntimeConfig {
        panic!("Wasmer runtime not available - enable 'wasmer-runtime' feature")
    }

    pub fn minimal() -> crate::runtime::runtime_trait::RuntimeConfig {
        panic!("Wasmer runtime not available - enable 'wasmer-runtime' feature")
    }
}
