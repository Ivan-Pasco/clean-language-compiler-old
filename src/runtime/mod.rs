// Clean Language WebAssembly Runtime with Async Support
// Provides enhanced runtime capabilities for async programming features

use crate::error::CompilerError;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(feature = "wasmtime-runtime")]
use wasmtime::{Engine, Linker, Module, Store};

pub mod async_runtime;
pub mod file_io;
pub mod future_resolver;
pub mod host_functions;
pub mod http_client;
pub mod task_scheduler;
pub mod wasmtime_config;

// New runtime abstraction modules
pub mod runtime_manager;
pub mod runtime_trait;
pub mod wasmer_config;
pub mod wasmtime_runtime;

// Note: async_tests module removed due to missing implementation

/// Enhanced WebAssembly runtime with async support
#[cfg(feature = "wasmtime-runtime")]
pub struct CleanRuntime {
    engine: Engine,
    #[allow(dead_code)]
    task_scheduler: Arc<Mutex<TaskScheduler>>,
    #[allow(dead_code)]
    future_resolver: Arc<Mutex<FutureResolver>>,
    background_tasks: Arc<Mutex<Vec<BackgroundTask>>>,
}

/// Represents a background task running in the runtime
#[derive(Debug)]
pub struct BackgroundTask {
    pub id: u32,
    pub name: String,
    pub started_at: Instant,
    pub status: TaskStatus,
}

/// Status of a background task
#[derive(Debug, Clone)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed(String),
}

/// Task scheduler for managing async operations
pub struct TaskScheduler {
    next_task_id: u32,
    running_tasks: HashMap<u32, BackgroundTask>,
}

/// Future resolver for handling later assignments
pub struct FutureResolver {
    futures: HashMap<String, FutureValue>,
}

/// Represents a future value that will be resolved later
#[derive(Debug, Clone)]
pub struct FutureValue {
    pub id: String,
    pub value: Option<i32>, // For now, using i32 as the basic value type
    pub resolved: bool,
    pub created_at: Instant,
}

#[cfg(feature = "wasmtime-runtime")]
impl CleanRuntime {
    /// Create a new Clean Language runtime with async support
    pub fn new() -> Result<Self, CompilerError> {
        // Initialize HTTP client
        http_client::init_http_client();

        // Use centralized wasmtime configuration for consistency
        let engine = wasmtime_config::CleanWasmtimeConfig::create_engine()?;

        Ok(CleanRuntime {
            engine,
            task_scheduler: Arc::new(Mutex::new(TaskScheduler::new())),
            future_resolver: Arc::new(Mutex::new(FutureResolver::new())),
            background_tasks: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Execute a WebAssembly module with async support
    pub async fn execute_async(&self, wasm_bytes: &[u8]) -> Result<(), CompilerError> {
        let module = Module::new(&self.engine, wasm_bytes).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create WebAssembly module: {e}"),
                None,
                None,
            )
        })?;

        let mut store = Store::new(&self.engine, ());
        let mut linker = Linker::new(&self.engine);

        // Add all host functions using centralized registry
        host_functions::register_all_host_functions(&mut linker)?;

        // Instantiate the module
        let instance = linker
            .instantiate_async(&mut store, &module)
            .await
            .map_err(|e| {
                CompilerError::runtime_error(
                    format!("Failed to instantiate WebAssembly module: {e}"),
                    None,
                    None,
                )
            })?;

        // Execute the start function
        if let Some(start_func) = instance.get_func(&mut store, "start") {
            println!("🚀 Executing Clean Language program with async support...");
            println!("--- Output ---");

            // Check the function signature to create the right results buffer
            let start_type = start_func.ty(&store);
            let results_len = start_type.results().len();

            // Create a buffer to store return values
            let mut results = vec![wasmtime::Val::I32(0); results_len];

            start_func
                .call_async(&mut store, &[], &mut results)
                .await
                .map_err(|e| {
                    CompilerError::runtime_error(
                        format!("Runtime error during execution: {e}"),
                        None,
                        None,
                    )
                })?;

            println!("--- End Output ---");

            // If there are return values, print them
            if !results.is_empty() {
                println!("Return value: {:?}", results[0]);
            }

            // Wait for background tasks to complete
            self.wait_for_background_tasks().await;

            println!("✅ Execution completed successfully!");
        } else {
            return Err(CompilerError::runtime_error(
                "No start function found in WebAssembly module".to_string(),
                None,
                None,
            ));
        }

        Ok(())
    }

