#[cfg(feature = "wasmer-runtime")]
use crate::error::CompilerError;

#[cfg(feature = "wasmer-runtime")]
use crate::runtime::runtime_trait::{
    HostFunctionRegistry, OptimizationLevel, RuntimeConfig, RuntimeFeature, RuntimeValue, 
    ValueType, WebAssemblyRuntime,
};

#[cfg(feature = "wasmer-runtime")]
use wasmer::{Engine, Instance, Module, Store, Function, Value, RuntimeError};

#[cfg(feature = "wasmer-runtime")]
use std::sync::Arc;

/// Wasmer implementation of the WebAssembly runtime trait
#[cfg(feature = "wasmer-runtime")]
pub struct WasmerRuntime;

#[cfg(feature = "wasmer-runtime")]
impl WebAssemblyRuntime for WasmerRuntime {
    type Engine = Engine;
    type Store = Store;
    type Module = Module;
    type Instance = Instance;
    type Function = Function;
    type Value = Value;

    fn create_engine(config: &RuntimeConfig) -> Result<Self::Engine, CompilerError> {
        let mut compiler_config = wasmer::Cranelift::default();
        
        // Set optimization level
        match config.optimization_level {
            OptimizationLevel::None => {
                compiler_config = compiler_config.opt_level(wasmer::OptLevel::None);
            }
            OptimizationLevel::Speed => {
                compiler_config = compiler_config.opt_level(wasmer::OptLevel::Speed);
            }
            OptimizationLevel::SpeedAndSize => {
                compiler_config = compiler_config.opt_level(wasmer::OptLevel::SpeedAndSize);
            }
        }

        let mut store_config = wasmer::EngineBuilder::new(compiler_config);

        // Enable features based on configuration
        if config.bulk_memory {
            store_config = store_config.set_features(Some(wasmer::Features {
                bulk_memory: true,
                ..wasmer::Features::default()
            }));
        }

        let engine = store_config.build();
        Ok(engine)
    }

    fn create_store(engine: &Self::Engine) -> Result<Self::Store, CompilerError> {
        Ok(Store::new(engine))
    }

    fn create_module(engine: &Self::Engine, wasm_bytes: &[u8]) -> Result<Self::Module, CompilerError> {
        Module::new(engine, wasm_bytes).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create Wasmer module: {e}"),
                None,
                None,
            )
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
                let params: Vec<wasmer::Type> = host_func.signature.params
                    .iter()
                    .map(|p| value_type_to_wasmer_type(*p))
                    .collect();
                    
                let results: Vec<wasmer::Type> = host_func.signature.results
                    .iter()
                    .map(|r| value_type_to_wasmer_type(*r))
                    .collect();
                    
                let func_type = wasmer::FunctionType::new(params, results);
                
                // Create wrapper for the callback
                let callback = Arc::new(host_func.callback.as_ref());
                let func = Function::new_native_with_env(
                    store,
                    Arc::clone(&callback),
                    move |env: &Arc<dyn Fn(&[RuntimeValue]) -> Result<Vec<RuntimeValue>, CompilerError> + Send + Sync>, args: &[Value]| -> Result<Vec<Value>, RuntimeError> {
                        // Convert Wasmer values to runtime values
                        let runtime_args: Vec<RuntimeValue> = args.iter()
                            .map(|v| wasmer_value_to_runtime_value(v))
                            .collect();
                            
                        // Call the host function
                        match env(&runtime_args) {
                            Ok(results) => {
                                let wasmer_results: Vec<Value> = results.iter()
                                    .map(|v| runtime_value_to_wasmer_value(v))
                                    .collect();
                                Ok(wasmer_results)
                            }
                            Err(e) => Err(RuntimeError::new(format!("Host function error: {e}"))),
                        }
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
        store: &mut Self::Store,
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
        function.call(store, args).map_err(|e| {
            CompilerError::runtime_error(
                format!("Function call failed: {e}"),
                None,
                None,
            )
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
            0x07, 0x08, 0x01, 0x04, 0x74, 0x65, 0x73, 0x74, 0x00, 0x00, // export section: export "test" function 0
            0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b, // code section: function 0 body is empty (just end)
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