#[cfg(feature = "wasmtime-runtime")]
use crate::error::CompilerError;

#[cfg(feature = "wasmtime-runtime")]
use crate::runtime::runtime_trait::{
    HostFunctionRegistry, OptimizationLevel, RuntimeConfig, RuntimeFeature, RuntimeValue,
    ValueType, WebAssemblyRuntime,
};

#[cfg(feature = "wasmtime-runtime")]
use wasmtime::{Config, Engine, Func, Instance, Linker, Module, Store, Val, ValType};

/// Wasmtime implementation of the WebAssembly runtime trait
#[cfg(feature = "wasmtime-runtime")]
pub struct WasmtimeRuntime;

#[cfg(feature = "wasmtime-runtime")]
impl WebAssemblyRuntime for WasmtimeRuntime {
    type Engine = Engine;
    type Store = Store<()>;
    type Module = Module;
    type Instance = Instance;
    type Function = Func;
    type Value = Val;

    fn create_engine(config: &RuntimeConfig) -> Result<Self::Engine, CompilerError> {
        let mut wasmtime_config = Config::new();

        // Configure async support
        wasmtime_config.async_support(config.async_support);

        // Configure threading
        wasmtime_config.wasm_threads(config.threads_support);

        // Configure SIMD
        wasmtime_config.wasm_simd(config.simd_support);

        // Configure bulk memory operations
        wasmtime_config.wasm_bulk_memory(config.bulk_memory);

        // Configure reference types
        wasmtime_config.wasm_reference_types(config.reference_types);

        // Set optimization level
        match config.optimization_level {
            OptimizationLevel::None => {
                wasmtime_config.cranelift_opt_level(wasmtime::OptLevel::None);
            }
            OptimizationLevel::Speed => {
                wasmtime_config.cranelift_opt_level(wasmtime::OptLevel::Speed);
            }
            OptimizationLevel::SpeedAndSize => {
                wasmtime_config.cranelift_opt_level(wasmtime::OptLevel::SpeedAndSize);
            }
        }

        // Configure memory settings
        wasmtime_config
            .static_memory_maximum_size(config.memory_config.static_memory_maximum as u64);
        wasmtime_config.dynamic_memory_guard_size(config.memory_config.dynamic_memory_guard as u64);

        // Configure debug information
        wasmtime_config.debug_info(config.debug_info);
        if config.debug_info {
            wasmtime_config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
        } else {
            wasmtime_config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Disable);
        }

        Engine::new(&wasmtime_config).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create Wasmtime engine: {e}"),
                None,
                None,
            )
        })
    }

    fn create_store(engine: &Self::Engine) -> Result<Self::Store, CompilerError> {
        Ok(Store::new(engine, ()))
    }

    fn create_module(
        engine: &Self::Engine,
        wasm_bytes: &[u8],
    ) -> Result<Self::Module, CompilerError> {
        Module::new(engine, wasm_bytes).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create Wasmtime module: {e}"),
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
        let mut linker = Linker::new(store.engine());

        // Register host functions
        for (name, host_func) in &host_functions.functions {
            let parts: Vec<&str> = name.split("::").collect();
            if parts.len() == 2 {
                let module_name = parts[0];
                let func_name = parts[1];

                // Convert function signature
                let params: Vec<ValType> = host_func
                    .signature
                    .params
                    .iter()
                    .map(|p| value_type_to_wasmtime_type(*p))
                    .collect();

                let results: Vec<ValType> = host_func
                    .signature
                    .results
                    .iter()
                    .map(|r| value_type_to_wasmtime_type(*r))
                    .collect();

                // Create a simple host function placeholder for now
                linker
                    .func_wrap(module_name, func_name, || -> i32 { 0 })
                    .map_err(|e| {
                        CompilerError::runtime_error(
                            format!("Failed to define host function {name}: {e}"),
                            None,
                            None,
                        )
                    })?;
            }
        }

        linker.instantiate(store, module).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to instantiate Wasmtime module: {e}"),
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
        match instance.get_func(store, name) {
            Some(func) => Ok(Some(func)),
            None => Ok(None),
        }
    }

    fn call_function(
        store: &mut Self::Store,
        function: &Self::Function,
        args: &[Self::Value],
    ) -> Result<Vec<Self::Value>, CompilerError> {
        let mut results = vec![Val::I32(0); function.ty(&*store).results().len()];
        function
            .call(&mut *store, args, &mut results)
            .map_err(|e| {
                CompilerError::runtime_error(format!("Function call failed: {e}"), None, None)
            })?;
        Ok(results)
    }

    fn runtime_name() -> &'static str {
        "Wasmtime"
    }

    fn runtime_version() -> &'static str {
        "20.0" // Match the version in Cargo.toml
    }

    fn supports_feature(feature: RuntimeFeature) -> bool {
        match feature {
            RuntimeFeature::AsyncSupport => true,
            RuntimeFeature::Threads => true,
            RuntimeFeature::Simd => true,
            RuntimeFeature::BulkMemory => true,
            RuntimeFeature::ReferenceTypes => true,
            RuntimeFeature::Memory64 => true,
            RuntimeFeature::ComponentModel => true,
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

#[cfg(feature = "wasmtime-runtime")]
fn value_type_to_wasmtime_type(value_type: ValueType) -> ValType {
    match value_type {
        ValueType::I32 => ValType::I32,
        ValueType::I64 => ValType::I64,
        ValueType::F32 => ValType::F32,
        ValueType::F64 => ValType::F64,
    }
}

#[cfg(feature = "wasmtime-runtime")]
fn wasmtime_value_to_runtime_value(value: &Val) -> RuntimeValue {
    match value {
        Val::I32(v) => RuntimeValue::I32(*v),
        Val::I64(v) => RuntimeValue::I64(*v),
        Val::F32(v) => RuntimeValue::F32(f32::from_bits(*v)),
        Val::F64(v) => RuntimeValue::F64(f64::from_bits(*v)),
        _ => RuntimeValue::I32(0), // Default fallback
    }
}

#[cfg(feature = "wasmtime-runtime")]
fn runtime_value_to_wasmtime_value(value: &RuntimeValue) -> Val {
    match value {
        RuntimeValue::I32(v) => Val::I32(*v),
        RuntimeValue::I64(v) => Val::I64(*v),
        RuntimeValue::F32(v) => Val::F32(v.to_bits()),
        RuntimeValue::F64(v) => Val::F64(v.to_bits()),
    }
}

// Stub implementations when Wasmtime feature is not enabled
#[cfg(not(feature = "wasmtime-runtime"))]
pub struct WasmtimeRuntime;

// Export the CleanWasmtimeConfig for backward compatibility
#[cfg(feature = "wasmtime-runtime")]
pub use crate::runtime::wasmtime_config::CleanWasmtimeConfig;