    /// Wait for all background tasks to complete
    async fn wait_for_background_tasks(&self) {
        let mut completed = false;
        let mut iterations = 0;
        const MAX_WAIT_ITERATIONS: u32 = 100; // Prevent infinite waiting

        while !completed && iterations < MAX_WAIT_ITERATIONS {
            {
                let tasks = self.background_tasks.lock().unwrap();
                completed = tasks.iter().all(|task| {
                    matches!(task.status, TaskStatus::Completed | TaskStatus::Failed(_))
                });

                if !completed {
                    let running_count = tasks
                        .iter()
                        .filter(|task| matches!(task.status, TaskStatus::Running))
                        .count();
                    if running_count > 0 {
                        println!(
                            "⏳ Waiting for {running_count} background task(s) to complete..."
                        );
                    }
                }
            }

            if !completed {
                tokio::time::sleep(Duration::from_millis(50)).await;
                iterations += 1;
            }
        }

        if iterations >= MAX_WAIT_ITERATIONS {
            println!("⚠️  Timeout waiting for background tasks to complete");
        } else {
            println!("✅ All background tasks completed");
        }
    }
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskScheduler {
    pub fn new() -> Self {
        TaskScheduler {
            next_task_id: 1,
            running_tasks: HashMap::new(),
        }
    }

    pub fn create_task(&mut self, name: String) -> u32 {
        let task_id = self.next_task_id;
        self.next_task_id += 1;

        let task = BackgroundTask {
            id: task_id,
            name,
            started_at: Instant::now(),
            status: TaskStatus::Running,
        };

        self.running_tasks.insert(task_id, task);
        task_id
    }

    pub fn complete_task(&mut self, task_id: u32) {
        if let Some(task) = self.running_tasks.get_mut(&task_id) {
            task.status = TaskStatus::Completed;
        }
    }

    pub fn fail_task(&mut self, task_id: u32, error: String) {
        if let Some(task) = self.running_tasks.get_mut(&task_id) {
            task.status = TaskStatus::Failed(error);
        }
    }
}

impl Default for FutureResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl FutureResolver {
    pub fn new() -> Self {
        FutureResolver {
            futures: HashMap::new(),
        }
    }

    pub fn create_future(&mut self, id: String) {
        let future = FutureValue {
            id: id.clone(),
            value: None,
            resolved: false,
            created_at: Instant::now(),
        };
        self.futures.insert(id, future);
    }

    pub fn resolve_future(&mut self, id: String, value: i32) {
        if let Some(future) = self.futures.get_mut(&id) {
            future.value = Some(value);
            future.resolved = true;
        }
    }

    pub fn get_future_value(&self, id: &str) -> Option<i32> {
        self.futures
            .get(id)
            .and_then(|f| if f.resolved { f.value } else { None })
    }

    pub fn is_future_resolved(&self, id: &str) -> bool {
        self.futures.get(id).map(|f| f.resolved).unwrap_or(false)
    }
}

/// Convenience function to create and run a Clean Language program with async support
#[cfg(feature = "wasmtime-runtime")]
pub async fn run_clean_program_async(wasm_bytes: &[u8]) -> Result<(), CompilerError> {
    let runtime = CleanRuntime::new()?;
    runtime.execute_async(wasm_bytes).await
}

/// Synchronous wrapper for async execution (for backward compatibility)
#[cfg(feature = "wasmtime-runtime")]
pub fn run_clean_program_sync(wasm_bytes: &[u8]) -> Result<(), CompilerError> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| {
        CompilerError::runtime_error(format!("Failed to create async runtime: {e}"), None, None)
    })?;

    rt.block_on(run_clean_program_async(wasm_bytes))
}
